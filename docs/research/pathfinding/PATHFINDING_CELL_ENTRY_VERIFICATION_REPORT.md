# Pathfinding Cell Entry Cost — Binary Verification Report

**Date:** 2026-04-04
**Binary:** gamemd.exe (YR 1.001)
**Method:** Live Ghidra MCP decompilation, memory reads, disassembly verification
**Status:** Expands and corrects existing docs

This report verifies and expands the pathfinding cell-entry cost system against
gamemd.exe. It cross-references the four existing reports:
- [PATHFINDERCLASS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/PATHFINDERCLASS_GHIDRA_REPORT.md)
- `PATHFINDING_ASTAR_GHIDRA_REPORT.md`
- [PATHFINDING_STANDALONE_FUNCTIONS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/PATHFINDING_STANDALONE_FUNCTIONS_GHIDRA_REPORT.md)
- [TERRAIN_COST_FACTSHEET.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TERRAIN_COST_FACTSHEET.md)

For each finding: **CONFIRMS**, **CORRECTS**, or **EXPANDS** existing documentation.

---

## 1. Cell Entry Cost Calculation (AStar_compute_edge_cost — 0x429830)

### 1.1 Base Cost Table — CONFIRMS existing docs

**Address:** `0x0081870c` (8 floats in .rdata)

Verified by raw memory read:

| Index | Can_Enter_Cell Code | Cost (float) | Meaning |
|-------|---------------------|--------------|---------|
| 0 | Clear | 1.0 | Freely passable |
| 1 | Crushable | 1000.0 | Strong avoidance |
| 2 | TemporaryBlock | 1.0 (base, overridden) | Moving friendly — see §1.2 |
| 3 | BridgeRamp | 1.0 | Normal traversal |
| 4 | OccupiedFriendly | 60.0 | Expensive detour |
| 5 | OccupiedEnemy | 20.0 | Moderate — may fight through |
| 6 | Cliff | 8.0 | Prefer avoiding |
| 7 | Impassable | 10000.0 | Effectively infinite |

**Confidence:** VERIFIED — raw memory matches all prior reports exactly.

### 1.2 TemporaryBlock Path-Prediction (Code 2) — CONFIRMS + EXPANDS

When `Can_Enter_Cell` returns 2 (moving friendly blocks cell), the cost function
does NOT simply use the base table cost of 1.0. Instead:

**Algorithm (decompiled from 0x429830):**

1. Select the object list from the destination cell:
   - Ground: `cell+0xE4` (FirstObject linked list)
   - Bridge: `cell+0xE8` (AltObject linked list)

2. **If `PathfinderClass+0x3C == 0` (normal mode):** iterate objects on cell:
   - Check if object is active: `(object+0x14 >> 2) & 1` (activity bit)
   - If active: determine blocking unit's facing direction:
     - If unit has zero velocity (`object+0x558 == 0.0`): read cached path direction
       from `object[0x178]` = FootClass path_queue[0]
     - If unit has velocity: derive facing from `RateTimer::Current()` —
       `(timer >> 12) + 1) >> 1) & 7`
   - Step to the next cell in that direction using `g_DirectionOffsets`
   - Check bridge/height compatibility for the next cell
   - Repeat up to **10 cells** along the predicted path
   - After loop exits without a clearing prediction: **cost = 4.0**

3. **If `PathfinderClass+0x3C == 2` (destroyer mode):** skip prediction entirely,
   **cost = 1000.0**

**Key insight:** This is a friendly-unit-path-prediction mechanism. It traces where
the blocking unit is heading for up to 10 cells to assess whether to wait or repath.
The final cost can remain 1.0 when the prediction shows the blocker will clear,
become 4.0 for a jam in normal mode, or become 1000.0 in destroyer mode.

**CORRECTS** the A* report which described this as "road-following." There is NO
road-following logic. The STANDALONE report already corrected this, confirmed here.

**Confidence:** HIGH — full decompilation verified, assembly cross-checked.

### 1.3 Temporary 0x40000 Marker Multiplier — CONFIRMS

If `cell+0x140` (CellFlags) has bit `0x40000` set:
```
cost *= 4.0    (DAT_007e37bc = 0x40800000 = 4.0f)
```

This is the A* temporary bridge-approach / peer-path marker multiplier, not a
generic cliff-ramp terrain flag.

**Confidence:** VERIFIED — matches all prior reports.

### 1.4 Bridge Diagonal Cost Modifiers — CONFIRMS + EXPANDS

Applied when `bridge_flag != 0` AND `PathfinderClass+0x01 != 0` (bridge-aware mode).

**New detail — NS vs EW bridge selection:**
The function checks `cell+0x140 & 0x800` to distinguish NS bridges from EW bridges:
- `0x800` NOT set → uses flanking table at `DAT_007e3710` (EW bridge offsets)
- `0x800` SET → uses flanking table at `DAT_007e3730` (NS bridge offsets)

Both tables are indexed by a direction value computed from:
```c
dir = DAT_007e3760[(dest_y - src_y) * 3 + (dest_x - src_x)]
```

The flanking check examines two cells adjacent to the diagonal movement:
- First flank: `param_3[table[dir]]`
- Second flank: `param_3[table[(dir - 4) & 7]]`

Cost multipliers based on bridge flag (`cell+0x140 & 0x100`) on flanking cells:

| First flank bridge | Second flank bridge | Multiplier |
|--------------------|---------------------|------------|
| YES | YES | 2.0 (DAT_007e37b4) |
| YES | NO | 1.0 (DAT_007e2ac8) |
| NO | — | 10.0 (DAT_007e37b8) |

**Confidence:** HIGH — decompilation and data tables verified.

### 1.5 Final Cost Formula — CONFIRMS + EXPANDS

For compass directions (0-7), the full formula in `AStar_main_loop` is:
```
g_cost = AStar_compute_edge_cost(...) * PathfinderClass.cost_multiplier(+0x04)
         + DirectionTiebreaker[direction]
```

For bridge crossing (direction 8), cost is **Chebyshev distance** (max of abs deltas)
cast to float. No edge cost function is called; no multiplier or tiebreaker applied.

**EXPANDS:** Bridge crossing (dir 8) uses Chebyshev distance as the step cost,
NOT the edge cost function. This was implied but not explicitly stated in prior docs.

### 1.6 Direction Tiebreaker Table — CONFIRMS

**Address:** `0x0081872c` (9 floats)

Verified by raw memory read:

| Dir | Name | Tiebreaker |
|-----|------|------------|
| 0 | N | 0.001 |
| 1 | NE | 0.005 |
| 2 | E | 0.002 |
| 3 | SE | 0.006 |
| 4 | S | 0.003 |
| 5 | SW | 0.007 |
| 6 | W | 0.004 |
| 7 | NW | 0.008 |
| 8 | Bridge | 0.000 |

Cardinals (0,2,4,6) have lower tiebreakers than diagonals (1,3,5,7), producing
a slight cardinal preference when costs are otherwise equal.

**Confidence:** VERIFIED — exact match with all prior reports.

### 1.7 Node Reopening Threshold — NEW FINDING

**Address:** `0x007e37c0` — **double** (not float) = `1.009`

When a cell is already in the closed set, the A* loop checks whether the new path
offers a significantly better g-cost before reopening:

```
if stored_g_cost < (current_node.g_cost + 1.009):
    skip  // existing path is good enough
```

This prevents marginal improvements (less than 1.009 cost units) from reopening
closed nodes. This is a performance optimization that may cause slightly suboptimal
paths in rare cases but dramatically reduces node revisitation.

**Assembly verification:** `0x429eec: FADD double ptr [0x007e37c0]` — confirmed as
double load by disassembly.

**Confidence:** HIGH — verified from both ground-level and bridge-level code paths
(two xrefs at 0x429eec and 0x429f21, both `FADD double ptr`).

**Not in existing docs.** This is a new finding.

---

## 2. Can_Enter_Cell — UnitClass (0x73f0a0)

### 2.1 Function Overview — CONFIRMS + EXPANDS

468 lines decompiled. This is the critical virtual function (vtable+0x1AC) that
determines cell passability. Returns integer 0-7 matching the cost table indices.

### 2.2 Check Order — EXPANDS

The function performs checks in this priority order:

**Phase 0 — Bridge height check:**
- If cell has bridge flag (`cell+0x140 & 0x100`) AND height difference
  `abs(param_4 - cell.Level) >= 2` → set bridge-level flag

**Phase 1 — Tunnel/tube checks:**
- If unit has a pending tunnel destination (`TechnoType+0xDFC != -1`):
  - Cell LandType == 10 (Tunnel): check tile orientation compatibility
  - If LandType != current tunnel type AND not tunnel terrain AND no valid
    overlay (0xED-0xEE range): return 7 (Impassable)

**Phase 2 — Bridge crossing (direction 8):**
- If tube exists at cell: check destination coords, return 0 (passable) or 7
- If no tube: return 7

**Phase 3 — Tube exit legality:**
- If tube exists and height mismatch is in range (3-5) and direction != -1: return 7

**Phase 4 — Bridge traversal sub-check:**
- Calls `vtable+0x1B0`, which is **not** parent/TechnoClass
  `Can_Enter_Cell`. For `UnitClass`, the slot is `CheckBridgeTraversal @
  0x4D9C60`.
- If that returns 7: return 7

**Phase 5 — Bridge-level object selection:**
- If on bridge: read objects from `cell+0xE8` (AltObject/bridge list)
- If on ground: read objects from `cell+0xE4` (FirstObject/ground list)

**Phase 6 — Shroud/fog check:**
- If not map editor mode AND cell not visible AND unit has cloaking capability
  AND `this+0x3D5 != 0` (some flag): return 7

**Phase 7 — Locomotor passability (`FootClass__LocomotorPassabilityCheck @ 0x4D9C10`):**
- Separate from the `vtable+0x1B0` bridge sub-check. This is called later inside
  `UnitClass::Can_Enter_Cell`.
- Calls locomotor COM interface for passability
- If returns 7: return 7

**Phase 8 — Overlay checks:**
- If cell has overlay:
  - `overlay+0x2AA` flag (crate?) + not player-controlled + game mode 0: return 7
  - `overlay+0x2A8` (Wall flag): complex wall/crusher/weapon checks
    - CrusherAll (MovementZone 12) can pass
    - Units with weapons can attack walls
    - Otherwise return 7

**Phase 9 — Object iteration on cell:**
Iterates through all objects on the cell (linked list via `object[0xC]`):

- **Self-check:** Skip if `object == this`
- **Team/mission target:** If unit is attacking this object's cell, return 0
- **Passengers:** If unit has `TypeClass+0xE18` (IsPassenger?) and object is
  infantry with same flag: return 0 (can share cell)
- **Building (RTTI 6):**
  - Check garrison eligibility
  - Allied building: cost 3 (BridgeRamp)
  - Enemy building: cost 5 (OccupiedEnemy)
  - Wall buildings with specific flags: cost 6 (Impassable via zone 6)
  - LaserFence buildings: return 7
- **TerrainObject (RTTI 0x24):**
  - Check water type, foundation flags → cost 5 or zone 2
- **Moving units:**
  - Active unit (has velocity or moving): classified as friendly/enemy
  - **Same-facing head-on collision detection:** If blocking unit faces toward
    this unit AND distance < 0x200 leptons: return 7 (prevent head-on deadlock)
  - Friendly moving: cost 2 (TemporaryBlock)
  - Enemy: cost 5 (OccupiedEnemy)
  - Stationary enemy: cost 5
  - Stationary friendly: cost 6 (Cliff — high penalty)

**Phase 10 — Terrain speed check (ground level only):**
```c
if (on_ground && SpeedTable[cell.LandType * 9 + unit.SpeedType] == 0.0)
    return 7;  // Impassable
```

**Phase 11 — Ground occupancy checks:**
- If cell has vehicle occupancy flag (`occupancy & 0x20`):
  - Enemy vehicle: check weapons, crusher ability
  - Friendly vehicle: cost 2 (TemporaryBlock) if moving
- If cell has infantry occupancy (`occupancy & 0x0F`): similar checks

**Final return:** Maximum cost code accumulated across all checks.

### 2.3 Cost Code Semantics — CORRECTS + EXPANDS

The cost code meanings are more nuanced than previously documented:

| Code | Meaning | Who sets it | Locomotor response |
|------|---------|-------------|-------------------|
| 0 | Clear | Default, self-cell, passenger, mission target | Proceed |
| 1 | Crushable obstacle | Neutral/civilian blocker (diplo mode 2 at +0x220) | Attempt crush |
| 2 | TemporaryBlock | Moving friendly unit, friendly vehicle | Wait + repath |
| 3 | Allied building | Allied BuildingClass on cell | Navigate around |
| 4 | Occupied (wall overlay) | Wall overlay passable by crusher | Path through |
| 5 | Enemy occupant | Enemy unit/building, TerrainObject | Attack or path around |
| 6 | Cliff/steep + friendly building | Friendly stationary unit if no weapon, building walls | Repath |
| 7 | Impassable | Terrain, bridges, walls, buildings | Full stop |

**CORRECTS** earlier docs that labeled code 3 as "BridgeRamp" — it's actually for
allied buildings on a cell, not bridge ramp transitions specifically.

**CORRECTS** earlier docs that labeled code 4 as "OccupiedFriendly" — it's actually
for wall overlays passable by certain movement zones.

**Key correction:** The codes 0-6 returned by Can_Enter_Cell are NOT terrain
classifications. They represent the WORST-CASE blocker type found when iterating
all objects on the cell. The function accumulates the maximum code across all blockers.

**Confidence:** HIGH — full 468-line function decompiled across 3 pages.

---

## 3. ZoneType (cell+0x4C) — CellClass::RecalcZoneType (0x483c80)

### 3.1 ZoneType Is NOT LandType — CONFIRMS + CLARIFIES

The `cell+0x4C` field ("ZoneType" or "computed land category") is distinct from
`cell+0xEC` (LandType). ZoneType has 8 values (0-7) used as the column index in
the passability matrix. LandType has 12 values (0-11) used for speed table lookups.

### 3.2 Assignment Priority — CONFIRMS with one CORRECTION

**CORRECTS** the TERRAIN_COST_FACTSHEET which had confusing non-obvious mappings.
Here is the verified assignment order:

| Priority | Condition | ZoneType | Value |
|----------|-----------|----------|-------|
| 1 | Cell outside playfield | OutOfBounds | 7 |
| 2 | Overlay has `IsCrate` flag (+0x22D) | Road | 1 | <!-- corrected 2026-05-28: was 'IsWall'; binary CellClass__RecalcZoneType plate comment reads 'IsCrate flag +0x22D' — ROOT_CAUSE: RTTI_LABEL_DRIFT via decompile_function 0x483c80 -->
| 3 | Overlay has `IsWall` flag (+0x2A8) | Water | 2 | <!-- corrected 2026-05-28: was 'IsWater'; Ghidra plate comment and UnitClass__Can_Enter_Cell both identify +0x2A8 as IsWall — ROOT_CAUSE: RTTI_LABEL_DRIFT via decompile_function 0x483c80 + 0x73f0a0 -->
| 4 | Overlay Foot speed == 0.0 | Impassable | 6 |
| 5 | Overlay `IsRailroad` (+0x2B5) | Impassable | 6 |
| 6 | Overlay `IsRubble` (+0x2B4) | → skip to default | 0 |
| 7 | LandType == 2 (Water tile) | DeepWater | 4 |
| 8 | LandType == 6 (Beach tile) | Beach | 3 |
| 9 | **Foot speed for LandType ≤ 0.01** | Impassable | 6 |
| 10 | Building (RTTI 6) with LaserFence | Impassable | 6 |
| 10 | Building (RTTI 6) gate (wrong mask) | Impassable | 6 |
| 11 | TerrainObject (RTTI 0x24): water type 7 + game mode | Water | 2 |
| 11 | TerrainObject (RTTI 0x24): other | Terrain | 5 |
| 12 | Default | Clear | 0 |

**CORRECTION on Priority 9:** The previous report said "Foot speed ≤ threshold" but
described the threshold incorrectly. The actual comparison is:

```asm
FLD float ptr [EAX*0x4 + 0x89ea48]   ; load Foot speed (SpeedType 0 + 2 = Wheel? No...)
FCOMP double ptr [0x007e3808]          ; compare against double 0.01
```

Wait — `0x89ea48` = `0x89ea40 + 8`. The table at `0x89ea40` is `g_SpeedType_LandType_Table`
with layout `[LandType][SpeedType]` where SpeedType stride is 9 floats. Offset +8 = index 2 =
**Wheel** speed type. But the first comparison (Priority 4 for overlay) uses the SAME offset:
`(&DAT_0089ea48)[overlay.LandType * 9]` which is `SpeedTable[overlay.LandType * 9 + 2]` = Wheel
speed for that land type.

**CORRECTION:** Both comparisons check the **Foot** speed (index 0), NOT Wheel. The reason
`DAT_0089ea48` appears instead of `DAT_0089ea40` is because the address is
`g_SpeedType_LandType_Table + 8` = pointer to the third float in the first row. With the
indexing `[iVar3 * 9]`, the actual lookup is:
```
address = 0x89ea48 + iVar3 * 9 * 4 = 0x89ea40 + 8 + iVar3 * 36
```
This is `SpeedTable[iVar3 * 9 + 2]` which IS SpeedType 2 (Wheel), not Foot.

So the check is actually: **if Wheel speed for this LandType ≤ 0.01, zone = Impassable(6)**.
This makes sense — Wheel is a middle-ground speed type. If even wheeled vehicles can't traverse
it at any reasonable speed, the zone is marked impassable.

**Note on the == 0.0 check (Priority 4):** `FCOMP float ptr [0x007e1748]` where
`0x007e1748` = float 0.0. So overlay Foot speed exactly 0.0 → zone 6.

Actually, wait. Let me re-examine. `DAT_0089ea48` = `g_SpeedType_LandType_Table[2]` (the
third float in the flat array). The table is laid out as `Row[LandType] = 9 floats
{Foot, Track, Wheel, Hover, Winged, Float, Amphibious, FloatBeach, Buildable}`.
So `DAT_0089ea48[LandType * 9]` = `Table[LandType * 9 + 2]` = **Wheel speed** for that LandType.

For Priority 4 (overlay check): `DAT_0089ea48[overlay.Land * 9] == 0.0` means
"Wheel speed for overlay's land type is zero" → zone 6.

For Priority 9 (base terrain check): `DAT_0089ea48[baseLandType * 9] <= 0.01` means
"Wheel speed for base land type is ≤ 0.01" → zone 6.

**This means the zone system uses Wheel speed as the passability reference**, not Foot.
If a terrain type has zero Wheel speed, it's considered impassable for zone purposes.

**Confidence:** HIGH — verified by disassembly showing `float ptr [EAX*0x4 + 0x89ea48]`
and cross-referencing with the speed table layout.

### 3.3 ZoneType Column Mapping for Passability Matrix — EXPANDS

The 8 ZoneType values map to the passability matrix columns as follows:

| ZoneType | Value | Matrix Column | Set By |
|----------|-------|---------------|--------|
| Clear | 0 | Clear | Default |
| Road | 1 | Road | Wall overlay (IsWall flag) |
| Water | 2 | Water | Water overlay / water terrain obj |
| Beach | 3 | Rock | Beach tile (LandType 6) |
| DeepWater | 4 | Wall | Water tile (LandType 2) |
| Terrain | 5 | Tiberium | TerrainObject (non-water) |
| Impassable | 6 | Beach | Overlay/terrain with zero speed |
| OutOfBounds | 7 | Rough | Outside playfield |

**IMPORTANT — Non-obvious column names:** The passability matrix column names do NOT match
the ZoneType names. ZoneType "Beach" (3) maps to matrix column "Rock" (3). ZoneType
"DeepWater" (4) maps to matrix column "Wall" (4). This is because the passability matrix
columns were named after their primary terrain type in TS, but YR reused the same matrix
with different zone semantics.

**Confidence:** HIGH — verified from memory dump of passability matrix at `0x0082a594`.

---

## 4. Passability Matrix (0x0082a594) — CONFIRMS

13 rows (MovementZone) × 8 columns (ZoneType), 4 bytes per entry (int).

Verified from raw memory dump — exact match with TERRAIN_COST_FACTSHEET:

```
        Clear Road Water Rock Wall  Tib  Beach Rough
Normal:  [1,   2,   2,    2,   2,   2,   2,    3]
Crusher: [1,   1,   2,    2,   2,   2,   2,    3]
Destr:   [1,   1,   1,    2,   2,   2,   2,    3]
AmpDest: [1,   1,   1,    1,   1,   1,   2,    3]
AmpCrsh: [1,   1,   2,    1,   1,   2,   2,    3]
Amphib:  [1,   2,   2,    1,   1,   2,   2,    3]
Subterr: [1,   1,   1,    2,   2,   2,   1,    3]
Infntry: [1,   2,   2,    2,   2,   1,   2,    3]
InfDest: [1,   1,   1,    2,   2,   1,   2,    3]
Fly:     [1,   1,   1,    1,   1,   1,   1,    3]
Water:   [2,   2,   2,    2,   1,   2,   2,    3]
WtrBch:  [2,   2,   2,    1,   1,   2,   2,    3]
CrshAll: [1,   1,   1,    2,   2,   2,   2,    3]
```

Values: 1=passable, 2=blocked, 3=destroyable (walls that some units can break through)

**Indexing:** `matrix[MovementZone * 8 + ZoneType]`

**Confidence:** VERIFIED — binary match confirmed by memory read.

---

## 5. Zone Map System

### 5.1 Per-Cell Zone Data Structure — EXPANDS

**MapClass+0x68**: Array of 4-byte entries, one per cell (TotalCellCount entries).

| Byte Offset | Type | Field |
|-------------|------|-------|
| 0 | byte | ZoneType (0-7, initialized to 7=OutOfBounds) |
| 1 | byte | Height level |
| 2-3 | ushort | Node index (used by GetZoneID for zone lookup) |

**MapClass+0x70**: Array of 10-byte entries, one per cell.

| Byte Offset | Type | Field |
|-------------|------|-------|
| 0-1 | ushort | Zone ID at hierarchical level 0 (fine) |
| 2-3 | ushort | Zone ID at hierarchical level 1 (medium) |
| 4-5 | ushort | Zone ID at hierarchical level 2 (coarse) |
| 6-7 | ushort | Parent zone ID at next level |
| 8 | byte | Height level (duplicated from +0x68 entry) |
| 9 | byte | Padding/unused |

### 5.2 Zone ID Lookup (MapClass::GetZoneID — 0x56d230) — EXPANDS

**Signature:** `uint GetZoneID(MapClass *this, CellStruct *cell, int zoneCategory, char bridgeAware)`

**Algorithm:**

1. **Bridge resolution (if bridgeAware):**
   - Get cell from coordinates
   - If cell has bridge flag (0x100):
     - Find bridge record via `MapClass::FindBridgeRecord`
     - If bridge record `byte+8 == 0`: walk cell downward using
       `Pathfinding_update_continued` until finding a non-bridge cell
     - Check if non-bridge cell is a bridge ramp; if so, use alternate coords
   - Result: cell coordinates pointing to the ground cell under the bridge

2. **Linear index computation:**
   ```c
   linearIndex = (MapWidth + 1 + MapHeight) * cell_y + cell_x
   ```
   Clamped to `[0, TotalCellCount - 1]`.

3. **Zone ID lookup (two-level indirection):**
   ```c
   nodeIndex = *(ushort*)(MapClass+0x68 + linearIndex * 4 + 2)
   zoneId = *(ushort*)(zoneTable[zoneCategory] + nodeIndex * 2)
   ```
   Where `zoneTable` array is at `MapClass+0x18 + zoneCategory * 4`.

**Wait — MapClass+0x18:** This seems too early in MapClass. Looking at the
PathfinderClass report: PathfinderClass is at MapClass+0xEC. The zone tables might
be somewhere else. Verifying from AStar_main_loop:

In AStar_main_loop, the zone lookup uses `DAT_0087f858` for the per-cell zone data:
```c
iVar17 = ZoneMap__CellToZoneIndex(psVar1);  // returns linear index
uVar14 = (uint)*(short*)(DAT_0087f858 + iVar17 * 10);  // level 0 zone ID
```

So `DAT_0087f858` is a global pointer to the 10-byte-per-cell zone array (MapClass+0x70's
content). The zone ID for level 0 is at byte offset 0 within each 10-byte entry. For
other levels, offset is `level * 2`.

**5 zone categories:** GetZoneID takes `param_3` as the zone category (0-4). The lookup
table at `MapClass+0x18 + param_3 * 4` has 5 entries, one per MovementZone category:

Looking at the per-cell 10-byte layout, only 3 hierarchical levels (0-2) have zone IDs
stored at offsets 0, 2, 4. The other 2 entries at offsets 6 and 8 store parent/height info.

**Confidence:** HIGH for the basic mechanism, MEDIUM for exact field-by-field 10-byte
layout (some fields need more tracing).

### 5.3 Zone Flood Fill (FUN_005824a0) — NEW ANALYSIS

Called from `FUN_00581f90` (zone level builder). For each hierarchical level:

1. Iterates through all cells in row-major order
2. For each unassigned cell (zone ID == 0 AND ZoneType != 7):
   - Starts a scan-line flood fill:
     - Scans left along the row until: edge of fill area OR height difference ≥ 2
     - Marks all scanned cells with the current zone ID
   - Records zone adjacency edges when touching cells in different zones
   - Computes zone centroid from fill extents

3. Zone adjacency data stored in per-level structures at `DAT_0087f878 + level * 0x18`:
   - Each zone has an adjacency list with entries: `{neighbor_zone_id, is_diagonal, edge_type}`
   - Edge type corresponds to the ZoneType of the transition cell

**Hierarchical subdivision sizes** (from FUN_00567110):
```c
for level 1..3:
    subdivision_size = (MapWidth * MapHeight * 4) / (level_size * level_size)
```
Where `level_size = 1 << level`. So:
- Level 0: finest granularity
- Level 1: 4x coarser (2×2 cells per zone subdivision)
- Level 2: 16x coarser (4×4 cells per zone subdivision)

### 5.4 Zone Connectivity for Bridges — EXPANDS

From `FUN_00567110` (InitZoneMap):
1. After basic zone flood-fill, calls `MapClass::ComputeBridgeZones`
2. Then `MapClass::UpdateBridgeZonesHelper`
3. For each bridge record: calls `FUN_00582d70` (bridge/tunnel path resolution)
   which updates zone adjacency to include bridge-crossing edges

Bridge zones connect the ground-level zones on either side of the bridge, allowing
the zone precheck to find paths that cross bridges.

**Confidence:** MEDIUM — bridge zone setup traced at function call level but not
fully decompiled.

---

## 6. A* Neighbor Expansion Details

### 6.1 Direction Count — CONFIRMS

**9 directions:** 8 compass (0-7) + 1 bridge crossing (8).

Verified from AStar_main_loop: `do { ... iStack_44 = iStack_44 + 1; } while (iStack_44 < 9);`

### 6.2 Diagonal Cost — NEW FINDING (clarification)

There is **NO diagonal cost multiplier** (√2) in the A* search. All compass directions
use the same base cost from the cost table. The only directional bias comes from the
tiny tiebreaker values (0.001-0.008). This means the A* search treats diagonal moves
as having the SAME cost as cardinal moves (both = 1.0 for clear terrain).

This is intentional — the heuristic is Euclidean distance which accounts for diagonal
geometry, so the path still prefers diagonals when they're shorter.

**CORRECTS** the Rust implementation which uses `CARDINAL_COST=10, DIAGONAL_COST=14`
(octile integer costs). The original does NOT differentiate cardinal vs diagonal cost.

**Confidence:** HIGH — verified from decompilation. The `AStar_compute_edge_cost` function
receives the cell and a bridge flag but NOT a direction — it has no way to distinguish
cardinal from diagonal moves.

### 6.3 Occupied Cell Handling — CONFIRMS + EXPANDS

Occupied cells are NOT blocked — they receive increased cost based on the Can_Enter_Cell
return code:

| Situation | Can_Enter_Cell code | A* cost |
|-----------|---------------------|---------|
| Clear terrain | 0 | 1.0 |
| Crushable unit | 1 | 1000.0 |
| Moving friendly | 2 | 1.0 / 4.0 / 1000.0 depending prediction and urgency |
| Allied building | 3 | 1.0 |
| Wall/gate | 4 | 60.0 |
| Enemy unit | 5 | 20.0 |
| Friendly stationary | 6 | 8.0 |
| Impassable | 7 | NOT EXPANDED (filtered) |

Units with `Crusher` flag (TypeClass+0xC94) force code to 0 for any passable cell,
meaning crushers pathfind through everything at base cost.

### 6.4 Bridge Cell Transitions — CONFIRMS + EXPANDS

**Ground → Bridge:** Determined by height comparison.
```c
if (cell.Flags & 0x100) != 0:  // cell has bridge
    if abs(currentHeight - cell.Level) < 2:
        // Ground level — use ground closed set
    else:
        // Bridge level — use bridge closed set
```

**Bridge → Ground (direction 8):** When the current cell has a bridge partner
(`cell+0x116 != -1`), look up the bridge record table (`DAT_008b413c`) to find the
endpoint cell. The A* creates a node at that endpoint with the Chebyshev distance
as cost.

### 6.5 Zone Corridor Constraint — CONFIRMS

During A* expansion, each neighbor cell's zone ID is checked against the hierarchical
corridor established by Zone_precheck:

```c
if (zoneId == PathfinderClass.hier_path[corridor_index]) {
    // Advance corridor index
    PathfinderClass.corridor_index++;
    PathfinderClass.corridor_cell = cell_coords;
}
```

This keeps the cell-level search within the zone corridor from the precheck,
dramatically reducing search space for long-distance paths.

### 6.6 Aircraft Override — CONFIRMS

```c
if (unit_is_crusher_capable && cost < 7) {
    cost = 0;  // Aircraft/crushers ignore terrain costs
}
```

This was labeled `bVar10` in the decompilation. It's set by checking
`TechnoTypeClass+0xC94` (IsCrusher flag). When set, all passable cells (code < 7)
are treated as clear (cost 0), eliminating terrain cost influence on pathfinding.

**Note:** Despite the field name "IsCrusher", this applies to both crushers and
aircraft — both bypass terrain costs entirely in A* search.

---

## 7. Speed Multipliers (Runtime Movement vs Pathfinding)

### 7.1 Terrain Speed Table — CONFIRMS

**Address:** `0x0089ea40` (BSS, populated from INI at runtime)
**Layout:** `float[12 LandTypes][9 entries per row]` where entries 0-7 are SpeedTypes
and entry 8 is Buildable flag.

**Key fact:** The A* pathfinding cost function does NOT use this table. The speed
table only affects runtime movement speed in `DriveLocomotionClass::Process_Movement`.

The A* cost depends solely on the Can_Enter_Cell return code (0-7) mapped through
the base cost table at `0x0081870c`.

### 7.2 The Speed Table IS Used for Zone Assignment — CORRECTS

While the A* cost function doesn't use the speed table, `CellClass::RecalcZoneType`
DOES use it to classify cells:

- Overlay passability: `SpeedTable[overlay.LandType * 9 + 2] == 0.0` → zone 6
- Terrain passability: `SpeedTable[baseLandType * 9 + 2] <= 0.01` → zone 6

Where index +2 = Wheel speed type. So the zone system uses **Wheel speed** as the
reference for whether terrain is passable enough to include in a zone.

### 7.3 CellClass::CheckCellPassability (0x4834a0) — EXPANDS

This is a general-purpose cell passability function (NOT the A* Can_Enter_Cell).
Used for placement checks and zone flood-fill validation.

**Checks performed:**
1. Zone ID match (if expected zone provided)
2. Height/bridge compatibility
3. Occupation flags (infantry sub-cells, vehicle, building)
4. **Wall overlay special case:** If cell has Wall overlay (IsWall flag at +0x2A8 — corrected 2026-05-28: was 'IsWater'; +0x2A8 is IsWall per decompile_function 0x4834a0 + 0x483c80 — ROOT_CAUSE: RTTI_LABEL_DRIFT),
   MovementZones 2, 3, 8 (Destroyer variants) and 1, 4 (Crusher variants with
   IsCrate +0x22D flag — corrected 2026-05-28: was 'IsWall'; +0x22D is IsCrate per decompile_function 0x483c80) and 12 (CrusherAll) can pass. Others return 0 (blocked).
5. **Speed table check:** `SpeedTable[SpeedType + LandType * 9] == 0.0` → blocked
   (but NOT on bridge level)

**Confidence:** HIGH — fully decompiled.

---

## 8. Hierarchical Pathfinding — CONFIRMS + EXPANDS

### 8.1 Structure — CONFIRMS

YR uses a 3-level hierarchical zone system with zone-level Dijkstra as a quick
reject + corridor constraint, NOT as a replacement for cell-level A*.

The hierarchy is:
1. **Zone precheck** (Dijkstra on zone adjacency graph) — establishes corridor
2. **Cell-level A*** — constrained to corridor zones
3. **Retry with edge invalidation** — if A* fails, mark broken zone edges, retry

### 8.2 Zone-Level Search (Zone_precheck — 0x42c290) — CONFIRMS + EXPANDS

**Iterates levels 2 → 0 (coarse to fine):**

For each level:
1. Get source and dest zone IDs at this level
2. If same zone: corridor = [that zone], done for this level
3. Otherwise: Dijkstra on zone adjacency graph:
   - **Edge cost:** `ZoneEdgeCostTable[edge_type] + diagonal_penalty`
     where `ZoneEdgeCostTable` at `0x007e3794`:
     ```
     [0]=1.0, [1]=0.0, [2]=0.0, [3]=1.0, [4]=1.0, [5]=0.0, [6]=1.0, [7]=1.0
     ```
     Diagonal penalty = 0.001 (double at `0x007e3818`)
   - **Passability filter:** `g_PassabilityMatrix[MovementZone * 8 + edge_type] == 1`
   - **Cross-level constraint:** At levels 0 and 1, the search also checks that the
     neighboring zone's parent zone (at next coarser level) is in the corridor from
     the coarser level's solution. This restricts finer searches to zones within the
     coarser corridor.
4. Store zone path in `PathfinderClass.hier_path[level]` (up to 500 zones per level)

**NEW — Threat avoidance integration:**
```c
if (param_5 != 0) {  // unit has threat data
    FUN_00585f40(threat_data, level, source_zone, neighbor_zone);
    threat_cost = Math__ftol();
}
edge_cost += threat_cost;
```
The zone precheck can incorporate threat costs from the unit's threat map, making
units prefer safer zone-level paths.

**Confidence:** HIGH — 200+ lines decompiled.

### 8.3 Retry Mechanism — CONFIRMS

Max retries: 5 (if max_search_depth == -1) or 1 (if specific depth given).

On A* failure:
1. `UpdateHierarchicalEdges` — re-flood from corridor cell, detect broken edges
2. `InvalidateZoneEdge` — remove broken zone-zone edges from adjacency graph
3. `Reset` — clear search state
4. Re-run `Zone_precheck` with updated edges
5. Re-run `AStar_main_loop`

If `search_valid` flag (+0x38) is cleared during this process, abort.

---

## 9. CellClass Field Map (Pathfinding-Relevant) — CONSOLIDATED

| Offset | Type | Field | Used By |
|--------|------|-------|---------|
| +0x24 | packed short x, short y | MapCoord | All pathfinding |
| +0x38 | int | IsoTileTypeIndex | RecalcAttributes |
| +0x44 | int | OverlayTypeIndex | RecalcZoneType, Can_Enter_Cell |
| +0x4C | int | ZoneType (0-7) | Passability matrix column |
| +0xE4 | ptr | FirstObject (ground list) | Can_Enter_Cell, edge cost |
| +0xE8 | ptr | AltObject (bridge list) | Can_Enter_Cell, edge cost |
| +0xEC | int | LandType (0-11) | RecalcZoneType, speed table |
| +0x116 | short | BridgePartnerIndex | Bridge crossing (dir 8) |
| +0x11A | byte | TunnelDirection? | Tunnel checks in Can_Enter_Cell |
| +0x11B | char | Level (height) | Height comparison, zone fill |
| +0x11C | byte | SlopeIndex | RecalcAttributes |
| +0x122 | byte | ShroudStatus | Fog-of-war check in A* |
| +0x124 | uint | OccupationFlags_Ground | Occupancy checks |
| +0x128 | uint | OccupationFlags_Bridge | Occupancy checks |
| +0x140 | uint | Flags | Bridge (0x100), NS (0x800), temporary A* marker (0x40000) |

---

## 10. Gaps in Rust Implementation

Based on this verification, the key gaps between our Rust implementation and gamemd.exe:

### Critical (affects path quality):

1. **A* cost model is wrong:** Rust uses CARDINAL_COST=10, DIAGONAL_COST=14 (octile).
   Original uses float costs from Can_Enter_Cell return code (1.0-1000.0 scale) with
   NO diagonal multiplier. All directions have the same base cost.

2. **No occupation-aware pathfinding:** Rust A* doesn't consider enemy/friendly
   occupation in cell cost. Original weighs occupation heavily (60.0 for friendly
   stationary, 20.0 for enemy, etc.).

3. **No TemporaryBlock path prediction:** When a moving friendly blocks a cell,
   the original traces the blocker's path 10 cells ahead. The cost can remain
   1.0 if the blocker will clear, become 4.0 if jammed, or become 1000.0 in
   destroyer urgency. This helps units decide whether to wait or repath.

### Important (affects path behavior):

4. **No direction tiebreakers:** Original adds 0.001-0.008 epsilon per direction
   to break ties deterministically. Without this, paths may oscillate.

5. **No bridge diagonal cost modifiers:** Original multiplies diagonal bridge
   crossing costs by 1.0/2.0/10.0 based on flanking cell geometry.

6. **No temporary 0x40000 marker multiplier:** Original multiplies cells carrying
   the search-scoped bridge-approach marker by 4.0.

7. **No node reopening threshold:** Original uses 1.009 threshold to prevent
   marginal reopenings. This is a performance optimization.

8. **No cost_multiplier field:** PathfinderClass+0x04 scales all edge costs.

### Structural:

9. **Single zone level vs 3 hierarchical levels:** Original subdivides zones at
   3 granularities for faster long-distance pathfinding.

10. **No stamp-based closed set:** Rust allocates fresh Vec per search. Original
    uses persistent singleton with O(1) stamp-based clearing.

11. **Zone assignment uses Wheel speed reference:** Our zone system may use a
    different speed type for passability classification.

---

## 11. Functions Verified/Labeled in This Session

All functions below were already labeled from prior sessions. Decompilation verified
their behavior matches the labels:

| Address | Name | Status |
|---------|------|--------|
| 0x429830 | AStar_compute_edge_cost | Verified |
| 0x429a90 | AStar_main_loop | Verified |
| 0x42c290 | Zone_precheck | Verified |
| 0x483c80 | CellClass__RecalcZoneType | Verified |
| 0x4834a0 | CellClass__CheckCellPassability | Verified |
| 0x56d230 | MapClass__GetZoneID | Verified |
| 0x56d3f0 | ZoneMap__CellToZoneIndex | Verified |
| 0x567110 | MapClass__InitZoneMap (FUN_00567110) | Verified |
| 0x5840c0 | ZoneMap__FloodFillReachableZones | Verified |
| 0x73f0a0 | UnitClass__Can_Enter_Cell | Verified (468 lines) |
| 0x4d9c10 | FootClass__LocomotorPassabilityCheck | **Newly identified** |
| 0x581f90 | ZoneMap__BuildZoneLevel | **Newly identified** |
| 0x5824a0 | ZoneMap__FloodFillScanline | **Newly identified** |

---

## Sources

### Ghidra MCP decompilation:
- 0x73f0a0 — UnitClass::Can_Enter_Cell (468 lines, 3 pages)
- 0x429830 — AStar_compute_edge_cost (full)
- 0x429a90 — AStar_main_loop (427 lines, 3 pages)
- 0x42c290 — Zone_precheck (295 lines, 2 pages)
- 0x483c80 — CellClass::RecalcZoneType (79 lines, full)
- 0x4834a0 — CellClass::CheckCellPassability (full)
- 0x56d230 — MapClass::GetZoneID (full)
- 0x56d3f0 — ZoneMap::CellToZoneIndex (full)
- 0x567110 — MapClass::InitZoneMap (64 lines, full)
- 0x581f90 — ZoneMap::BuildZoneLevel (218 lines)
- 0x5824a0 — ZoneMap::FloodFillScanline (100+ lines)
- 0x4d9c10 — FootClass::LocomotorPassabilityCheck (18 lines, full)
- 0x568bb0 — MapClass::InitCellAttributes (108 lines, full)
- 0x4cbba0 — FootClass::Run_AStar (21 lines, full)
- 0x5840c0 — ZoneMap::FloodFillReachableZones (188 lines)

### Raw memory reads:
- 0x0082a594 — Passability matrix (416 bytes, 13×8 ints)
- 0x0081870c — A* base cost table (32 bytes, 8 floats)
- 0x0081872c — Direction tiebreaker table (36 bytes, 9 floats)
- 0x007e37c0 — Node reopening threshold (8 bytes, double = 1.009)
- 0x007e3808 — Zone speed threshold (8 bytes, double = 0.01)
- 0x007e1748 — Zero constant (4 bytes, float = 0.0)
- 0x007e3794 — Zone edge cost table (32 bytes, 8 floats)
- 0x007e3818 — Zone diagonal penalty (8 bytes, double = 0.001)

### Disassembly verification:
- 0x483c80 — RecalcZoneType FLD/FCOMP instructions (float vs double data types)
- 0x429a90 — AStar_main_loop FADD double ptr at 0x429eec (reopening threshold)
