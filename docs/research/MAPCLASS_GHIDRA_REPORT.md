# MapClass — Ghidra Research Report

> **⚠ STALE — superseded 2026-04-24.** This is the original report
> (2026-04-06). Two subsequent passes corrected several claims. When
> the text here conflicts with the sources below, **trust the newer
> documents**:
>
> - [`MAPCLASS_COMPLETE_DECODE.md`](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_COMPLETE_DECODE.md) —
>   master summary with the final status matrix.
> - [`MAPCLASS_GHIDRA_REPORT_REVISIT_2026_04_24.md`](MAPCLASS_GHIDRA_REPORT_REVISIT_2026_04_24.md)
>   — vtable size (30, not 64), slot 3 = `IsCellExplored`, dead-byte
>   proof for `+0x74–0x7F` / `+0x11C–0x123`, `+0x115C` DynVec meaning.
>
> **Known errors still present in the body below (left in place for
> history):**
> - Init_Clear address given as `0x565190` (lines 52, 122, 541) — the
>   real Init_Clear is `0x5659F0`, confirmed via leaked debug string.
>   `0x565190` is an unrelated helper.
> - §8 "64-slot vtable" — vtable is 30 slots; addresses past `0x7ED47C`
>   belong to the adjacent `VectorClass` vtable.
> - Slot 3 called "CellHasBuilding-style" — correct name is
>   `IsCellExplored` (returns `(cell.ShroudFlags >> 3) & 1`).
>
> The struct layout, crate slot table, bridge record format, and zone
> speed cache layout sections remain valid.

**Constructor:** `0x00565090`
**Vtable:** `0x007ED404`
**Global Instance:** `0x0087F7E8`
**Init/Alloc:** `0x00565800` (vtable slot 5)
**Destructor/Clear:** `0x005652C0`
**RTTI:** `.?AVMapClass@@` at `0x00816BC8`
**Total Size:** 0x1174 bytes (4468 bytes) — MapClass-specific fields end here; DisplayClass starts at +0x1174
**Map Cell Init:** `0x00565C10` (the core map resize/cell-creation function)
**Cell Iterator Reset:** `0x00578350`
**Confidence:** HIGH (constructor assembly + destructor + 25+ methods decompiled)
**Active in YR:** Yes — core map infrastructure, always active

## 1. Overview

MapClass is the second class in the game's main display hierarchy, inheriting from
GScreenClass and serving as the base for DisplayClass. It owns the **cell grid**,
**zone pathfinding system**, **bridge records**, **shroud reveal tables**, **crate
slot management**, and the **cell iterator** used for map-wide operations. The single
global instance at `0x0087F7E8` is part of a ~21,868-byte mega-object that spans the
full hierarchy from GScreenClass through SidebarClass.

**Inheritance chain:**
```
GScreenClass         (vtable: 0x7EA6FC, size: 0x10)
  └─ MapClass        (vtable: 0x7ED404, adds: +0x10 to +0x1173)
      └─ DisplayClass (vtable: 0x7E6114, starts at: +0x1174)
          └─ RadarClass (vtable: 0x7F0344)
              └─ PowerClass (vtable: 0x7EFF54)
                  └─ SidebarClass (vtable: 0x7F3058)
                      └─ TabClass / ScrollClass / MouseClass
```

## 2. Class Layout / Key Offsets

All offsets are **byte offsets** from the start of the MapClass instance.

### GScreenClass Base (inherited, 0x00-0x0F)

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0x00 | 4 | ptr | vtable | Overridden to `0x7ED404` (MapClass) |
| +0x04 | 4 | int | bitfield/state | Set to 0 in constructor |
| +0x08 | 4 | int | unknown | Set to 0 in constructor |
| +0x0C | 4 | int | blit_mode | Set to 2 in GScreenClass constructor |

### Zone System Core (+0x10 to +0x4F)

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0x10 | 1 | bool | zones_initialized | **UNVERIFIABLE 2026-05-28**: Init_Clear (`0x5659F0`) does NOT write to `+0x10` (decompile_function confirms it touches only +0x158 crate table and +0x148). Neither does the constructor. No binary write to this offset found in any decompiled MapClass function. Field name and purpose are INFERRED, not confirmed. |
| +0x11-0x13 | 3 | — | padding | — |
| +0x14 | 4 | ptr | zone_connection_hash | Heap-allocated hash table (256 buckets of DynVec). NULL initially, allocated in `0x565800`. Freed in destructor `0x5652C0`. Used in `ZoneFloodFillScanLine` (0x56CB90). Each bucket is a DynVec<uint64> with stride 0x18 bytes storing zone adjacency edge pairs. |
| +0x18 | 52 | ptr[13] | zone_ids[MovementZone] | 13 pointers (one per MovementZone 0-12). Each points to a `ushort[]` array indexed by cluster_id, returning zone_id. Freed/zeroed in `UpdateBridgeZonesHelper` (0x56C510). |
| +0x4C | 4 | int | zone_cluster_count | Total zone clusters allocated (inferred — sits between zone_ids end and bridge DynVec) |

### Bridge Records DynamicVectorClass (+0x50 to +0x67)

Embedded `DynamicVectorClass<BridgeRecord>` where each BridgeRecord is 16 bytes.

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0x50 | 4 | ptr | vtable | `0x7ED4C0` (DynVec vtable) |
| +0x54 | 4 | ptr | data_ptr | Points to array of 16-byte BridgeRecords. Accessed in `GetZoneID` (0x56D230) and `FindBridgeRecord` (0x56DA10). |
| +0x58 | 4 | int | capacity | Allocated capacity |
| +0x5C | 1 | bool | owns_memory | Memory ownership flag |
| +0x5D | 1 | bool | is_valid | — |
| +0x5E-0x5F | 2 | — | padding | — |
| +0x60 | 4 | int | count | Current bridge record count. Checked in `FindBridgeRecord`. |
| +0x64 | 4 | int | grow_step | = 10 (set in constructor) |

**BridgeRecord structure (16 bytes — verified 2026-04-06):**
```
+0x00: endpoint_a (CellStruct, packed x:i16, y:i16)
+0x04: endpoint_b (CellStruct, packed x:i16, y:i16)
+0x08: u8 is_intact (1=intact, 0=destroyed; toggled by Validate/InvalidateBridgeZones)
+0x09: u8[3] _init_residue (stack-init artifact, not read by any verified function)
+0x0C: i32 bridge_kind (0=high bridge — searchable, 1=low bridge — FindBridgeRecord skips)
```
Direction (horizontal/vertical) is NOT stored — computed geometrically as
`endpoint_a.X == endpoint_b.X → vertical, else horizontal`.

**Constructor:** `MapClass::ComputeBridgeZones` at `0x56D6E0` iterates all cells via
`CellIterator_Next`, finds bridge cells, walks perpendicular to find the other endpoint,
and pushes a record. Sets `is_intact = 1` if the perpendicular walk completes through
bridge cells, `0` if it hits a non-bridge cell mid-walk. Records are never removed —
destruction toggles only the `is_intact` byte.

### Zone Cell Data (+0x68 to +0x7F)

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0x68 | 4 | ptr | zone_cell_data | Heap pointer to **4-byte-per-cell** zone array. Each cell entry: byte[0]=zone_type (0x07=sentinel), byte[1]=height_level, bytes[2-3]=cluster_id (ushort). Accessed in `GetZoneID`, `UpdateBridgeZonesHelper`. Written by `CellClass::RecalcAttributes` (0x47D2B0): `entry[0] = cell.ZoneType`, `entry[1] = cell.Level`. Freed in destructor. |
| +0x6C | 4 | int | zone_cell_count | Number of cells in zone array. End = `zone_cell_data + zone_cell_count * 4`. |
| +0x70 | 4 | ptr | zone_speed_cache | Heap pointer to **10-byte-per-cell** zone speed cache. Each entry stores pre-computed zone IDs per speed category for fast pathfinding lookup. Freed in destructor. Accessed in `CellClass::RecalcAttributes` (0x47D2B0), `Zone_precheck` (0x42C339), `AStar_main_loop` (0x429E8A), `PathfinderClass::UpdateHierarchicalEdges` (0x42CCEB). |
| +0x74-0x7F | 12 | — | zone_metadata | Unknown zone-related fields. Not explicitly freed in destructor → likely scalar data. |

**Zone speed cache entry structure (10 bytes per cell at +0x70):**
```
+0x00: short zone_id_speed0   — zone ID for speed category 0
+0x02: short zone_id_speed1   — zone ID for speed category 1
+0x04: short zone_id_speed2   — zone ID for speed category 2
+0x06: short unknown          — possibly unused or reserved
+0x08: byte  height_level     — cell height/level (written by RecalcAttributes)
+0x09: byte  unknown          — padding or flag
```
Indexed as: `zone_speed_cache[ZoneMap::CellToZoneIndex(cell) * 10 + speed_category * 2]`
for the zone_id shorts, and `zone_speed_cache[index * 10 + 8]` for height.

### Zone Graph Pointers (+0x80 to +0x8B)

Three pointers to heap-allocated zone graph objects (one per speed category). Each
has a vtable and is destructed via vtable call in destructor (0x5652C0).

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0x80 | 4 | ptr | zone_graph[0] | Speed category 0 zone graph |
| +0x84 | 4 | ptr | zone_graph[1] | Speed category 1 zone graph |
| +0x88 | 4 | ptr | zone_graph[2] | Speed category 2 zone graph |

All three are zeroed in `MapClass::Init_Clear` (`0x5659F0` — the
original `0x565190` here was wrong; corrected per FOLLOWUP §4) and
freed via vtable destructors in the destructor (0x5652C0).

### Zone Connection DynamicVectorClasses (+0x8C to +0xD3)

Three `DynamicVectorClass` instances, each 24 bytes (0x18), constructed by
`FUN_0058ae60`. These store zone connection graph data per speed category.

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0x8C | 24 | DynVec | zone_conn_vec[0] | vtable `0x7ED4A0`, grow_step=10 |
| +0xA4 | 24 | DynVec | zone_conn_vec[1] | vtable `0x7ED4A0`, grow_step=10 |
| +0xBC | 24 | DynVec | zone_conn_vec[2] | vtable `0x7ED4A0`, grow_step=10 |

Each DynVec layout:
```
+0x00: vtable (4)
+0x04: data_ptr (4) — freed in destructor via FUN_0040b070
+0x08: capacity (4)
+0x0C: owns_memory (1)
+0x0D: flag (1)
+0x0E-0x0F: padding (2)
+0x10: count (4)
+0x14: grow_step (4) = 10
```

### Bridge Zone DynamicVectorClass (+0xD4 to +0xEB)

Another `DynamicVectorClass<int*>` constructed by `FUN_0042fcb0`.

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0xD4 | 4 | ptr | vtable | `0x7E3890` (DynVec<int*> vtable) |
| +0xD8 | 4 | ptr | data_ptr | Freed in destructor |
| +0xDC | 4 | int | capacity | — |
| +0xE0 | 1 | bool | owns_memory | At byte 0xE1 in destructor |
| +0xE1-0xE3 | 3 | — | padding | — |
| +0xE4 | 4 | int | count | — |
| +0xE8 | 4 | int | grow_step | = 10 |

### Map Size Parameters (+0xEC to +0xF3)

Set by `FUN_00565C10` (map cell init) from the `[Map] Size=left,top,width,height` values.
Cleared to 0 during scenario clear (`FUN_006851F0`).

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0xEC | 4 | int | size_left | Global: `0x87F8D4`. Set from `Size.left` but **immediately overwritten to 0** in `FUN_00565C10`. Always 0 in practice. |
| +0xF0 | 4 | int | size_top | Global: `0x87F8D8`. Set from `Size.top` but **immediately overwritten to 0** in `FUN_00565C10`. Always 0 in practice. |

### Map Bounds (Diamond Playfield) (+0xF4 to +0x10B)

These fields define the diamond-shaped playfield in isometric cell coordinates.
Used extensively by `Is_Cell_In_Playfield` (0x578460), `CellCoordToLinearIndex`
(0x56D430), `RevealShroud` (0x5673A0), and the cell iterator.

**Set by `FUN_00565C10` from `[Map] Size` rect:**

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0xF4 | 4 | int | map_size_width | Global: `0x87F8DC`. Set from `Size.width` (param_2[2]). This is the diamond half-diagonal. Used in CellCoordToLinearIndex: `stride = F8 + 1 + F4`. If > 63, sets large-map rendering mode. Read in 30+ functions. |
| +0xF8 | 4 | int | map_size_height | Global: `0x87F8E0`. Set from `Size.height` (param_2[3]). Combined with +0xF4: `F4 + F8*2` = bottom bound of diamond. |

**Set by `FUN_006E21E0` from `[Map] LocalSize` rect:**

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0xFC | 4 | int | local_left | Global: `0x87F8E4`. Set from `LocalSize.left`. Used by Is_Cell_In_Playfield, TacticalClass::DrawObjects, RadarClass, HouseClass::DetermineEdge. |
| +0x100 | 4 | int | local_top | Global: `0x87F8E8`. Set from `LocalSize.top`. |
| +0x104 | 4 | int | local_width | Global: `0x87F8EC`. Set from `LocalSize.width`. |
| +0x108 | 4 | int | local_height | Global: `0x87F8F0`. Set from `LocalSize.height`. |

**CellCoordToLinearIndex formula (0x56D430):**
```
stride = [+0xF8] + 1 + [+0xF4]  // = Size.height + 1 + Size.width
linear_index = stride * Y + X
```
This computes a compact index for zone arrays, not the full 512×512 cell array.

**Map cell creation (FUN_00565C10):**
After setting dimensions, iterates `Y = 0..[+0x130]*2+2`, `X = 0..[+0x12C]+2`,
creates `CellClass` instances (0x148 bytes each) for every cell inside the diamond
test: `F4 < X+Y`, `X-Y < F4`, `Y-X < F4`, `X+Y <= F4 + F8*2`.

**Is_Cell_In_Playfield diamond test (0x578460):**
The playfield is a diamond defined by four inequalities on `(X+Y)` and `(X-Y)`,
parameterized by the six fields above. Height correction (+0x11B slope level) is
applied when param_3 is nonzero.

### Cell Iterator State (+0x10C to +0x11B)

Used by the diagonal zigzag cell iterator (`FUN_00578290`) for map-wide operations
like `BlackoutShroud` and `RevealEntireMap`. Reset by `FUN_00578350`.

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0x10C | 4 | int | iter_state | Set to 1 at start. Alternates direction per step. |
| +0x110 | 4 | int | iter_x | Current X coordinate. Initialized to `[+0xF4]` (= Size.width). |
| +0x114 | 4 | int | iter_remaining | Remaining cells in current row. Initialized to `[+0xF4] - 1`. |
| +0x118 | 4 | ptr | iter_cell_ptr | Direct pointer into cell array: `cell_base + (F4*512 + 1)*4`. Updated each step by `-0x1FF` dwords (= -511 entries, moves diagonally up-left). |

**Iterator reset (FUN_00578350):**
```
iter_state = 1
iter_x = [+0xF4]
iter_remaining = [+0xF4] - 1
iter_cell_ptr = [+0x13C] + [+0xF4] * 0x800 + 4
             // = cell_array + (Size.width * 512 + 1) * sizeof(ptr)
```

**Iterator step (FUN_00578290):** Walks the diamond-shaped map in a zigzag
pattern. Each step: returns `*iter_cell_ptr`, decrements remaining, advances
pointer by -511 (up one row, back one column). When `iter_remaining` reaches 0,
swaps X/Y direction and adjusts for even/odd parity relative to `[+0xF4]`.

### Unknown Region (+0x11C to +0x123)

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0x11C-0x123 | 8 | — | unknown | No global xrefs found. No MapClass methods observed accessing these offsets. Likely padding or rarely-used internal state. |

### Playfield Bounds (Rectangular) (+0x124 to +0x137)

Set by `FUN_00565C10` (map cell init). The playfield_left and playfield_top are
**hardcoded to 1**. Width and height are computed as `Size.width + Size.height - 1`.

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0x124 | 4 | int | playfield_left | Global: `0x87F90C`. **Always set to 1** in `FUN_00565C10`. Used by `PlaceCrateAtRandomCell` for random X range. |
| +0x128 | 4 | int | playfield_top | Global: `0x87F910`. **Always set to 1** in `FUN_00565C10`. Used for random Y range. |
| +0x12C | 4 | int | playfield_width | Global: `0x87F914`. Set to `Size.width + Size.height - 1`. Used as inner loop bound during cell creation, and as random placement range. |
| +0x130 | 4 | int | playfield_height | Global: `0x87F918`. Set to same value: `Size.width + Size.height - 1`. Used as `height * 2 + 2` for outer cell creation loop. |
| +0x134 | 4 | int | unknown_134 | Global: `0x87F91C`. Written in `ScenarioClass::Full_Init` (0x687B9C). No other xrefs found. Purpose unclear — possibly a scenario-specific playfield flag or adjustment. |

### Cell Array VectorClass (+0x138 to +0x147)

Embedded `VectorClass<CellClass*>`, the master cell grid (512 × 512 = 262,144 entries).

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0x138 | 4 | ptr | vtable | `0x7ED480` (VectorClass vtable). Resize method called in `FUN_00565B00`. |
| +0x13C | 4 | ptr | cell_array | Points to `CellClass*[262144]`. Accessed in `Get_CellClass` (0x5657A0): `cell_array[Y*512 + X]`. Freed in destructor. |
| +0x140 | 4 | int | capacity | = 0x40000 (262144). Resized in `FUN_00565B00` if < 0x40000. |
| +0x144 | 1 | bool | owns_memory | Set to 1 in constructor. |
| +0x145 | 1 | bool | flag | Set to 0 in constructor. |
| +0x146-0x147 | 2 | — | padding | — |

**Cell lookup formula (Get_CellClass, 0x5657A0):**
```
index = Y * 0x200 + X    // Y * 512 + X
if (index < 0 || index > 0x3FFFF) → return default_cell
cell = cell_array[index]
if (cell == NULL) → return default_cell
```
Default/null cell is at global `0x00ABDC50`.

### Map Dimensions (+0x148 to +0x157)

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0x148 | 4 | int | num_movement_zones | = 0xD (13). Set in constructor (0x565090). |
| +0x14C | 4 | int | map_width_cells | = 0x200 (512). Set in `FUN_00565800` (vtable slot 5). |
| +0x150 | 4 | int | map_height_cells | = 0x200 (512). Set in `FUN_00565800`. |
| +0x154 | 4 | int | total_cell_count | = 0x40000 (262144 = 512×512). Set in `FUN_00565800`. |

### Crate Slot Table (+0x158 to +0x1157)

256 crate slots, each 16 bytes. Total = 4096 bytes (0x1000).

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0x158 | 4096 | CrateSlot[256] | crate_slots | Iterated in `UpdateCrateRegenTimers` (0x56BBE0), `PlaceCrateAtRandomCell` (0x56BD40), `RemoveCrateAtCell` (0x56C020). |

**CrateSlot structure (16 bytes):**
```
+0x00: start_frame (int)  — frame counter when timer started, -1 = no timer
+0x04: unknown (int)      — possibly crate type index
+0x08: regen_duration (int) — frames until regeneration
+0x0C: cell_x (short)     — crate position X
+0x0E: cell_y (short)     — crate position Y
```

Empty sentinel: cell coords match `DAT_00ABD480` (likely {0,0}).
Constructor zeroes only the cell coordinates (bytes +0x0C-0x0F of each slot)
via loop starting at byte +0x164 (= +0x158 + 0x0C offset within first slot).

### Final DynamicVectorClass (+0x115C to +0x1173)

Another `DynamicVectorClass<int*>`, purpose likely related to map overlay
tracking or cell change notifications. **Note:** this is a different DynVec
type from +0xD4 — its vtable `0x7E38D0` ≠ `0x7E3890` used by +0xD4.

| Offset | Size | Type | Field | Evidence |
|--------|------|------|-------|----------|
| +0x115C | 4 | ptr | vtable | `0x7E38D0` (corrected 2026-05-28: was `0x7E3890`; binary shows `param_1[0x457] = &PTR_FUN_007e38d0 = 0x7E38D0` via `decompile_function 0x00565090` — ROOT_CAUSE: OFFSET_RETYPED_WRONG, conflation with +0xD4 same-type assumption; this is a **different** DynVec specialization from +0xD4) |
| +0x1160 | 4 | ptr | data_ptr | Freed in destructor |
| +0x1164 | 4 | int | capacity | — |
| +0x1168 | 1 | bool | owns_memory | — |
| +0x1169 | 1 | bool | flag | — |
| +0x116A-0x116B | 2 | — | padding | — |
| +0x116C | 4 | int | count | — |
| +0x1170 | 4 | int | grow_step | = 10 |

**End of MapClass: +0x1174** (confirmed by DisplayClass constructor at 0x4A8730
writing its first field at `param_1[0x45D]` = byte 0x1174).

## 3. Core Logic

### Zone Lookup (GetZoneID, 0x56D230)

```
function GetZoneID(cell: CellStruct, movementZone: int, checkBridge: bool) -> uint16:
    if checkBridge:
        cellObj = g_CellArray[cell.Y * 512 + cell.X]
        if cellObj.Flags & 0x100:  // BridgeStructuralCell
            bridgeIdx = FindBridgeRecord(cell, 1, 0)
            if bridgeIdx == -1: return 0xFFFF
            // Navigate to bridge endpoint for zone lookup
            ...use bridge record endpoints...

    linear = ([+0xF8] + 1 + [+0xF4]) * cell.Y + cell.X
    linear = clamp(linear, 0, [+0x6C] - 1)

    cluster_id = zone_cell_data[linear].cluster_id  // bytes 2-3 of 4-byte entry
    return zone_ids[movementZone][cluster_id]
```

### Zone Computation (UpdateBridgeZonesHelper, 0x56C510)

1. Clear all zone connection data in hash table at [+0x14]
2. Free all 13 zone_ids arrays at [+0x18..+0x48], set to NULL
3. Zero cluster_ids in all zone_cell_data entries
4. Add sentinel zone type (7 = impassable) as first entry
5. Flood-fill scan line algorithm (`ZoneFloodFillScanLine`, 0x56CB90):
   - Walk zone_cell_data linearly
   - Skip cells with type 7 (impassable) or already assigned
   - For each unassigned cell: flood-fill connected region with new zone ID
   - Record zone adjacency edges in hash table
6. Allocate zone_ids arrays for each of 13 MovementZones
7. Populate zone_ids from cluster assignments

### Cell Iterator (FUN_00578290)

Diagonal zigzag traversal of the diamond-shaped map:
```
function NextCell() -> CellClass*:
    result = *iter_cell_ptr
    if iter_remaining > 0:
        iter_state += 1
        iter_remaining -= 1
        iter_x -= 1
        iter_cell_ptr -= 0x1FF  // -511 = up 1 row, back 1 column in 512-wide grid
    else:
        swap(iter_state, iter_x)  // swap X and state
        if (iter_x + iter_state - [+0xF4] - 1) is even:
            iter_state += 1
            iter_remaining = [+0xF4] - 2
        else:
            iter_x += 1
            iter_remaining = [+0xF4] - 1
        iter_cell_ptr = cell_array + (iter_x * 512 + iter_state) * 4
    return result
```

### Shroud Reveal (RevealShroud, 0x5673A0)

1. Convert lepton coordinates to cell coordinates with Z-height adjustment
2. Bounds-check against map diamond ([+0xF4], [+0xF8])
3. Clamp sight range to MAX_SIGHT = 10
4. Look up reveal spiral table (`DAT_007ED3D0[sightRange]` = entry count)
5. For each spiral entry:
   - Compute target cell from center + spiral offset
   - Bounds check against diamond playfield
   - Distance check: `sqrt(dx² + dy²) <= sightRange`
   - Height check: if `RulesClass+0x17EE` flag set, reject cells too far below
   - Clear shroud flag (cell+0x140 &= ~0x40)
   - Call `CellClass::RevealShroudFlags()` or `UpdateFogOfWarCell()` as appropriate

**Allied reveal:** controlled by `RulesClass+0x17E7` flag. When set, allies share vision.

### Crate System

**PlaceCrateAtRandomCell (0x56BD40):**
1. Find first empty slot (cell coords == sentinel) in 256-slot table
2. Up to 1000 random placement attempts:
   - Generate random cell within playfield bounds ([+0x124..+0x130])
   - Check if water (LandType == 2) → use water passability check
   - Find nearby passable cell via `FootClass::Find_Nearby_Passable_Cell`
   - Call `CrateSlot::PlaceOverlayAndInitTimer`

**UpdateCrateRegenTimers (0x56BBE0):**
- Only runs in multiplayer (`g_GameMode != 0`) with crates enabled (`DAT_00A8B261`)
- Iterates all 256 slots at [+0x158], stride 16 bytes
- If timer expired: clear slot and place new crate

## 4. INI Keys

MapClass itself does not directly parse INI. Map dimensions come from the scenario
file's `[Map]` section, parsed during scenario loading. Related keys:

| Key | Section | Type | Default | Effect |
|-----|---------|------|---------|--------|
| LocalSize | [Map] | 4 ints | — | Sets map diamond bounds at +0xF4/+0xF8/+0xFC/+0x100/+0x104/+0x108 |
| Size | [Map] | 4 ints | — | Sets overall map dimensions |
| CrateRegen | [CrateRules] | float (min) | — | Converted to frames: `minutes × 1800` for crate slot timers |
| CrateMinimum/Maximum | [CrateRules] | int | — | Crate count bounds |
| FogOfWar | [SpecialFlags] | bool | false | Gates fog-of-war code in RevealShroud. **TS legacy — disabled by default in YR.** |
| Shroud | [MultiplayerDialogSettings] | bool | yes | Controls whether shroud starts enabled |
| DestroyableBridges | [SpecialFlags] | bool | yes | Gates bridge destruction in MapClass bridge methods |

## 5. Integration Points

### Callers (when MapClass methods are invoked):

| Caller | Method Called | When |
|--------|-------------|------|
| `Main_Tick` (0x55DD01) | reads +0xF4, +0xF8 | Every game frame — bounds checks |
| `World::advance_tick` | `UpdateCrateRegenTimers` | Per-tick crate regeneration |
| `TechnoClass::RevealToHouses` | `RevealShroud` | When any unit reveals terrain |
| `Scenario::Init` | `FUN_00565800` (vtable[5]) | Map load — allocates cell array and zone tables |
| Bridge destruction/repair methods | `InvalidateBridgeZones`, `ValidateBridgeZones` | When bridge state changes |
| Pathfinding system | `GetZoneID`, `Can_Reach_Zone` | Pre-path zone reachability check |

### Callees (what MapClass calls):

| Method | Calls | Purpose |
|--------|-------|---------|
| `RevealShroud` | `CellClass::RevealShroudFlags`, `UpdateFogOfWarCell` | Per-cell shroud update |
| `BlackoutShroud` | `ParanoidRevealAll`, `ParanoidUnrevealAll`, `RadarClass::RefreshRadar` | Full shroud reset |
| `PlaceCrateAtRandomCell` | `FootClass::Find_Nearby_Passable_Cell`, `CrateSlot::PlaceOverlayAndInitTimer` | Crate placement |
| `UpdateBridgeZonesHelper` | `ZoneFloodFillScanLine` | Zone recomputation |

### Tick order position:

MapClass methods run at various points within the tick:
- **Crate timers**: during the "ore growth + production + repairs" phase
- **Zone recomputation**: triggered on demand (bridge destroy/repair, building placement)
- **Shroud reveal**: triggered by unit movement and sensor updates
- **Cell iteration**: used by shroud reset and reveal-all operations

## 6. Current Rust Implementation Status

The Rust codebase does NOT have a direct MapClass equivalent. Instead, functionality
is distributed across several modules:

| MapClass Feature | Rust Location | Status |
|-----------------|---------------|--------|
| Cell grid (VectorClass<CellClass*>) | `src/map/map_file.rs` (MapFile.cells), `src/map/resolved_terrain.rs` (ResolvedTerrainGrid) | Implemented — different structure but equivalent functionality |
| Zone pathfinding | `src/sim/pathfinding/zone_map.rs` | Implemented — flood-fill zone computation present |
| Passability matrix | `src/sim/pathfinding/passability.rs` | Implemented — 13×8 PASSABILITY_MATRIX |
| Bridge records | `src/sim/bridge_state.rs` (BridgeRuntimeState) | Implemented — different representation |
| Crate system | — | **Not implemented** |
| Cell iterator (diagonal zigzag) | — | **Not implemented** (not needed — Rust uses flat iteration) |
| Shroud/vision | `src/sim/vision/mod.rs` (OwnerVisibility) | Implemented — per-player visibility grid |
| Map bounds/playfield | `src/map/terrain.rs` (LocalBounds) | Implemented |
| Reveal spiral table | — | **Not implemented** (vision uses different approach) |

**Key architectural difference:** The Rust codebase separates static map data (parsing)
from dynamic simulation state, whereas gamemd.exe's MapClass owns both. The Rust
approach is better suited for the `sim/` ≠ `render/` separation invariant.

## 7. Open Questions

### Resolved (moved from previous version)

- ~~+0x70~~ → **Resolved:** 10-byte-per-cell zone speed cache (zone IDs per speed category + height)
- ~~+0xEC/+0xF0~~ → **Resolved:** Size.left/top (immediately zeroed, always 0 in practice)
- ~~+0xF4 to +0x108~~ → **Resolved:** +0xF4/F8 = Size.width/height; +0xFC-0x108 = LocalSize left/top/width/height
- ~~+0x4C~~ → **Resolved:** zone_cluster_count (written in UpdateBridgeZonesHelper: `*(param_1+0x4C) = cluster_count & 0xFFFF`)

### Remaining

1. **+0x74 to +0x7F (12 bytes):** Zone metadata region between zone_speed_cache pointer
   and zone_graph pointers. No global xrefs found at +0x74, +0x78, +0x7C. Not freed in
   destructor → likely scalar data (counts, flags, or configuration). These may be
   zone computation parameters or cached values that are only accessed through `this`
   in methods we haven't decompiled.

2. **+0x11C to +0x123 (8 bytes):** Between cell iterator and playfield bounds. No global
   xrefs found. No observed access in any decompiled MapClass method. Likely padding,
   or very rarely used state (possibly TS legacy fields).

3. **+0x134 (4 bytes):** Written in `ScenarioClass::Full_Init` (at instruction 0x687B9C).
   No read xrefs found. Purpose unknown — possibly scenario-specific flag or reserved.

4. **+0x115C DynVec purpose:** The final DynamicVectorClass — **different** vtable type from +0xD4 (vtable `0x7E38D0` vs `0x7E3890`).

7. **+0x1158 (4 bytes):** Immediately before the +0x115C DynVec. Init_Clear (`0x5659F0`) writes `*(undefined1 *)(param_1 + 0x1158) = 0` (1-byte write, confirmed via `decompile_function 0x005659f0`). Not freed in destructor → likely a scalar flag. Purpose unknown.


   Purpose unclear. May track cell change notifications or dirty regions.

5. **Zone connection hash table lifecycle:** The object at +0x14 is allocated in
   `FUN_00565800` as a 16-byte control struct + 256 DynVec buckets (0x1804 bytes).
   Hash function: `(zoneA & 0xF) << 4 | (zoneB & 0xF)`. Each bucket stores 8-byte
   edge pairs. Freed in destructor. When exactly it's rebuilt during gameplay (beyond
   bridge changes) needs more tracing.

6. **Zone graph node structure (0x24 bytes):** The zone_conn_vec data contains 36-byte
   nodes per zone (accessed as `data + zone_id * 0x24`). Fields observed:
   - +0x04: pointer to edge list
   - +0x10: edge count
   - +0x14: grow_step (= 0x14 = 20)
   - +0x18: zone cost weight (ushort)
   - +0x1C: zone cost type (int, indexes into cost table at `0x7E3794`)
   Full node layout needs more tracing through Zone_precheck and AStar.

## 8. Vtable (0x7ED404)

Selected identified methods from the MapClass vtable (30 slots — corrected 2026-05-28: was "64 entries at minimum"; `read_memory 0x7ED404` + xref analysis confirms vtable ends at slot 29; addresses past `0x7ED478` belong to the adjacent `VectorClass` vtable at `0x7ED480` — ROOT_CAUSE: RTTI_LABEL_DRIFT):

| Slot | Address | Name/Purpose |
|------|---------|-------------|
| 0 | 0x4F4240 | Scalar deleting destructor (inherited) |
| 3 | 0x5656D0 | MapClass override (unknown) |
| 4 | 0x588BF0 | MapClass override (unknown) |
| 5 | 0x565800 | **Init/Alloc** — allocates cell array, zone tables, sets map dimensions |
| 14 | 0x4F42F0 | GScreenClass::MarkNeedsRedraw (parameter 2 = full redraw) |

## Sources

### Ghidra addresses decompiled (30+ functions)

**Constructor/destructor/init chain:**
- 0x565090 (constructor — **assembly fully traced**), 0x5652C0 (destructor/clear), ~~0x565190 (init/clear)~~ → **0x5659F0 (Init_Clear)**; `0x565190` here was wrong, see FOLLOWUP §4, 0x565800 (alloc/vtable slot 5), 0x565B00 (cell array clear), 0x565C10 (**map cell init — sets +0xF4/F8/EC/F0/124-130, creates CellClass instances**)

**Base class constructors:**
- 0x4F4220 (GScreenClass ctor), 0x4A8730 (DisplayClass ctor)

**Embedded object constructors:**
- 0x58ADB0 (VectorClass<BridgeRecord> at +0x50), 0x58AE60 (zone conn VectorClass at +0x8C/A4/BC), 0x42FCB0 (VectorClass<int*> at +0xD4)

**Zone system:**
- 0x56D230 (GetZoneID), 0x56D430 (CellCoordToLinearIndex), 0x56D100 (Can_Reach_Zone), 0x56C510 (UpdateBridgeZonesHelper — **320+ lines, zone rebuild logic**), 0x56CB90 (ZoneFloodFillScanLine), 0x56D460/0x56D5A0 (zone propagation helpers)

**Pathfinding consumers (proving +0x70 zone_speed_cache):**
- 0x42C339 (Zone_precheck — uses +0x70 as 10-byte-per-cell cache), 0x47D2B0 (CellClass::RecalcAttributes — writes to +0x68 and +0x70 per-cell data)

**Shroud/vision:**
- 0x5673A0 (RevealShroud), 0x577D90 (BlackoutShroud), 0x577F30 (RevealEntireMap), 0x577AB0 (RestoreShroud — key: param_1 is `int *`, confirms all offsets via ×4), 0x4ADEE0 (ParanoidRevealAll), 0x561910 (InitRevealSpiralTable)

**Cell iterator:**
- 0x578290 (cell iterator next), 0x578350 (cell iterator reset)

**Bridge system:**
- 0x56DA10 (FindBridgeRecord), 0x578460 (Is_Cell_In_Playfield)

**Crate system:**
- 0x56BBE0 (UpdateCrateRegenTimers), 0x56BD40 (PlaceCrateAtRandomCell), 0x56C020 (RemoveCrateAtCell)

**Map dimension setters:**
- 0x6E21E0 (sets +0xFC-0x108 from LocalSize, then RecalcAttributes all cells), 0x653F50→0x565C10 (sets +0xF4/F8 from Size)

**Scenario init:**
- 0x686B20 (ScenarioClass::Full_Init — writes +0x134), 0x6851F0 (scenario clear — zeroes +0xEC-0xF8)

**Other:**
- 0x56EB80 (SetOverlayAndPropagate), 0x594870 (random cell in playfield — confirms +0xFC-0x108 usage)

### Doc files referenced
BRIDGE_SYSTEM.md, [SHROUD_SYSTEM_COMPLETE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHROUD_SYSTEM_COMPLETE.md), [ZONE_PASSABILITY_VERIFIED.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ZONE_PASSABILITY_VERIFIED.md), CELLCLASS_STRUCT_GHIDRA_REPORT.md, [TIMER_CLASSES_AND_ZONE_MAP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TIMER_CLASSES_AND_ZONE_MAP_GHIDRA_REPORT.md), GAMEMD_ARCHITECTURE.md, [OVERLAY_CLASS_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/OVERLAY_CLASS_SYSTEM_GHIDRA_REPORT.md)

### INI files checked
rulesmd.ini ([CrateRules], [SpecialFlags], [MultiplayerDialogSettings])
