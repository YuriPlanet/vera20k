# Bridge System — Complete Binary Reference

Verified against `gamemd.exe` via Ghidra MCP. All addresses, offsets, constants,
and algorithms confirmed from live decompilation and assembly analysis.

## Core Principle

Bridges are **NOT slope types**. They use a completely separate system based on
cell flags, dual occupancy lists, and height-level arithmetic. The bridge surface
is always modeled as `ground_height_level + 4` in the height system.

## CellClass Bridge Fields

| Offset | Type | Field | Purpose |
|--------|------|-------|---------|
| +0x24 | i16+i16 | packed_cell | Packed cell coordinate {X, Y} |
| +0x38 | i32 | iso_tile_type_index | IsoTileType index (0xFFFF/-1 = clear) |
| +0x11A | byte | iso_sub_tile_idx | Universal IsoTileType sub-tile (icon) index. Consumed by `TMP_TileBlitter` for all terrain (sand/grass/slope/water/bridges). Bridge-rim matchers (`UpdateAdjacentBridges_High @ 0x576770`, `UpdateBridgeEdgeTiles_High @ 0x576200`) compare it against literal slot numbers (2, 4, 5, 7, 8, 12). NOT bridge-specific, NOT an orientation byte, NOT a damage state — damage state lives at `+0x11E`. See [CELL_0x11A_POLARITY_RECONCILE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/CELL_0x11A_POLARITY_RECONCILE_GHIDRA_REPORT.md). |
| +0x11B | i8 | height_level | Signed height level (each level = 15 pixels up) |
| +0x11C | u8 | slope_type | Terrain slope (0-20) — set by TMP_ReadSlopeType, NOT a bridge flag |
| +0x11E | u8 | bridge_damage_state | 18-state damage machine (0x00-0x11) |
| +0xE4 | ptr | ground_occupant_list | Linked list of objects at ground level |
| +0xE8 | ptr | bridge_occupant_list | Linked list of objects on bridge deck |
| +0xEC | i32 | land_type | Land type enum (used for speed table lookup) |
| +0x124 | u32 | ground_occupancy_bits | Bit 0x20 = ground occupied |
| +0x128 | u32 | bridge_occupancy_bits | Bit 0x20 = bridge occupied |
| +0x140 | u32 | cell_flags | Bitfield (see flag table below) |

### Cell Flags at +0x140

| Bit | Mask | Meaning | Set By |
|-----|------|---------|--------|
| 7 | 0x0080 | Has bridge overlay (body cell). Used by GetEffectiveHeight to add +4 | SetBridgeDirection (NOT RecalcAttributes) |
| 8 | 0x0100 | **Bridge structural cell** (head/ramp). Primary flag for movement/pathfinding | SetBridgeDirection, bridge state machine |
| 9 | 0x0200 | Bridgehead (entry/exit point). Required for bridge entry in Can_Enter_Cell | SetBridgeDirection |
| 10 | 0x0400 | **Bridge body cell, destroyed state** (mutually exclusive with bit 0x100 = alive). SetBridgeDirection_NESW @ 0x47E040 writes via `param_3 = (uint)(cVar14 == '\0') << 10` — SET when collapse (state.byte0==0), CLEAR otherwise. Read by `DestroyBridge_{High,Low}_OnHutDeath @ 0x5742E4 / 0x574F00` (`TEST [reg+0x140], 0x400`) and `UpdateAdjacentBridges_High @ 0x576770` to walk destroyed-run boundaries. No rendering reader; NOT a rail/guard post. See [CELL_FLAGS_0x400_SEMANTIC_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/CELL_FLAGS_0x400_SEMANTIC_GHIDRA_REPORT.md). | SetBridgeDirection |
| 11 | 0x0800 | Bridge orientation (1=N-S, 0=E-W) — verified via `SetBridgeDirection_NESW @ 0x47E040`: `SETZ DL; SHL EDX,0xb` on direction param; direction=0 (NS) → bit SET, direction=6 (EW) → bit CLEAR. See [BRIDGE_ANCHOR_OVERLAY_18_19_AXIS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/BRIDGE_ANCHOR_OVERLAY_18_19_AXIS_GHIDRA_REPORT.md). | SetBridgeDirection |
| 16 | 0x10000 | Tall tile neighbor marker | RecalcAttributes |
| 17 | 0x20000 | Tile animation placed | RecalcAttributes |
| 13 | 0x2000 | Bridge pavement bit | ToggleBridgePavement (0x0056e990) |
| 18 | 0x40000 | Altered passability (XOR-toggled for bridge pathfinding) | PathfinderClass (0x0042acf0) |
| 20 | 0x100000 | Bridge NS zone marker | MapClass::PopulateZones |
| 21 | 0x200000 | Bridge EW zone marker | MapClass::PopulateZones |

**Bit 0x80 vs 0x100 distinction (verified):**
- **0x80** = "has any bridge overlay" — used by `GetEffectiveHeight` (0x487d50) to add +4
  to height_level. Present on all bridge body cells.
- **0x100** = "bridge structural cell" — the primary flag checked by Drive locomotion,
  Can_Enter_Cell, and A* pathfinding for traversal decisions.

Both can be set simultaneously on walkable bridge deck cells.

## Height Arithmetic

### Bridge Height = Ground + 4 (CONSTANT)

Verified from assembly at 3 separate locations:
- Process_Drive_Track ramp detection (0x004b1812): `SUB EAX, 4`
- A* pathfinding start/goal height (0x00429b77): `ADD ECX, 4`
- CheckBridgeTraversal (0x004d9c60): `*(char*)(cell+0x11b) + 4`

```
Bridge deck height = ground_height_level + 4
```

### Two Height Thresholds

**Threshold 1: `abs(diff) < 2` (pathfinding / Can_Enter_Cell)**

Used to determine if a unit is at ground level or bridge level.
Assembly: `CMP EAX, 1` at 0x0073f0dc (with `JLE 0x0073f0e8`) and at 0x00429e75
(with `JG 0x00429e7f`).

```
if abs(path_height - cell.height_level) <= 1:
    treat as ground level (pass UNDER bridge)
else:
    treat as bridge level (ON bridge)
```

**Threshold 2: `abs(diff) < 3` (scatter / movement)**

Used during Process_Drive_Track to decide if obstacles on bridge cells
should trigger scatter. Assembly: `CMP EAX, 2` at 0x004b1f1e (with
`JG 0x004b1f28`).

```
if abs(techno.Z / HeightStep - cell.height_level) <= 2:
    passing UNDER bridge → ignore bridge obstacles
else:
    ON bridge → scatter obstacles
```

### GetGroundHeight Returns Ground Only

`CellClass::GetGroundHeight` (0x00578080 → 0x0047b3a0) computes the **ground-level
Z only**. It does NOT add BridgeZOffset. The **caller** is responsible:

```c
bridge_offset = -(uint)((cell_flags & 0x100) != 0) & g_BridgeZOffset;
Z = GetGroundHeight(coords) + bridge_offset;
```

The `-(uint)(bool)` trick produces 0xFFFFFFFF when true, 0x00000000 when false,
so `& BridgeZOffset` yields either the full offset or zero.

## Dual Occupancy System

Bridges maintain **two parallel object tracking systems** per cell.

### Occupant Linked Lists

`CellClass::AddContent` (0x0047e8a0) and `CellClass::RemoveContent` (0x0047ea90):

```c
void AddContent(CellClass* cell, ObjectClass* object, char on_bridge) {
    if (on_bridge == 0)
        prepend object to cell+0xE4 (ground list)
    else
        prepend object to cell+0xE8 (bridge list)
}
```

The `on_bridge` parameter is `(char)object[0x23]` = byte at `FootClass+0x8C`.

### Occupancy Bit Fields

`ObjectClass::Mark_Occupation` (0x7441B0):

```c
void Mark_Occupation(ObjectClass* obj) {
    CellClass* cell = GetCellAt(obj);
    int ground_z = GetGroundHeight(obj);
    if (ground_z + threshold <= obj->Z && (cell->flags & 0x100)) {
        cell->bridge_occupy |= 0x20;    // cell+0x128
    } else {
        cell->ground_occupy |= 0x20;    // cell+0x124
    }
}
```

## Bridge Ramp Detection (Drive Locomotion)

In `DriveLocomotionClass::Process_Drive_Track` (0x004b0f20), during cell transitions:

```c
src_cell = MapClass::Get_CellClass(prev_coords);
dst_cell = MapClass::Get_CellClass(next_coords);

// Step 1: ENTRY — height drops by exactly 4 AND dst is structural → on_bridge=1
// (Both reads of cell+0x11B are MOVSX, i.e. SIGNED — Rust must use i8 here.)
if (dst_cell.height_level == src_cell.height_level - 4 && (dst_cell.flags & 0x100)) {
    techno.on_bridge = 1;
}

// Step 2: EXIT — UNCONDITIONAL on (!dst.flag & src.flag), independent of height-diff
// Fires even when height matched but dst lacks 0x100 — edge case at bridge boundary cells.
if (!(dst_cell.flags & 0x100) && (src_cell.flags & 0x100)) {
    techno.on_bridge = 0;
}
```

Assembly: `MOVSX EAX, [ESI+0x11B]` (src) and `MOVSX ECX, [EBX+0x11B]` (dst) at
0x004b1807 / 0x004b180e; `SUB EAX, 0x4` at **0x004b1815** confirms the constant is
exactly 4. The entry-set write is at 0x004b1830 (`MOV [techno+0x8C], 0x1`); the
exit-clear write is at 0x004b184a (`MOV [techno+0x8C], 0x0`). The exit branch
falls through from BOTH the height-mismatch path AND the entry-set path (when
the entry write completes, control reaches the same `TEST [dst+0x140], 0x100`
at 0x004b1837 — but since dst's flag IS set in that case, `JNZ 0x004b1851`
skips the exit). Thus only `!dst.flag & src.flag` (regardless of height-diff)
actually triggers the exit-clear.

### Set_Destination Bridge Z Adjustment

`DriveLocomotionClass::Set_Destination` (0x004afd40) checks the **DESTINATION CELL's**
bridge flag, NOT the unit's current on_bridge state:

```c
if (dest != NullCoord) {
    cell = GetCellAt(dest);
    if (cell.flags & 0x100)
        dest.Z += g_BridgeZOffset;     // unconditional add
}
```

This assumes the unit WILL be on the bridge at the destination. Edge cases
(units passing underneath) are handled by the `< 3` threshold at runtime.

## A* Pathfinding — Dual-Layer Bridge Support

The A* system at 0x00429a90 has **full bridge awareness** with dual closed lists.

### Start/Goal Height Initialization

```c
if (unit_type == INFANTRY || !(dest_cell.flags & 0x100))
    goal_height = dest_cell.height_level;           // ground
else
    goal_height = dest_cell.height_level + 4;       // bridge deck

if (is_crusher && (dest_cell.flags & 0x100) &&
    abs(unit.Z / PathfindHeightStep - start_height) > 2)
    start_height += 4;                              // correct for bridge
```

### Dual Closed Lists

Two separate visited/cost arrays indexed by cell:
- `param_1+0x18` / `param_1+0x24`: ground-level closed list + costs
- `param_1+0x1C` / `param_1+0x20`: bridge-level closed list + costs

Which list a cell enters depends on its height_level vs the current path height:

```c
if (cell.height_level < current_path_height)
    use bridge_closed_list[cell_index]
else
    use ground_closed_list[cell_index]
```

A cell can be visited at BOTH ground and bridge level if the path height differs.

### Neighbor Evaluation

```c
if (!(neighbor.flags & 0x100) || abs(path_height - neighbor.height_level) < 2)
    at_ground_level = true;     // check ground closed list
else
    at_ground_level = false;    // check bridge closed list
```

### Pathfinding HeightStep Global

`DAT_0089c2d8` — a **separate HeightStep** from `g_DriveHeightStep` (0x008a07d0).
Used specifically in A* pathfinding. Likely same runtime value but maintained
separately by the engine.

## Can_Enter_Cell Bridge Logic

`UnitClass::Can_Enter_Cell` (0x0073F0A0, 465 lines) opens with bridge detection:

```c
if ((cell.flags & 0x100) == 0 ||
    (height_param != -1 && abs(height_param - cell.height_level) < 2))
    is_bridge_cell = false;     // ground level, pass under
else
    is_bridge_cell = true;      // bridge level, walk on top
```

Later, bridge ramp entry is detected:

```c
if (height_param != -1 && (cell.flags & 0x100) &&
    height_param == cell.height_level + 4)
    // Remap to bridge occupancy (cell+0x128 instead of cell+0x124)
```

## on_bridge Flag (FootClass+0x8C)

### Who Writes It

| Address | Function | Value | Condition |
|---------|----------|-------|-----------|
| 0x004b1830 | DriveLocomotionClass::Process_Drive_Track | 1 | dst_height == src_height - 4 AND dst has 0x100 |
| 0x004b184a | DriveLocomotionClass::Process_Drive_Track | 0 | src has 0x100, dst doesn't qualify |
| 0x005f418f | Object placement | 1 | Placed on bridge cell |
| 0x005f5940 | Object::Unlimbo | 1 | Cell has 0x100 flag |
| 0x006a1bd7 | ShipLocomotionClass::Process_Drive_Track | varies | Ship bridge transition |
| 0x0054d99c | Map loading | varies | Initial placement |

### Who Reads It

- `CellClass::AddContent` / `RemoveContent` — selects ground vs bridge list
- `ObjectClass::Mark_Occupation` / `Clear_Occupation` — selects ground vs bridge bits
- `DriveLocomotionClass::Process_Drive_Track` — collision layer selection
- `DriveLocomotionClass::Process_Movement` — bridge transition flag
- `Can_Enter_Cell` — occupancy layer selection
- All rendering functions that need Z adjustment

## Bridge Overlay Types

### Tile Set Globals (set during theater loading at 0x00545150)

| Global | INI Key | Purpose |
|--------|---------|---------|
| 0x00aa0e28 | BridgeSet | Base tile index for high/concrete bridges (16 tiles) |
| 0x00abad1c | WoodBridgeSet | Base tile index for low/wooden bridges (16 tiles) |

### Bridge Piece INI Keys

| Global | INI Key |
|--------|---------|
| 0x00abc2b4 | BridgeTopLeft1 |
| 0x00aa1130 | BridgeTopLeft2 |
| 0x00abc1e8 | BridgeBottomRight1 |
| 0x00aa0e38 | BridgeBottomRight2 |
| 0x00aa1548 | BridgeTopRight1 |
| 0x00aa0740 | BridgeTopRight2 |
| 0x00abc1d0 | BridgeBottomLeft1 |
| 0x00aa1540 | BridgeBottomLeft2 |
| 0x00abad30 | BridgeMiddle1 (4 variants) |
| 0x00aa1028 | BridgeMiddle2 (4 variants) |

### Overlay ID Ranges

| Range | Type |
|-------|------|
| 0x4A-0x52 (74-82) | Low bridge EW intact |
| 0x53-0x5B (83-91) | Low bridge NS intact |
| 0x5C-0x5F (92-95) | Low bridge EW damaged ends |
| 0x60-0x63 (96-99) | Low bridge NS damaged ends |
| 0x64 (100) | Low bridge EW destroyed |
| 0x65 (101) | Low bridge NS destroyed |
| 0xCD-0xD5 (205-213) | High bridge EW intact |
| 0xD6-0xDE (214-222) | High bridge NS intact |
| 0xDF-0xE2 (223-226) | High bridge EW damaged ends |
| 0xE3-0xE6 (227-230) | High bridge NS damaged ends |
| 0xE7 (231) | High bridge EW destroyed |
| 0xE8 (232) | High bridge NS destroyed |

### Bridge Type Checkers

```c
// CellClass::IsBridge (0x00486750)
return (DAT_00aa0e28 != -1) &&
       (this->overlay >= DAT_00aa0e28) &&
       (this->overlay < DAT_00aa0e28 + 16);

// CellClass::IsWoodBridge (0x00486770)
return (DAT_00abad1c != -1) &&
       (this->overlay >= DAT_00abad1c) &&
       (this->overlay < DAT_00abad1c + 16);
```

## Bridge Damage State Machine

Cell+0x11E stores an 18-state damage tracker (two-axis system):

| States | Axis | Description |
|--------|------|-------------|
| 0-5 | N-S | Progressive N-S direction damage |
| 6 | N-S | Both N-S sides damaged |
| 7-8 | N-S | N-S ramp collapse |
| 9-14 | E-W | Progressive E-W direction damage |
| 15 | E-W | Both E-W sides damaged |
| 16-17 | E-W | E-W ramp collapse |

On destruction, the state machine:
1. Walks bridge cells using 8-direction adjacency bitmask (NW=0x40, N=0x80, NE=0x01, W=0x20, E=0x02, SW=0x10, S=0x08, SE=0x04)
2. Selects destroyed tile from a 42-entry lookup table
3. Adjusts height_level by +4 on collapsed ramp cells
4. Spawns debris animations
5. Destroys/scatters units on the bridge deck

## Bridge Destruction/Repair Functions

| Address | Name | Purpose |
|---------|------|---------|
| 0x00486750 | CellClass::IsBridge | Check if overlay is in BridgeSet range |
| 0x00486770 | CellClass::IsWoodBridge | Check if overlay is in WoodBridgeSet range |
| 0x0047d2b0 | CellClass::RecalcAttributes | Master cell calculator (sets height, flags) |
| 0x0047dd70 | CellClass::BlowUpBridge | Destroy units on bridge, spawn debris |
| 0x004d9c60 | CheckBridgeTraversal | Pathfinding bridge height validation |
| 0x00570050 | ProcessBridgeDestruction_Low | Entry for low bridge destruction |
| 0x00571490 | ProcessBridgeDamageStateMachine_Low | 18-state machine for low bridges |
| 0x00573540 | ProcessBridgeDestruction_High | Entry for high bridge destruction |
| 0x00574000 | DestroyBridge_High | Map init bridge destroy |
| 0x00574c20 | DestroyBridge_Low | Map init bridge destroy |
| 0x00575ee0 | RepairBridgeSegment | Walk bridge, repair 3-wide cells |
| 0x00576ba0 | ProcessBridgeDamageStateMachine_High | 18-state machine for high bridges |
| 0x00578d80 | IsOnBridgeRamp | Check if cell is in one of 6 ramp regions |
| 0x00578e60 | MarkBridgesForRepair_Low | Analyze low bridge connectivity |
| 0x0057a0c0 | MarkBridgesForRepair_High | Analyze high bridge connectivity |
| 0x0057b440 | ApplyBridgeTile | Final tile placement for bridge pieces |
| 0x0057baa0 | DestroyBridge_Low (tile) | Destroy individual low bridge tiles |
| 0x0057ccf0 | DestroyBridge_High (tile) | Destroy individual high bridge tiles |
| 0x0057f200 | RepairBridge_Low | Entry for low bridge repair |
| 0x0057f440 | RepairBridge_High | Entry for high bridge repair |
| 0x00587180 | ApplyDamageToCell | Dispatch to correct bridge damage handler |

## Global Address Reference

### Runtime Globals (computed during map/theater load)

| Address | Type | Name | Purpose |
|---------|------|------|---------|
| 0x008a07c4 | int | g_BridgeZOffset_Drive | `round(DriveHeightStep * 4)` (at 0x4af4c0) |
| 0x008a07d0 | int | g_DriveHeightStep | `ftol(atan2(tileA-tileB) * scale * 0.5)` (at 0x4af42b) |
| 0x0089c2d8 | int | g_PathfindHeightStep | Same formula, DIFFERENT inputs (at 0x42968b) |
| 0x00b0782c | int | g_BridgeZOffset_Ship | Z offset for bridge surfaces (ship) |
| 0x00b07838 | int | g_ShipHeightStep | Z leptons per height level (ship) |
| 0x008b3cac | int | g_FlyBridgeHeight | fly loco bridge Z = ftol(height * 0.5) |
| 0x00aa0e28 | int | g_BridgeSet | Base tile index for high bridges (-1 if none) |
| 0x00abad1c | int | g_WoodBridgeSet | Base tile index for low bridges (-1 if none) |

### Per-Unit-Type Bridge Fields (from rules.ini)

| Offset | INI Key | Purpose |
|--------|---------|---------|
| TechnoTypeClass+0xDCC | ZFudgeBridge | Additional per-type Z fudge on bridges |

## Summary: Bridge Movement Pipeline

```
1. Set_Destination checks DESTINATION CELL for bridge flag
   → adds BridgeZOffset to dest.Z if cell has 0x100

2. Process_Movement calls Can_Enter_Cell
   → Can_Enter_Cell uses abs(height - cell.height_level) < 2
     to decide: ground level (pass under) vs bridge level (walk on)
   → selects ground occupancy (cell+0x124) or bridge occupancy (cell+0x128)

3. A* pathfinder maintains dual closed lists (ground + bridge)
   → bridge deck height = cell.height_level + 4
   → separate visited arrays per layer
   → neighbor evaluation uses < 2 threshold

4. Process_Drive_Track detects bridge ramp during cell transitions
   → dst.height_level == src.height_level - 4 AND dst has 0x100
     → on_bridge = 1 (entering bridge)
   → src has 0x100 but dst doesn't qualify
     → on_bridge = 0 (leaving bridge)

5. Cell occupancy uses on_bridge flag
   → AddContent routes to cell+0xE4 (ground) or cell+0xE8 (bridge)
   → Mark_Occupation sets bit 0x20 in cell+0x124 or cell+0x128

6. Scatter check uses abs(Z/HeightStep - height_level) < 3
   → within 2 levels = UNDER bridge → ignore bridge obstacles
   → 3+ levels = ON bridge → scatter obstacles

7. GetGroundHeight returns ground-only Z
   → caller adds BridgeZOffset when cell has 0x100 flag

8. Pathfinding bridge passability toggle
   → PathfinderClass (0x0042acf0) XOR-toggles bit 0x40000 in 5x5 grid
   → propagates along up to 24 waypoints in the path queue
   → enables temporary bridge-aware cost adjustments
```

## GetEffectiveHeight (0x00487d50) — Verified

Uses bit 0x80 (NOT 0x100) to determine bridge height:

```c
int CellClass::GetEffectiveHeight() {
    return (int)(char)(this+0x11B) + ((this+0x140 >> 7) & 1) * 4;
}
```

Assembly: `SHR EAX, 7` / `AND EAX, 1` / `LEA EAX, [ECX + EAX*4]`

This means bit 0x80 ("has bridge overlay") contributes to effective height,
while bit 0x100 ("bridge structural") is used for movement decisions. Both
can be set on walkable deck cells.

## CheckBridgeTraversal (0x004d9c60) — Height Diff Details

Verified from decompilation:

- **Height diff 0**: Passable (same level)
- **Height diff 1**: Checks `cell+0x11C` (SlopeIndex, NOT flag 0x200). If the
  uphill/downhill cell has zero SlopeIndex → blocked (return 7). Non-zero
  SlopeIndex means the cell has a terrain ramp → passable.
- **Height diff 4**: Bridge-to-ground transition. Validates using both 0x100
  (bridge structural) and 0x200 (bridgehead) flags.
- **Height diff 2, 3, 5+**: Always blocked (return 7).

**CORRECTION from gidra reports:** Report 039 claimed "ramp flag (0x200)" is
used for height-diff-1 checks. This is WRONG — it checks `cell+0x11C`
(SlopeIndex byte), not flag 0x200 in cell_flags. Flag 0x200 is only used
in the height-diff-4 bridge validation.

## MapClass Bridge Connection List

MapClass+0x54 stores a list of bridge connection records (16 bytes each):

```
struct BridgeRecord {           // 16 bytes — verified 2026-04-06 against ComputeBridgeZones, Validate/InvalidateBridgeZones
    CellStruct endpoint_a;      // +0x00: packed {x:i16, y:i16}
    CellStruct endpoint_b;      // +0x04: packed {x:i16, y:i16}
    u8 is_intact;               // +0x08: 1=intact, 0=destroyed (toggled by Validate/InvalidateBridgeZones)
    u8 _unused[3];              // +0x09: VERIFIED UNUSED — no reader across all 6 BridgeRecord-accessing functions (ComputeBridgeZones, FindBridgeRecord, GetZoneID, ValidateBridgeZones, InvalidateBridgeZones, UpdateBridgeZonesHelper)
    i32 bridge_kind;            // +0x0C: 0=high bridge, 1=low bridge (FindBridgeRecord skips kind!=0)
};
```

**Direction is NOT stored.** It is computed geometrically: `endpoint_a.X == endpoint_b.X` → vertical bridge,
otherwise horizontal. See FindBridgeRecord (0x56DA10) for the runtime check.

- MapClass+0x54 = pointer to array
- MapClass+0x58 = capacity
- MapClass+0x5D = owns_memory flag
- MapClass+0x60 = count
- MapClass+0x64 = grow_increment

`MapClass__FindBridgeRecord` (0x0056da10) searches this list for a record
containing a given cell, checking distance thresholds per axis.

## Passability Toggle System (0x0042acf0)

`PathfinderClass__UpdateBridgePassability` XOR-toggles bit 0x40000 in
cell+0x140 to temporarily alter pathfinding costs near bridges.

**Algorithm:**
1. Walk the unit's path queue (up to 24 entries at FootClass+0x5E0)
2. For each waypoint cell, copy the 0x40000 bit from a reference cell
   using `(old ^ (~src ^ old)) & 0x40000`
3. After path propagation, toggle 0x40000 in a **5x5 grid** (-2 to +2
   in both X and Y) around the reference cell
4. Only cells with `cell+0x124 != 0` (has ground occupancy) are toggled
5. The center cell is also toggled at the end

This creates a temporary "bridge-aware zone" that increases pathfinding
costs (x4 via ComputeMoveCost at 0x00429830) for cells near the bridge,
steering units away from congested bridge approaches.

### 0x40000 Cost Multiplier (verified)

The pathfinding cost constant at **0x007e37bc** = float **4.0**.
When 0x40000 is set: `cost = cost * 4.0`.

Surrounding cost constants in the same table:
- 0x7e37b0: 1.0 (base cost)
- 0x7e37b4: 2.0 (bridge adjacency)
- 0x7e37b8: 10.0 (diagonal bridge)
- 0x7e37bc: **4.0** (0x40000 flag multiplier)

## Verified Cell Field Summary

Every field confirmed from assembly with exact instruction addresses:

| Offset | Size | Type | Field | Verified At |
|--------|------|------|-------|-------------|
| +0x24 | 4 | packed | MapCoord {X:i16, Y:i16} | SetBridgeDirection |
| +0x2C | 4 | ptr | **Bridge anchor CellClass*** | SetBridgeDirection (write), ApplyDamage (read) |
| +0x38 | 4 | int | IsoTileTypeIndex | IsBridge (0x48675b) |
| +0x3C | 4 | ptr | AttachedTag (TagClass*) — NOT bridge | Ghidra struct |
| +0x44 | 4 | int | OverlayTypeIndex (-1 = none) | ApplyDamageToCell (0x5871db) |
| +0x116 | 2 | i16 | tube_index (-1 = none) | GetTubeAtCell (0x484f20) |
| +0x11A | 1 | u8 | iso_sub_tile_idx = row\*width+col within placed IsoTileType; NOT orientation (corrected 2026-07-12: was "bridge_sub_type (bit 0 = orientation)" — `decompile_function 0x0057b440` shows write source `piVar1[0xb9]*iStack_18+iStack_10` (width\*row+col), matching this doc's own +0x11A row above, not a bridge-specific orientation byte — STRUCT_FAMILY_CASCADE) | ApplyBridgeTile (0x57b440) |
| +0x11B | 1 | i8 | height_level | IsBridge, Process_Drive_Track |
| +0x11C | 1 | u8 | slope_type (NOT bridge flag) | RecalcAttributes (0x47d35e) |
| +0x11E | 1 | u8 | bridge_damage_state (0-17) | ProcessBridgeDamageState (0x571ff7) |
| +0xE4 | 4 | ptr | FirstObject (ground list head) | AddContent (0x47e8a0) |
| +0xE8 | 4 | ptr | AltObject (bridge list head) | AddContent (0x47e8a0) |
| +0xEC | 4 | int | land_type enum | SpeedType table lookup |
| +0x124 | 4 | u32 | OccupationFlags (bit 0x20 = occupied) | Mark_Occupation (0x7441fb) |
| +0x128 | 4 | u32 | AltOccupationFlags (bridge, bit 0x20) | Mark_Occupation (0x7441f1) |
| +0x140 | 4 | u32 | cell_flags bitfield | Everywhere |

## Verified Global Computations

### g_BridgeZOffset_Drive (0x008a07c4)

```asm
004af4a6: LEA ECX, [EAX*4 + 0x0]     ; ECX = DriveHeightStep * 4
004af4b5: FILD dword ptr [ESP]         ; push as float
004af4bb: CALL ftol                    ; round to int
004af4c0: MOV [0x008a07c4], EAX        ; store
```
**Formula: `g_BridgeZOffset = round(g_DriveHeightStep * 4)`**
Confirms bridge = 4 height steps in world coordinates.

### g_DriveHeightStep (0x008a07d0) vs g_PathfindHeightStep (0x0089c2d8)

Both use the same formula `ftol(atan2(A - B) * scale * 0.5)` but with
**different input globals**:
- Drive: inputs at 0x8a0758/0x8a0780/0x8a0778
- Pathfind: inputs at 0x89a2f8/0x89c288/0x89c280

They likely produce the same runtime value but are maintained separately.

## Corrections to gidra/ Reports

The following claims from the decompilation reports were **verified as incorrect**
via live Ghidra analysis:

| Report | Claim | Truth |
|--------|-------|-------|
| 024 (line 70) | FUN_00486380 = "IsBridgeCell" | **WRONG**: It's `IsClearTile` — returns true when IsoTileTypeIndex is 0xFFFF or 0 (default tile). Has 48 callers, not 33. |
| 025 (line 13) | FUN_004865b0 = "IsOverlayBridge" checking overlay range | **WRONG**: Checks IsoTileTypeIndex (not overlay), and DAT_00abad28 = ShorePieces base (not bridge overlay). It's `IsShorePieceTile`. |
| 025 (line 135) | Cell+0x11C = "bridge flag" | **WRONG**: Cell+0x11C is SlopeIndex set by TMP_ReadSlopeType. Not a bridge flag. Used alongside bridge checks but is slope data. |
| 039 (line 118) | Height diff 1 uses "ramp flag 0x200" | **WRONG**: Uses SlopeIndex at +0x11C (non-zero = has ramp). Flag 0x200 is only used in height-diff-4 bridge validation. |
| 066 (line 236) | Cell+0x3C = "bridge set index" | **WRONG**: Cell+0x3C is AttachedTag (TagClass*), a trigger system pointer. Bridge anchor is at +0x2C. |
| Prior docs | Rules+0xFA8 = BridgeStrength | **WRONG**: Rules+0xFA8 is C4Warhead (warhead ptr). BridgeStrength is at **Rules+0x1740**. Verified from INI parse at 0x66cd88: `MOV [ESI+0x1740], EAX`. |

These corrections are important — if implementation followed the incorrect
report claims, it would produce wrong bridge behavior.

## Bridge Damage & Destruction (verified)

### BlowUpBridge (0x0047dd70)

Called when a bridge segment is destroyed. Two-phase operation:

```
Phase 1 — Destroy units:
  Walk ground occupant list (cell+0xE4):
    Call ReceiveDamage(&unit->Health, 0, Rules->C4Warhead@+0xFA8, 0, 1, 1, 0)
    — damage source is unit's OWN health field (object+0x6C, dereferenced via
      piVar2[0x1b]), warhead is C4Warhead. Effect = guaranteed kill because
      damage == full HP. BridgeStrength is NOT used here (it gates the random
      threshold in Apply_area_damage only).
  Walk bridge occupant list (cell+0xE8):
    Call Destroy() (vtable+0xEC) — unconditional kill

Phase 2 — Spawn debris:
  if Rules->BridgeVoxelMax > 0:
    50% chance: spawn random MetallicDebris anim (DBRIS1LG..DBRS10SM)
    Always: spawn random BridgeExplosions anim (TWLT026/036/050/070)
    Both at Z offset = 0x600 (1536 leptons) above cell
    BridgeExplosions get random 1-5 frame start delay
```

### Rules.ini Bridge Fields

| Rules Offset | INI Key | Default |
|-------------|---------|---------|
| +0x13C | MetallicDebris (list ptr) (corrected 2026-07-12: was +0x140 — xref from "MetallicDebris" string @0x83cef0 in `RulesClass::ReadGeneral` resolves to `LEA EBX,[ESI+0x13c]` at 0x66db4b, not +0x140 — OFFSET_RETYPED_WRONG) | DBRIS1LG...DBRS10SM (20 entries) |
| +0x14C | MetallicDebris count | 20 |
| +0x158 | BridgeExplosions (list ptr) (corrected 2026-07-12: was +0x15C — xref from "BridgeExplosions" string @0x83cedc in `RulesClass::ReadGeneral` resolves to `LEA EBX,[ESI+0x158]` at 0x66dc52, not +0x15C — OFFSET_RETYPED_WRONG) | TWLT026, TWLT036, TWLT050, TWLT070 |
| +0x624 | BridgeVoxelMax (corrected 2026-07-12: was +0x168 — xref from "BridgeVoxelMax" string @0x83c954 in `RulesClass::ReadGeneral` resolves to `MOV [ESI+0x624],EAX` at 0x66f0bf. The old +0x168 value coincides with BridgeExplosions's own sub-field offset (0x158+0x10), evidence the two were conflated — OFFSET_RETYPED_WRONG / STRUCT_FAMILY_CASCADE) | 3 |
| +0xFA8 | C4Warhead (warhead ptr for bridge damage) | — |
| +0x1740 | BridgeStrength | 1500 |

### ApplyDamageToCell Routing (0x00587180)

The combat system checks `WarheadType.Wall=yes` (+0x144) before calling this.
**CORRECTION:** Prior docs said `Wood=yes` — WRONG. `Wood=yes` (+0x147) controls
wooden overlay destruction, NOT bridge tile damage. `Wall=yes` gates bridge damage.
`BridgeDestruction` is a multiplayer dialog setting, not a warhead property.
The function itself routes to the correct handler:

```
1. Check overlay ID range:
   0x4A-0x63 → DestroyBridge_Low (overlay damage)
   0xCD-0xE6 → DestroyBridge_High (overlay damage)

2. Check IsoTileTypeIndex vs BridgeSet/WoodBridgeSet:
   In BridgeSet range → ProcessBridgeDamageState_High (tile damage)
   In WoodBridgeSet range → ProcessBridgeDamageState_Low (tile damage)

3. For structural cells (flag 0x100):
   Follow cell link to bridge head
   Check head's overlay ID to determine bridge type
```

### 18-State Damage Machine (cell+0x11E)

Two independent axes (N-S and E-W):

```
States 0-5:  N-S progressive damage → jumps to state 6
State 6:     Both N-S ramps damaged → begins collapse
States 7-8:  Individual N-S ramp collapse
States 9-14: E-W progressive damage → jumps to state 15
State 15:    Both E-W ramps damaged → begins collapse
States 16-17: Individual E-W ramp collapse
```

On final collapse:
1. Calls SetBridgeDirection_NESW with health_state=0 (destroyed)
2. SetBridgeDirection updates 4-5 cells with new flags
3. BlowUpBridge called on each cell
4. Bridge damage state reset to 0
5. Overlay cleared to -1
6. Neighbors notified, bridge zones recomputed

### 8-Direction Adjacency Bitmask (verified from 0x00579b70)

Used by bridge connectivity analysis to determine which neighboring
cells are part of the bridge structure:

| Direction | Bit | Hex | Condition |
|-----------|-----|-----|-----------|
| NE | 0 | 0x01 | neighbor.height_level == this.height_level + 4 |
| E | 1 | 0x02 | AND neighbor passes passability check |
| SE | 2 | 0x04 | |
| S | 3 | 0x08 | |
| SW | 4 | 0x10 | |
| W | 5 | 0x20 | |
| NW | 6 | 0x40 | |
| N | 7 | 0x80 | |

### Bridge Direction Table (0x0082a944, 16 entries)

Maps tile offset within BridgeSet to direction:

```
Index:  0   1   2   3   4   5   6   7   8   9  10  11  12  13  14  15
Value:  0   0  -1   2   2  -1   0   0   0   0   0   2   2   2   2   2
```

- Value 0 = NE direction
- Value 2 = SE direction
- Value -1 = corner/transition piece (no single direction)

### SetBridgeDirection (0x0047e040 / 0x0047e470)

**NESW (0x47e040) and NWSE (0x47e470) are INSTRUCTION-IDENTICAL / COMPILED-TWIN**
(refined 2026-05-12 from prior "byte-identical" claim). Same opcodes, same
operands, same constants (0x89f688 / 0x89f690 / mask values 0xfffee07f /
0xfffee8ff / 0xfffee7ff / 0xfffefffff), same CALL/JMP **targets** (e.g., both
call 0x5657A0 = `MapClass__Get_CellClass`, 0x47DD70 = `BlowUpBridge`,
0x6551C0 = `RadarClass__MarkTerrainDirty`). They differ ONLY in the **relative
offset bytes** of CALL/JMP instructions (e.g., `E8 48 75 0E 00` vs
`E8 18 71 0E 00` for the same absolute target from different positions).
Function sizes match exactly (0x422 bytes incl. terminator). Spot-checks at
prologue, three mid sections, and epilogue confirmed: byte-identical except
where a window contains a CALL with a non-zero relative offset to its target.
The naming distinction reflects only the CALLER's convention — `OverlayClass::Mark`
at 0x5FC570 picks NESW when bridge overlay type is 0x18/0x19, NWSE for 0xED/0xEE
(high-bridge variants). The flag-setting logic is the same. Rust port: implement
as one function.

Sets bridge flags on **4-5 cells** per bridge segment (writing param_3 as
the state value, with `uVar9 = param_3 & 1` driving most bit placements):

```
1. Anchor cell (param_1): mask 0xfffee07f, then OR-in:
     0x100 (bit 8, uVar9<<8), 0x200 (bit 9, uVar9<<9),
     0x1000 (bit 12, uVar9<<12), 0x10000 (bit 16, uVar9<<16),
     0x400 if param_3==0 (destroyed), 0x800 if param_2!=0 (orientation),
     and *** 0x80 (bit 7) IF AND ONLY IF param_3 & 1 *** (SHL EAX,7 at 0x47e0e7).
2. Forward neighbor 1 (one step in dir param_2): mask 0xfffee8ff & 0xfffff7ff,
   then OR-in 0x100/0x200/0x1000/0x10000 + orientation. Bit 7 (0x80) PRESERVED
   untouched — never written here.
3. Forward neighbor 2 (two steps in dir param_2): same mask, OR-in
   0x100/0x1000/0x10000 + orientation (no 0x200). Bit 7 (0x80) PRESERVED.
4. Forward neighbor 3 (three steps): mask 0xffffefff, OR-in only 0x1000. Bit 7
   (0x80) PRESERVED.
5. Opposite neighbor (dir (param_2-4)&7): mask 0xfffff8ff & 0xfffee7ff, OR-in
   0x100/0x200/0x10000 + orientation + 0x400 if destroyed. Bit 7 (0x80)
   PRESERVED.
6. (direction 6 only): one extra cell offset by DAT_0089f690 gets only
   bit 0x10000 set.
```

**Only the anchor cell ever has bit 0x80 written by this function**, and only
when `param_3 & 1`. The map-load construction loop at the bottom of FUN_00565C10
invokes SetBridgeDirection ONLY on cells **already carrying bit 0x80** (with
param_3=1), so the bit's true origin is upstream of SetBridgeDirection (set
somewhere during map initialization — the specific load path has not been
traced; candidates include the .MAP file parser or the per-bit XOR flag-copy
block inside FUN_00565C10 itself, but the immediate source of those copied
flag values was not pinned down). What IS verified: bit 0x80 is set before
SetBridgeDirection's construction-time call runs, so SetBridgeDirection's
bit-0x80 write is functionally a re-assertion on the anchor.

If health_state=0 (destroyed): calls BlowUpBridge on each cell.
If health_state=1 (intact): sets bridge_damage_state (+0x11e) to 0 or 9.

## Bridge Rendering (verified from Ghidra, 2026-03-21)

### Rendering Paths

Bridge cells are rendered through THREE separate paths in Phase 1 (terrain pass):

**1. Bridge TMP tiles** — `iso_to_screen` → `CellOverlay_TileDraw` → `TMP_TileBlitter`
- Renders the isometric terrain tile for each bridge cell
- Uses the standard per-pixel Z-test (the ONLY active Z R+W path in the engine)
- heightLevel already has +4 baked in for bridge deck cells
- Y-position: `screenY + heightLevel * -15` (elevated by 60px for bridge deck)
- Z-adjust: uses `cellZAdjust` at CellClass+0x10C (derived from heightLevel)

**2. Bridge SHP overlays** — `FUN_004d1890` (case 0x14) → `CellClass__DrawOverlay_Body` / `CellClass__DrawOverlay_Shadow`
- Draws bridge deck surface graphics (SHP overlay images)
- Body and shadow drawn separately with different Z parameters

**3. Bridge railings/pavement** — `Tactical_layer_overlays` → `FUN_006d7c00` → `FUN_004802a0` → `FUN_00547230`
- Draws railing/pavement overlay decorations via `CC_Draw_Shape` with flag `0x4601`
- Y-adjust: `heightLevel * -15 + 0x3A` (58px offset)

### SHP Overlay Draw Calls (CellClass__DrawOverlay_Body at 0x0047f6a0)

**Body (flag 0x4E00):**
- Z-buffered, centered, palette, depth-write
- effective_height = `height_level + ((cell_flags >> 7) & 1) * 4` (bridge adds +4)
- Y-adjust = `effective_height * -15 - 2`
- Z-height = `cellZAdjust_bottom` from CellClass+0x10E (pre-computed for heightLevel+4)

**Shadow (flag 0x4601) — CellClass__DrawOverlay_Shadow at 0x0047f510:**
- Z-buffered, centered, palette, 50%-darken blitter (bit 0x01)
- Shadow frame = `num_frames / 2 + damage_frame` (second half of SHP)
- Y-adjust = `heightLevel * -15 - 2` (WITHOUT bridge +4, draws at ground level)
- Z-height = 1000 (default, no special depth)

### CellClass Z-Adjust Fields for Bridges

`Cell_ComputeZAdjust` (`0x00484680`) pre-computes three Z values per cell:

| Field | Offset | Computed From | Used By |
|-------|--------|--------------|---------|
| +0x10A | cellZAdjust_top | `gradient * heightLevel - offset` | Buildings, normal overlays |
| +0x10C | cellZAdjust | (+0x10A) * intensityFactor >> 16 | TMP_TileBlitter, normal overlays |
| +0x10E | cellZAdjust_bottom | `gradient * (heightLevel + 4) - offset`, scaled | **Bridge overlay body only** |

The +4 in the +0x10E formula is hardcoded in `Cell_ComputeZAdjust`. Every cell
pre-computes a bridge-level Z regardless of whether it has a bridge. Only bridge
overlay body rendering (flag 0x80 check in `DrawOverlay_Body`) reads +0x10E.

### Bridge Pixel Offsets

| Condition | X Adjust | Y Adjust |
|-----------|----------|----------|
| Bridge overlay (flag 0x80) | 0 | -16 |
| E-W damage states (9-17) body | 0 | -15 additional |
| E-W damage states (9-17) shadow | -15 | +7 |
| Height per level | 0 | -15 per level |
| Tile center | +30 | 0 |

### Z-Buffer: Units Under vs On Bridge

- Units **on** bridge: Z computed at `(ground_height + 4) * -15` -> sorts in FRONT of bridge surface
- Units **under** bridge: Z at `ground_height * -15` -> sorts BEHIND bridge overlay
- Bridge shadow drawn at ground Z (=1000) -> darkens ground under bridge, not units on top
- No special bridge rendering pass: bridges go through the same Phase 1 terrain pipeline

### g_BridgeZ_Offset (DAT_00B0782C)

Value: 0 at static time (runtime-initialized). Used ONLY by `ShipLocomotionClass`
for ships navigating under bridges. Adjusts ship destination Z-coordinate when
cell has flag 0x100 (bridge structural). NOT used by rendering pipeline.

### Radar Minimap Colors

- Bridge structural cells (flag 0x100): use overlay surface image pixel color
- Low bridge overlays (0x4A-0x63, 0xCD-0xE6): force frame=1 for consistent deck color
- Fallback for black pixels: logs warning, uses default overlay color

## Bridge Repair

> **⚠ CORRECTION 2026-05-12 (Phase 1 of bridge-repair RE pass).**
> Three load-bearing claims in the section below are WRONG.
> See **[BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/05-damage-collapse-repair-cabhut/BRIDGE_REPAIR_AND_HUT_DEATH_GHIDRA_REPORT.md)** for the verified
> chain. Summary of corrections:
>
> 1. **`field_0x6DF` is NOT a "repair pending flag"** set on engineer enter.
>    It is the **C4-plant-pending flag**, set by `InfantryClass::PerCellProcess`
>    (`0x519630`) in the Mission_Sabotage branch when an engineer plants C4.
>    Cleared by `BuildingClass::Update` when the C4 timer expires. The
>    bridge-repair path never touches it.
> 2. **`FUN_00574000` / `FUN_00574C20` are NOT the repair dispatchers.**
>    They are `DestroyBridge_High_MapInit` / `DestroyBridge_Low_MapInit`
>    (the `_MapInit` Ghidra suffix is misleading — they run at runtime),
>    called from `BombClass::Detonate` (demo truck) and `BuildingClass::Update`
>    (C4 timer expired on CABHUT). The **engineer-repair** dispatchers are
>    `ProcessBridgeDestruction_Low` (`0x570050`) and `ProcessBridgeDestruction_High`
>    (`0x573540`) — again misnamed; despite the "Destruction" label, these
>    are the repair-side entries called from `InfantryClass::PerCellProcess`.
> 3. **`RepairBridgeSegment` (`0x575EE0`) does NOT clear objects.** It fires
>    `TechnoClass::ProcessCellAction(0x1F, …)` on cells whose `field_0x3C`
>    (TagClass pointer) is non-null — a trigger-action fanout. It is called
>    only from destruction-side endpoint walkers
>    (`MapClass::FindBridgeEndpoints_*`, `MapClass::UpdateBridgeEdgeTiles_*`),
>    not from any repair path. The "Repair" in its name is a Ghidra labelling
>    error.
>
> The content below is left in place for archival/diff value but should not
> be treated as authoritative. The new report supersedes it.

### Trigger

Engineer enters Bridge Repair Hut (GAREFN/CAREFN building) → calls
RepairBridge_Low (0x57f200) or RepairBridge_High (0x57f440).

### RepairBridgeSegment (0x00575ee0)

Walks from endpoint_a to endpoint_b, clearing objects from a **3-wide path**:

```
for each cell along bridge axis:
    scatter/destroy objects on center cell
    scatter/destroy objects on perpendicular neighbor 1
    scatter/destroy objects on perpendicular neighbor 2
    advance to next cell along bridge axis
```

Does NOT place tiles — the callers (RepairBridge_Low/High) handle tile
placement and flag restoration after the path is cleared.

## Bridge Repair Hut Interaction (verified)

### INI Flag

`BridgeRepairHut=yes` in BuildingTypeClass. Stored at **BuildingTypeClass+0x16B6** (byte).

### Trigger Flow

```
BuildingClass::Update (0x0043fb20)
  → if field_0x6DF set (flagged for repair) AND Type+0x16B6 (BridgeRepairHut):
      → scan 5x5 area (-2 to +2) around building for bridge tiles
      → if low bridge overlay (0x4A-0x65): call FUN_00574c20 (repair low)
      → if high bridge tile (BridgeSet range): call FUN_00574000 (repair high)
      → clear repair flag
```

The Engineer entering a BridgeRepairHut sets `field_0x6DF` on the building,
which triggers the scan on the next update tick.

## Warhead Bridge Destruction (verified, CORRECTED)

### INI Key: `Wall=yes` (+0x144) gates bridge tile damage

**CORRECTION:** Prior version of this document said `Wood=yes`. That was WRONG.

- **`Wall=yes`** (+0x144) = gates bridge TILE damage in Apply_area_damage
- **`Wood=yes`** (+0x147) = gates wooden OVERLAY destruction (buildings, NOT bridges)
- `BridgeDestruction` is a **multiplayer dialog setting**, not a warhead property

### Bridge Damage Probability Check (from Apply_area_damage at 0x4894b0)

```
if (Wall=yes):
    if (warhead == Rules->IonCannonWarhead):   // Rules+0xFF0
        ApplyDamageToCell()                     // ALWAYS damages bridge
    else:
        if (Random(1, BridgeStrength) < effective_damage):
            ApplyDamageToCell()                 // probabilistic
```

Higher BridgeStrength (default 1500) = bridges more resistant.
IonCannonWarhead bypasses the random check entirely.

### WarheadTypeClass Flag Map (verified from INI parsing)

| Offset | INI Key | Type |
|--------|---------|------|
| +0x144 | Wall | bool |
| +0x145 | WallAbsoluteDestroyer | bool |
| +0x146 | PenetratesBunker | bool |
| **+0x147** | **Wood** | **bool** |
| +0x148 | Tiberium | bool |
| +0x14B | Sonic | bool |
| +0x14C | Fire | bool |
| +0x14D | Conventional | bool |
| +0x14E | Rocker | bool |
| +0x14F | DirectRocker | bool |
| +0x150 | Bright | bool |
| +0x154 | EMEffect | bool |
| +0x155 | MindControl | bool |
| +0x156 | Poison | bool |
| +0x157 | IvanBomb | bool |
| +0x158 | ElectricAssault | bool |
| +0x159 | Parasite | bool |
| +0x15A | Temporal | bool |
| +0x15B | IsLocomotor | bool |

### Damage Application (0x00489280)

```
Destroy overlay IF:
    WallAbsoluteDestroyer=yes
    OR Wall=yes
    OR (Wood=yes AND overlay.ArmorType == 6)   // armor type 6 = wood
```

The ArmorType is at OverlayTypeClass+0x9C. Only overlays with wood armor
are destroyed by `Wood=yes` warheads.

## Overlay Frame Layout (verified)

The bridge_damage_state at cell+0x11E is used directly as the SHP frame index.

### Intact Frames (with visual variance)

For frames 0 (NW-SE) and 9 (NE-SW), a **Latin square variance table** at
0x0081cc30 adds 0-3:

```
variance_table[16] = {0,1,2,3, 3,2,1,0, 2,3,0,1, 1,0,3,2}
index = ((cell_y & 3) << 2) | (cell_x & 3)
frame = damage_state + variance_table[index]
```

The 4x4 pattern ensures each variant (0-3) appears exactly once per row
and column — a Latin square for uniform visual distribution.

### Frame Map

| State | Frame | Axis | Visual |
|-------|-------|------|--------|
| 0 | 0-3 (variance) | NW-SE | Intact body |
| 4 | 4 | NW-SE | One ramp damaged |
| 5 | 5 | NW-SE | Other ramp damaged |
| 6 | 6 | NW-SE | Both ramps damaged |
| 7 | 7 | NW-SE | Ramp collapse side A |
| 8 | 8 | NW-SE | Ramp collapse side B |
| 9 | 9-12 (variance) | NE-SW | Intact body |
| 13 | 13 | NE-SW | One ramp damaged |
| 14 | 14 | NE-SW | Other ramp damaged |
| 15 | 15 | NE-SW | Both ramps damaged |
| 16 | 16 | NE-SW | Ramp collapse side A |
| 17 | 17 | NE-SW | Ramp collapse side B |

After full collapse, overlay changes to destroyed ID (0x64/0x65 low, 0xE7/0xE8 high).

## CellClass+0x11A Sub-Type Encoding (corrected 2026-07-12)

The byte at +0x11A is the **universal IsoTileType sub-tile (icon) index** — the same
field already documented in the CellClass Bridge Fields table at the top of this doc
(not bridge-specific). Set during `ApplyBridgeTile` (0x57b440) as `row * width + col`
(`puVar10[0x11a] = (char)iVar7` where `iVar7 = piVar1[0xb9]*row + col` —
verified via `decompile_function 0x0057b440`).

**CORRECTION:** This section previously claimed "bit 0 encodes orientation:
`(sub_type & 1)==0` -> NW-SE, `==1` -> NE-SW." That is WRONG/MISLEADING — +0x11A is
not an orientation-encoding byte (see this doc's own +0x11A summary row). What IS
verified: bridge collapse walkers (e.g. `MapClass__UpdateRamp_NS_CollapseA_Low` @
0x56ef50, `if ((puVar4[0x11a] & 1) == 0) { ... } else { ... }`) branch on
`(cell+0x11A) & 1` to choose between two sets of perpendicular neighbor cells for
`BlowUpBridge` — a real, verified behavior, but it is a sub-tile-parity branch, not
a documented "orientation" flag. Actual bridge axis orientation (N-S vs E-W) lives
in cell_flags bit 0x800 (see Cell Flags table above), a completely separate field.
(verified via `decompile_function 0x0056ef50` — STRUCT_FAMILY_CASCADE)

## CellClass+0x2C Bridge Anchor Pointer (verified)

**NOT cell+0x3C** as report 066 claimed. The field at +0x2C is a pointer
back to the bridge head/anchor cell. Set by `SetBridgeDirection_NESW`:

- Bridge intact: `neighbor->field_0x2C = anchor_cell_ptr`
- Bridge destroyed: `neighbor->field_0x2C = 0`

Used to trace bridge ownership from any structural cell to the anchor.

## Overlay Damage Progression (verified from all 8 walker functions)

### Two-Step Destruction

Bridge body overlays have a **two-step** destruction sequence: intact → damaged → destroyed.
Ramp overlays have a **one-step** sequence: intact → destroyed (no intermediate).

### Low Bridge NS Body (LOBRDG01-06)

```
INTACT (0x4D-0x4F) → DAMAGED (0x50) → DESTROYED (0x64)
```
- LOBRDG01 (0x4D), LOBRDG02 (0x4E), LOBRDG03 (0x4F) = intact variants (left/both/right endcap)
- LOBRDG04 (0x50) = damaged (all intact variants collapse to this single damaged state)
- LOBRDG24 (0x64) = destroyed rubble

### Low Bridge EW Body (LOBRDG07-15)

```
INTACT (0x53-0x58) → DAMAGED (0x59) → DESTROYED (0x65)
```
- LOBRDG07-12 (0x53-0x58) = intact variants
- LOBRDG13 (0x59) = damaged
- LOBRDG25 (0x65) = destroyed rubble

### Low Bridge Ramps (one-step, no intermediate)

| Intact | ID | Destroyed | ID | Direction |
|--------|-----|-----------|-----|-----------|
| LOBRDG16 | 0x5C | LOBRDG17 | 0x5D | NS ramp west |
| LOBRDG18 | 0x5E | LOBRDG19 | 0x5F | NS ramp east |
| LOBRDG20 | 0x60 | LOBRDG21 | 0x61 | EW ramp south |
| LOBRDG22 | 0x62 | LOBRDG23 | 0x63 | EW ramp north |

### High Bridge NS Body (LOBRDB01-05)

```
INTACT (0xD1-0xD2) → DAMAGED (0xD3) → DESTROYED (0xE7)
```

### High Bridge EW Body (LOBRDB06-14)

```
INTACT (0xD6-0xDB) → DAMAGED (0xDC) → DESTROYED (0xE8)
```

### High Bridge Ramps (one-step)

| Intact | ID | Destroyed | ID | Direction |
|--------|-----|-----------|-----|-----------|
| LOBRDB15 | 0xDF | LOBRDB16 | 0xE0 | NS ramp west |
| LOBRDB17 | 0xE1 | LOBRDB18 | 0xE2 | NS ramp east |
| LOBRDB19 | 0xE3 | LOBRDB20 | 0xE4 | EW ramp south |
| LOBRDB21 | 0xE5 | LOBRDB22 | 0xE6 | EW ramp north |

### Already-Destroyed Cells

Hitting an already-destroyed bridge cell (0x64, 0x65, 0xE7, 0xE8) does **nothing**.
The walker functions bail out immediately when overlay > the damaged range.

### Three Cells Per Column/Row

When any bridge body cell is damaged, ALL THREE cells in the perpendicular axis
(center + two neighbors) are set to the SAME overlay. Then damage propagates
laterally to neighboring columns via ApplyBridgeDestruction.

## Bridge Zone System Details (verified)

### Bridge Record Structure (16 bytes at MapClass+0x54)

| Offset | Size | Field | Purpose |
|--------|------|-------|---------|
| +0x00 | 4 | endpoint_a | Packed cell coord (X:i16, Y:i16) |
| +0x04 | 4 | endpoint_b | Packed cell coord (X:i16, Y:i16) |
| +0x08 | 1 | is_intact | 0=destroyed, 1=intact (toggled by Validate/InvalidateBridgeZones) |
| +0x09 | 3 | _init_residue | Stack-init artifact, not read by any verified function |
| +0x0C | 4 | bridge_kind | 0=high bridge (searchable), 1=low bridge (FindBridgeRecord skips) |

### FindBridgeRecord (0x56da10)

Linear scan through records. For vertical bridges (same X), checks if query Y
is between endpoints and `abs(query.X - bridge.X) <= threshold`. For horizontal,
checks X range and Y distance. Returns index or -1.

### Zone Graph Layers

3 parallel zone graphs at MapClass+0x8C, +0xA4, +0xBC (24 bytes each).
Each is a DynamicVectorClass of adjacency lists. Edges are 8-byte (zone_id, weight) pairs.

Zone disconnection (0x584e50) removes 6 edges per layer (3 pairs: direct, perpendicular, far-side).
Zone connection (0x5851b0) adds the same 6 edges per layer.

### MapClass Zone Fields

| Offset | Type | Purpose |
|--------|------|---------|
| +0x54 | ptr | Bridge record data |
| +0x60 | int | Bridge record count |
| +0x70 | ptr | Per-cell zone ID array (5 shorts per cell) |
| +0x8C | 24B | Zone graph layer 0 |
| +0xA4 | 24B | Zone graph layer 1 |
| +0xBC | 24B | Zone graph layer 2 |

## Rendering Constants (all verified from assembly)

| Constant | Value | Instruction | Address |
|----------|-------|-------------|---------|
| Height per level | 15 px | `LEA [EAX+EAX*2]; LEA [EAX+EAX*4]` | 0x480160 |
| Bridge Y offset | -16 px | `SUB ECX, 0x10` | 0x480137 |
| E-W damage X offset | -15 px | `SUB EDX, 0x0F` | 0x47f59a |
| E-W damage Y offset | +7 px | `ADD EAX, 0x07` | 0x47f59d |
| Shadow frame | num_frames/2 + frame | `SAR EAX, 1; ADD EAX, EDX` | 0x47f61d |
| Body draw flags | 0x4E00 | `PUSH 0x4E00` | 0x47f7f5 |
| Shadow draw flags | 0x4601 | `PUSH 0x4601` | 0x47f60b |
| Tile center X | +30 px | `ADD EDX, 0x1E` | 0x48015d |
| Z-depth | eff_height * -15 - 2 | `MOV EDX,-2; SUB EDX,EDI` | 0x47f7eb |
| GetEffectiveHeight | height + bridge_bit * 4 | `LEA [ECX+EAX*4]` | 0x487d63 |

## Zone System Bridge Integration (verified)

### No Separate Bridge Zone

The engine does **not** maintain separate bridge zones. Instead:

1. **Bridge intact:** `GetZoneID` (0x0056d230) checks cell flag 0x100, looks up
   the BridgeRecord, and returns the zone of the bridge endpoint cell.
   `ValidateBridgeZones` adds zone graph edges between endpoint zones,
   making them appear connected to the pathfinder.

2. **Bridge destroyed:** `InvalidateBridgeZones` removes zone graph edges.
   `GetZoneID` walks perpendicular to find a ground endpoint and returns
   its zone. The A* pre-check correctly rejects cross-bridge paths.

3. **Three zone layers:** The zone system maintains 3 parallel zone maps
   (one per speed category). Bridge connections are added/removed in all 3.

## Naval Units and Bridges (verified)

- Ships use **identical bridge code** as Drive (same Process_Drive_Track)
- Ships **DO** use the `on_bridge` flag at FootClass+0x8C
- Most ships have `TooBigToFitUnderBridge=true` → cannot enter bridge cells
- The height scatter threshold `< 3` is the same as Drive
- Ship BridgeZOffset is at separate global `0x00b0782c`

## Aircraft and Bridges (verified)

Aircraft **do NOT** check bridge passability — they fly over everything.
The only bridge interaction is **altitude adjustment**:

```
if on_bridge == 0 AND g_FlyBridgeHeight <= distance_to_dest:
    if cell has bridge flag (0x100):
        distance_to_dest -= g_FlyBridgeHeight   // 0x008b3cac = height * 0.5
```

This prevents aircraft from clipping through bridges during landing approaches.

## Low Bridge Water Passability (partially verified; corrected 2026-05-16)

Low bridges are implemented through the **tunnel/tube system**, NOT a simple
LandType override:

1. Verified predicate: `CellClass::IsLowBridgeCell @ 0x00484AB0` requires a valid
   `cell+0x116` tube index and `cell+0xEC == 10` (`LandType == Tunnel`).
2. Verified construction: `CellClass::RecalcAttributes` creates a same-cell
   TubeClass shell only after the cell already has `LandType == 10`, a qualifying
   tunnel/low-bridge tile range, and no valid tube index.
3. Verified explicit tube path: the `[Tubes]` parser creates full entry/exit/step
   TubeClass records and writes the parsed tube index to the entry cell.
4. Not yet verified here: the exact placement path that changes Water to Tunnel.
   The earlier wording "when placed, water cells change from Water to Tunnel" is
   an implementation inference until that tile mutation path is audited directly.
5. Tunnel LandType has passable speed values for ground units.
6. Ships cannot traverse tunnel cells if the Ship speed row blocks Tunnel.
7. Stale/unverified: "When destroyed, LandType reverts to Water and tube_index is
   cleared." A 2026-05-16 direct write audit found constructor/parser/save-
   compaction/removal/copy writers for `CellClass+0x116`, but did not find a low
   damage/repair helper directly clearing the tube index. Tube save/compaction
   can clear invalid low-bridge tubes, but that is not the same as live
   destruction/repair clearing them.

### Key Cell Fields for Low Bridges

| Offset | Type | Field | Values |
|--------|------|-------|--------|
| +0x116 | i16 | tube_index | -1 = no tube, 0+ = tube ID |
| +0xEC | i32 | land_type | 2 = Water, 10 = Tunnel |

## TubeClass Structure (verified)

Used for low bridge tunnel paths. RTTI at 0x00844a78.

**Size:** 0x1C4 bytes (452 bytes)

| Offset | Size | Type | Field |
|--------|------|------|-------|
| +0x00 | 4 | ptr | vtable (0x7f59b0) |
| +0x24 | 2 | i16 | start_x (cell coord) |
| +0x26 | 2 | i16 | start_y |
| +0x28 | 2 | i16 | end_x |
| +0x2A | 2 | i16 | end_y |
| +0x2C | 4 | int | entry_direction (0-7) |
| +0x30 | 400 | int[100] | waypoints (direction 0-7 per step, -1 = end) |
| +0x1C0 | 4 | int | num_waypoints |

Parsed from `[Tubes]` INI section:
```
Format: startX, startY, direction, endX, endY, dir0, dir1, ..., -1
```

RecalcAttributes creates TubeClass objects when detecting tunnel tile types
(Tunnels, TrackTunnels, DirtTunnels, DirtTrackTunnels at LandType=10).

Correction 2026-05-16: constructor-created/RecalcAttributes tubes start as
same-cell shells (`start == end`, `num_waypoints == 0`). `[Tubes]` parsing is the
verified path that overwrites entry/exit/step fields into a full traversal tube.
`ComputeBridgeZones` consumes `end_x/end_y`; it does not fill them.

## Ramp Update Functions (verified, all 16 renamed)

The 18-state damage machine calls these to modify individual ramp tiles.
Each walks to a neighbor cell, updates bridge_damage_state and overlay.

### Low Bridge Ramp Functions

| Address | Name | Called For | State Set | Direction |
|---------|------|-----------|-----------|-----------|
| 0x56ed40 | UpdateRamp_NS_DamageA_Low | States 0-5 | 4 (or 6 if B done) | East (2) |
| 0x56ee40 | UpdateRamp_NS_DamageB_Low | States 0-5 | 5 (or 6 if A done) | West (6) |
| 0x56ef50 | UpdateRamp_NS_CollapseA_Low | State 6,8 | 7 | East (2), recursive |
| 0x56f2f0 | UpdateRamp_NS_CollapseB_Low | State 6,7 | 8 | West (6), recursive |
| 0x56f690 | UpdateRamp_EW_DamageA_Low | States 9-14 | 14 (or 15) | South (4) |
| 0x56f7a0 | UpdateRamp_EW_DamageB_Low | States 9-14 | 13 (or 15) | North (0) |
| 0x56f8b0 | UpdateRamp_EW_CollapseA_Low | State 15,16 | 17 | South (4), recursive |
| 0x56fc80 | UpdateRamp_EW_CollapseB_Low | State 15,17 | 16 | North (0), recursive |

### High Bridge Ramp Functions (identical logic, different overlay base)

| Address | Name |
|---------|------|
| 0x572230 | UpdateRamp_NS_DamageA_High |
| 0x572330 | UpdateRamp_NS_DamageB_High |
| 0x572440 | UpdateRamp_NS_CollapseA_High |
| 0x5727e0 | UpdateRamp_NS_CollapseB_High |
| 0x572b80 | UpdateRamp_EW_DamageA_High |
| 0x572c90 | UpdateRamp_EW_DamageB_High |
| 0x572da0 | UpdateRamp_EW_CollapseA_High |
| 0x573170 | UpdateRamp_EW_CollapseB_High |

### Collapse Behavior

On collapse (tile == BridgeMiddle+3), the function:
1. Recurses to walk the full ramp
2. Checks cell+0x11A bit 0 (sub-tile parity — NOT an orientation flag; see the
   corrected "CellClass+0x11A Sub-Type Encoding" section below) to select which
   perpendicular neighbor set to target (corrected 2026-07-12, verified via
   `decompile_function 0x0056ef50` — STRUCT_FAMILY_CASCADE)
3. Calls BlowUpBridge on 3 adjacent cells (perpendicular to bridge axis)
4. Sets overlay height to `cell.height_level - 4` (drops ramp to ground)
5. Calls SetBridgeDirection with health_state=0 (destroyed)

### State Machine Dispatch Table (verified)

| State(s) | Action | Functions | Directions |
|----------|--------|-----------|------------|
| 0-5 | N-S damage | DamageA + DamageB | East(2), West(6) |
| 6 | N-S both collapse | CollapseA + CollapseB | East(2), West(6) |
| 7 | N-S collapse A only | CollapseA | East(2) |
| 8 | N-S collapse B only | CollapseB | West(6) |
| 9-14 | E-W damage | EW_DamageA + EW_DamageB | South(4), North(0) |
| 15 | E-W both collapse | EW_CollapseA + EW_CollapseB | South(4), North(0) |
| 16 | E-W collapse B only | EW_CollapseB | North(0) |
| 17 | E-W collapse A only | EW_CollapseA | South(4) |

## RecalcAttributes Bridge Correction (verified)

**RecalcAttributes (0x0047d2b0) does NOT set bridge flags 0x80/0x100/0x200/0x400/0x800.**

Exhaustive search of all 708 instructions found only two OR operations on +0x140:
- `OR [cell+0x140], 0x20000` — tile animation marker (on this cell)
- `OR [neighbor+0x140], 0x10000` — tall tile neighbor (on adjacent cells)

Bridge structural flags are set exclusively by `SetBridgeDirection_NESW` (0x47e040)
and `SetBridgeDirection_NWSE` (0x47e470).

### Hidden `level_override` parameter (added 2026-05-12)

**RecalcAttributes has a hidden SECOND parameter** (RET 0x4 confirms 1 stack
arg cleanup). Signature: `void __thiscall(CellClass *this, int level_override)`.
`level_override` is the byte source written to `[ESI+0x11B]` at 0x47D94E. The
write is gated by `level_override != -1`; most callers pass -1 ("don't override
Level"). Specific callers (likely PlaceBuilding for foundation-level
enforcement) pass a concrete byte value to force the cell's Level. This
parameter was missed in earlier audits — neither the 2026-05-11 AUDIT_LOG
entry nor the prior version of this section documented it.

### Per-byte write inventory for `CellClass+0x11A..+0x11E` and adjacent (added 2026-05-12)

Verified by full disassembly read of 0x47D2B0–0x47DD63. Every store enumerated:

| Site | Instruction | Field | Condition |
|------|-------------|-------|-----------|
| 0x47D318 | `MOV [ESI+0xEC], ECX` | LandType | Overlay branch — from overlay type field_0x298 |
| 0x47D35E | `MOV [ESI+0x11C], AL` | **SlopeIndex** | Overlay branch — from `TMP_ReadSlopeType(this->Height)` |
| 0x47D378 | `MOV [ESI+0x44], -1` | OverlayTypeIndex | Overlay-clear when slope-removable |
| 0x47D37F | `MOV [ESI+0x11E], 0` | bridge_state byte | Same path |
| 0x47D53E | `MOV [ESI+0xEC], 3` | LandType=3 | g_RulesClass+0x664==2 cliff-back path |
| 0x47D58D | `MOV [ESI+0x38], EBX` (=0xFFFF) | IsoTileTypeIndex | No-tile fallback init |
| 0x47D5E6 | `MOV [ESI+0x38], EBX` (=0xFFFF) | IsoTileTypeIndex | Tile-invalid cliff fallback |
| **0x47D5E9** | **`MOV [ESI+0x11A], AL`** | **iso_sub_tile_idx = 0** (corrected 2026-07-12: was "Height = 0" — Ghidra's applied CellClass struct labels offset +0x11A as "Height", but that label is stale: `TMP_ReadSlopeType` and `MapClass::ApplyBridgeTile` (0x57b440) both use this byte as the sub-tile icon index (`row*width+col`), matching this doc's own top Bridge Fields table entry — RTTI_LABEL_DRIFT) | **Cliff fallback (AL post-XOR 0). The ONLY write to +0x11A in this function.** |
| 0x47D5EF | `MOV [ESI+0xEC], 0` | LandType=0 | Cliff fallback |
| 0x47D5F9 | `MOV [ESI+0x11C], AL` | SlopeIndex=0 | Cliff fallback |
| 0x47D7C1 | `MOV [ESI+0xEC], 3` | LandType=3 | g_RulesClass+0x664==2 path #2 |
| 0x47D80D | `MOV [ESI+0x11C], AL` | SlopeIndex | Normal branch — TMP_ReadSlopeType result |
| 0x47D843 | `MOV [ESI+0xEC], EAX` | LandType | Tile-overlay path |
| 0x47D849 | `MOV [ESI+0x44], -1` | OverlayTypeIndex | Slope-removable overlay clear |
| 0x47D850 | `MOV [ESI+0x11E], 0` | bridge_state | Same |
| 0x47D86E | `MOV [ESI+0xEC], 5` | LandType=5 | Overlay LandType=0 path |
| 0x47D8AA | `MOV [ESI+0xEC], EAX` | LandType | FUN_00544BE0 result |
| **0x47D94E** | **`MOV [ESI+0x11B], AL`** | **Level = level_override** | **ONLY write to +0x11B. AL from [ESP+0x4C] = hidden 2nd param. Gated by `level_override != -1`.** |
| 0x47D993 | `MOV [ESI+0x11D], DL` | HeightInPixels | `(height_raw - 30) / 15` via signed magic multiply |
| 0x47DA88 | `OR [ESI+0x140], 0x20000` | Flags bit 17 (tube anim) | LandType==10 + tile match |
| 0x47DB40 | `MOV [ESI+0xEC], EDX` | LandType | Overlay LandType direct |
| 0x47DB48 | `MOV [ESI+0xEC], 0` | LandType=0 | Empty-tile fallback |
| 0x47DB52 | `MOV [ESI+0x11C], 0` | SlopeIndex=0 | Empty-tile fallback |
| 0x47DD2A | `MOV [ESI+0xEC], 3` | LandType=3 | g_RulesClass+0x664==2 path #3 |

**Summary of bridge-relevant byte writes:**
- `+0x11A` (iso_sub_tile_idx — corrected 2026-07-12, was "Height / tube sub-type": Ghidra's struct label "Height" at this offset is stale; verified as the universal sub-tile icon index via `MapClass::ApplyBridgeTile` 0x57b440 — RTTI_LABEL_DRIFT) — written ONLY at 0x47D5E9 (cliff fallback, sets to 0). Otherwise preserved from prior state.
- `+0x11B` (Level, signed i8) — written ONLY at 0x47D94E, gated by `level_override != -1`.
- `+0x11C` (SlopeIndex) — written at 4 sites: 0x47D35E (overlay branch), 0x47D5F9 (cliff fallback → 0), 0x47D80D (normal branch), 0x47DB52 (empty-tile fallback → 0).

**Runtime mutation reality:** In retail YR, RecalcAttributes runs once per cell
at map load AND once per overlay change. It does NOT fire on bridge collapse
directly — the collapse path mutates flags via SetBridgeDirection without
calling RecalcAttributes. So **`+0x11A`, `+0x11B`, `+0x11C` are STATIC after
map load** for any cell whose overlay does not change at gameplay time.

### CliffBackImpassability (g_RulesClass+0x664) — confirmed NOT TS-legacy

**`g_RulesClass+0x664 = CliffBackImpassability`**, default in rulesmd.ini = 2
(rulesmd.ini line 409, rules.ini line 319). The function runs an asymmetric
6-neighbor check in all three major branches (overlay, cliff fallback, normal
exit). When `=2`, sets `LandType=3` if all 6 neighbors are `>= 4 levels below
this cell`. The 6 neighbors checked (in order):

1. (X, Y-1) — N
2. (X-1, Y) — W
3. (X+2, Y+2) — **peculiar 2-step SE** (verified retail; intent unknowable)
4. (X+1, Y+1) — SE
5. (X-1, Y+1) — SW
6. (X+1, Y-1) — NE

Missing: S=(X,Y+1) and NW=(X-1,Y-1). The same asymmetric pattern is repeated
three times in the function body (probably a C++ macro inline), making "typo"
unlikely. Per parity bar, the Rust port must reproduce this exact pattern.

### Zone-cache mirror writes (NOT CellClass)

| Site | Instruction | Notes |
|------|-------------|-------|
| 0x47D560 | `MOV [EBX+0x1], DL` | DL = this.Level. Writes zone-cache slot at DAT_0087F850-indexed. |
| 0x47D569 | `MOV [ECX+0x8], AL` | AL = this.Level. Writes second zone-cache (DAT_0087F858). |
| 0x47D571 | `MOV [EBX], DL` | DL = this.field_0x4C |
| 0x47D7DD | `MOV [EAX+0x1], CL` | Normal-branch exit |
| 0x47D7EA | `MOV [ECX+0x8], DL` | Same pattern |
| 0x47DD45 | `MOV [EAX+0x1], DL` | Same |
| 0x47DD51 | `MOV [EAX+0x8], DL` | Same |

These mirror `Level` into two parallel zone arrays (probably
`ZoneMap__CellToZoneIndex` lookup targets for fast bulk queries). They are
NOT writes to CellClass; addresses come from `ZoneMap__CellToZoneIndex(X,Y)`
at function entry.

### Other behaviors (summary)

RecalcAttributes ALSO:
- Sets land_type (+0xEC) from tile terrain type or overlay (multiple sites)
- Creates TubeClass objects for tunnel tiles (LandType=10) at 0x47D8EC
- Sets Flags bit 0x20000 (tube-anim spawned) at 0x47DA88, sticky once set
- Calls `CellClass__ApplyLAT_and_SlopeFixup` (0x47CA80), `CellClass__RecalcZoneType`
  (0x483C80), `CellClass__OverlayToTiberiumIndex` (0x5FDD20)
- Skipped in `g_IsMapEditor != 0` for some paths (verify per-call-site)

## Pathfinding Cost System (verified)

### ComputeMoveCost (0x00429830)

**Base cost by SpeedType (table at 0x0081870c):**

| Index | SpeedType | Initial Cost |
|-------|-----------|-------------|
| 0 | Foot | 1.0 |
| 1 | Track | 1000.0 (overwritten by bridge walk loop) |
| 2 | Wheel | 1.0 |
| 3 | Float | 1.0 |

**Track vehicles (SpeedType 1) bridge walk loop:**

For tracked vehicles only, the function walks up to 10 cells along the bridge,
following occupant chains to detect congestion. After the walk:
- Normal bridge: cost forced to **4.0**
- `this+0x3C == 2` (destroyed bridge): cost = **1000.0** (impassable)

**0x40000 flag multiplier:** `cost *= 4.0` (constant at 0x7e37bc).

**Diagonal bridge costs (when on_bridge AND field_01 set):**

| Condition | Multiplier | Constant Address |
|-----------|-----------|-----------------|
| Both adjacent cells are bridge | 1.0 | 0x7e2ac8 |
| One adjacent cell is bridge | 2.0 | 0x7e37b4 |
| Neither adjacent is bridge | 10.0 | 0x7e37b8 |

This discourages units from moving diagonally off bridge edges.

## Bridge Height Tables (verified from memory)

### Start Heights (0x0082a734, 16 entries)

Maps tile offset to expected ground height for bridge start pieces:
```
[7, 7, -1, 7, 7, -1, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2]
```

### Walk Directions (0x0082a774, 16 entries)

Direction index for walking along bridge from start piece:
```
[2, 2, -1, 4, 4, -1, 2, 2, 2, 2, 2, 4, 4, 4, 4, 4]
```
(2=SE, 4=SW, -1=invalid)

### End Heights (0x0082a7b4, 16 entries)

Expected height for bridge end pieces:
```
[-1, -1, 4, -1, -1, 2, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2]
```

### Height Class Table (0x0082a7f4, 42 entries)

Maps sub-tile index to height compatibility class (0-25):
```
[0,0,0,1,2,3,4,4,5,5,5,6,7,8,9,9,10,10,10,11,12,13,
 14,14,15,15,15,16,17,18,19,19,20,20,21,21,22,22,23,23,24,25]
```
Tiles with same class can coexist; different classes = height conflict.

### Direction Class Table (0x0082a89c, 42 entries)

Maps sub-tile index to directional class (0-7):
```
[4,4,4,4,4,4,3,3,2,2,2,2,2,2,1,1,0,0,0,0,
 0,0,7,7,6,6,6,6,6,6,5,5,3,3,1,1,7,7,5,5,4,4]
```
Used by ApplyBridgeTile: `abs(dir_class[A] - dir_class[B])` must be in [3,5]
for valid ramp placement (opposite-facing pieces).

## CheckBridgeTraversal Complete Logic (0x004d9c60, verified)

```
Height diff 0 (same level):
  If src has BOTH 0x100+0x200 AND dest has 0x100: OK (bridge-to-bridge)
  If tracked_height != src_height: BLOCKED

Height diff 1 (ramp):
  Going down: check dest cell+0x11C (SlopeIndex) != 0
  Going up: check src cell+0x11C != 0
  If zero: BLOCKED

Height diff 4 (bridge entry/exit):
  Going down (leaving bridge): tracked_height must == src_height, dest must have 0x100
  Going up (entering bridge): src must have BOTH 0x100 AND 0x200 (bridgehead)
  Sets entering_bridge output flag = 1

Height diff 2, 3, 5+: ALWAYS BLOCKED
```

**Bridgehead (0x200) is REQUIRED for bridge entry.** Regular bridge deck cells
(0x100 only) cannot be entered from ground level — only bridgehead cells allow
the height-4 transition.

## Can_Enter_Cell Bridge Logic (verified, complete)

### Layer Selection in Can_Enter_Cell (0x0073F0A0)

The function makes a **binary ground/bridge choice** — never checks both layers.

**Step 1 — Initial determination (PRE-vtable, at 0x73F0BD-0x73F0EB):**
```
if cell has NO bridge (0x100 not set): on_bridge = 0
elif path_height != -1 AND abs(path_height - cell.height_level) < 2: on_bridge = 0  (under bridge)
else: on_bridge = 1  (on bridge deck)
Store at [ESP+0x13]  // local byte var — this DECIDES the object list iterated
```

**Step 1b — PRE-vtable: ground occupancy snapshot (at 0x73F0ED-0x73F109):**
```
local[0x14] = cell+0x124 low byte                   // ground occupier bits
local[0x1c] = cell+0x54                              // ground secondary list ptr
local[0x15] = (cell+0x124 dword >> 5) & 1            // vehicle bit from byte 1
```

**Step 2 — Vtable dispatch (at 0x73F2EB):**
```
result = this->vtable[0x1B0](cell, direction, &path_height, &output_unused)
// vtable+0x1B0 = CheckBridgeTraversal (0x4D9C60). MAY update path_height.
if (result == 7) return 7
```

**Step 3 — POST-vtable: CONDITIONAL bridge-layer OVERWRITE (at 0x73F303-0x73F34C):**
```
if path_height != -1 AND (cell.flags & 0x100) AND path_height == cell.Level + 4:
    SWITCH to bridge occupancy:
    local[0x14] = cell+0x128 low byte                // bridge occupier bits
    local[0x1c] = cell+0x58                          // bridge secondary list ptr
    local[0x15] = (cell+0x128 dword >> 5) & 1        // bridge vehicle bit
```

**Step 4 — Occupant loop (at 0x73F4F9-0x73FA8C):**
```
if [ESP+0x13] == 0: occupier_head = cell+0xE4  (FirstObject = ground list)
else:                occupier_head = cell+0xE8  (AltObject   = bridge list)
// Iterates occupier_head, classifying with the (possibly bridge-layer) bits.
```

**Pre-vs-post split — bounded parity divergence (verified 2026-05-12):**

The **object-list selection** ([ESP+0x13]) is decided PRE-vtable from
cell flags + path_height. The vtable can update path_height, but it does
NOT update [ESP+0x13]. The **occupancy bits** (local[0x14], local[0x15])
are decided POST-vtable with a fresh predicate (`path_height == cell.Level+4
AND cell has 0x100`). **These two decisions CAN disagree** in edge cases —
the function may iterate the ground occupier list while using bridge-layer
occupancy bits, or vice versa. This happens when path_height starts at -1,
the vtable doesn't fill it to Level+4 (e.g., dst cell isn't a bridge), but
the cell-flag pre-decision already chose ground/bridge list.

Rust currently pre-decides the layer at A* push-time via
`is_at_bridge_level(current.height, neighbor)` and uses that one decision
for BOTH object list AND occupancy. This matches the binary on most
retail bridge cells but **NOT** at the bridgehead-exit boundary tick. The
divergence is bounded (rarely observable in retail unit configurations)
but non-zero. Tracked as Q5 in
[BRIDGE_DEFERRED_MECHANICS_GHIDRA_REPORT.md](BRIDGE_DEFERRED_MECHANICS_GHIDRA_REPORT.md)
— requires a fidelity test to confirm whether retail unit placements
ever surface the difference.

**No cross-layer collision.** A unit on bridge cannot collide with ground objects.

**Speed table skipped for bridge:** When on_bridge=1, the SpeedType/LandType
passability check is bypassed entirely — bridges are always passable.

### Crush-on-Bridge Edge Case

The main occupant loop correctly uses the bridge layer for crush detection.
However, the **post-loop infantry-crush fallback** at 0x73fcfe calls
`FindInfantry(cell, 0)` with hardcoded `0` = ground list. This means the
fallback path always searches ground, even when the unit is on a bridge.
This appears to be an original engine quirk/bug.

## Fog of War and Bridges (verified)

### No Per-Layer Shroud

The shroud/fog system operates on **2D cells only**. There is no separate
"under bridge" vs "on bridge" visibility. If you have vision on a bridge
cell, both the bridge surface and ground beneath are visible.

- `cell+0x12C & 0x08` = shroud revealed flag (applies to entire cell)
- `cell+0x12C & 0x10` = explored flag
- `cell+0x140 & 0x20` = shroud counter active

### Height-Based LOS Blocking (RevealByHeight)

When `Rules->RevealByHeight` (+0x17EE) is enabled, bridge cells CAN block
line-of-sight for ground-level observers:

```
if target_cell.height_level > observer_height + 3:
    LOS BLOCKED (bridge occludes vision)
```

Since bridge cells have effective height = ground + 4, and threshold is +3:
- Observer at height 0: `0 + 3 = 3 < 4` → bridge BLOCKS vision
- Observer at height 1: `1 + 3 = 4 < 4` → FALSE, bridge does NOT block

### Key Rules Fields

| Offset | INI Key | Purpose |
|--------|---------|---------|
| +0x17E7 | AllyReveal | Allies share vision |
| +0x17EE | RevealByHeight | Enable height-based LOS |

## Bridge Destruction Helpers (verified, renamed)

Four tile-replacement functions called from the DestroyBridgeWalker functions.
Each converts overlay IDs to damaged/destroyed equivalents using a lookup table
and checks the 4-bit neighbor bitmask (intact/destroyed on each side).

| Address | Name |
|---------|------|
| 0x57dd50 | ApplyBridgeDestruction_NS_Low |
| 0x57e2a0 | ApplyBridgeDestruction_EW_Low |
| 0x57e7a0 | ApplyBridgeDestruction_NS_High |
| 0x57ed00 | ApplyBridgeDestruction_EW_High |

### Neighbor Check Functions (4-bit bitmask)

| Address | Name | Checks |
|---------|------|--------|
| 0x57b870 | CheckBridgeNeighbors_EW_Low | X-1/X+1 for NS low |
| 0x57b990 | CheckBridgeNeighbors_NS_Low | Y-1/Y+1 for EW low |
| 0x57cab0 | CheckBridgeNeighbors_EW_High | X-1/X+1 for NS high |
| 0x57cbe0 | CheckBridgeNeighbors_NS_High | Y-1/Y+1 for EW high |

Bitmask: bit 1=right intact, bit 2=right destroyed, bit 4=left intact, bit 8=left destroyed.

## Bridge Pavement System (verified)

### Bit 0x2000 in cell+0x140

The pavement bit is a **binary sub-tile variant selector** for bridge overlays.
It determines which of two tile images is drawn for the bridge surface.

- **Pavement = 0:** Default bridge appearance
- **Pavement = 1:** Alternate appearance (e.g., paved vs bare)

**Independent from the Latin square:** The Latin square at 0x81cc30 provides
4-way frame variance for intact bridges. Pavement is a separate binary choice
applied on top of that.

**Toggled by:** All ramp update functions and bridge destruction/repair functions.
Propagates to 8 neighbors sharing the same overlay type (recursive flood-fill).

## Superweapons and Bridges (verified)

All damage-dealing superweapons delegate to Apply_area_damage, which checks
`Wall=yes` for bridge tile damage. Layer-aware superweapons:

| Superweapon | Bridge-Aware? | Layer Selection |
|-------------|--------------|-----------------|
| Nuclear Missile | Yes (Z-offset) | Apply_area_damage handles both |
| Iron Curtain | Yes | Uses cell+0xE4 or +0xE8 based on bridge flag |
| Chronosphere | Yes | Same layer selection as Iron Curtain |
| Psychic Dominator | Yes | Checks IsOnBridgeSurface, finds ground cell |
| Genetic Mutator | Yes | Same as Psychic Dominator |
| Lightning Storm | No special handling | Apply_area_damage handles bridge tiles |

## Combat Damage Chain (verified)

```
BulletClass::AI (0x466410)
  → bullet detonation (0x468d80)
    → WarheadTypeClass::Detonate (0x4690b0)
      → Apply_area_damage (0x4894b0)
        → For each cell in CellSpread:
            Unit damage: iterates ONE layer (cell+0xE4 or +0xE8)
                Layer selected by: explosion_z > ground_height + bridge_height/2
            Bridge tile damage (independent, checked for ALL cells):
                if Wall=yes AND (isIonCannonWarhead OR Random(1,BridgeStrength) < damage):
                  ApplyDamageToCell()
```

**Key:** Unit AoE damage = single layer. Bridge tile damage = independent check.

## Rules Fields Summary (bridge-related, all verified)

| Offset | INI Key | Default | Purpose |
|--------|---------|---------|---------|
| +0xFA8 | C4Warhead | — | Warhead used for crush damage (10000 to target, 20 to self) |
| +0xFF0 | IonCannonWarhead | — | Bypasses BridgeStrength random check |
| +0x1740 | BridgeStrength | 1500 | Random threshold for bridge damage probability |
| +0x17E7 | AllyReveal | — | Allies share vision |
| +0x17EE | RevealByHeight | — | Height-based LOS blocking |
