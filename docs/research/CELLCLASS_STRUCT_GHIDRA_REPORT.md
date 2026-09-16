# CellClass — Complete Struct Layout & RecalcAttributes Pipeline

**Primary addresses:** Constructor `0x0047bbf0`, RecalcAttributes `0x0047d2b0`, RecalcZoneType `0x00483c80`
**Struct size:** 328 bytes (0x148)
**Confidence:** HIGH (all offsets verified from Ghidra decompilation)
**Active in YR:** Yes — CellClass is the fundamental per-cell map data structure

## 1. Overview

CellClass is the per-cell data structure for every map cell in gamemd.exe. The global cell array
(`g_CellArray_Base`) holds pointers to CellClass instances indexed by `Y * 512 + X`. CellClass
stores terrain type, height, slope, overlay, occupancy, shroud/sensor state, bridge flags, and
Z-adjust rendering data. It is the most cross-cutting struct in the engine — pathfinding, movement,
combat, overlays, terrain, bridges, shroud, and rendering all read/write CellClass fields.

CellClass inherits from AbstractClass (with INoticeSink interface), giving it 4 vtable pointers
at offsets 0x00-0x0C and AbstractClass base fields at 0x10-0x23.

## 2. Complete Struct Field Map

### Legend
- **HIGH** = offset verified from Ghidra decompilation of function that reads/writes it
- **MEDIUM** = offset from Ghidra struct definition or single function reference
- **LOW** = inferred from initialization pattern in constructor

### Base Class (0x00-0x23)

| Offset | Size | Type | Name | Init | Evidence | Conf |
|--------|------|------|------|------|----------|------|
| 0x00 | 4 | ptr | vtable | vtable__CellClass | Constructor | HIGH |
| 0x04 | 4 | ptr | vtable_secondary_1 | vtable_secondary_4 | Constructor | HIGH |
| 0x08 | 4 | ptr | vtable_secondary_2 | vtable_secondary_8 | Constructor | HIGH |
| 0x0C | 4 | ptr | vtable_secondary_3 | vtable_secondary_12 | Constructor | HIGH |
| 0x10-0x23 | 20 | — | AbstractClass base | — | INoticeSink_Constructor | MED |

### Map Coordinates (0x24-0x27)

| Offset | Size | Type | Name | Init | Evidence | Conf |
|--------|------|------|------|------|----------|------|
| 0x24 | 2 | short | MapCoord_X | 0 | Ghidra struct + many funcs | HIGH |
| 0x26 | 2 | short | MapCoord_Y | 0 | Ghidra struct + many funcs | HIGH |

### Pointers & Links (0x28-0x3F)

| Offset | Size | Type | Name | Init | Evidence | Conf |
|--------|------|------|------|------|----------|------|
| 0x28 | 4 | ptr | CellTag | 0 | Constructor (freed in dtor). CELL_OCCUPATION, OBJECT_FOG_VISIBILITY | MED |
| 0x2C | 4 | ptr | BridgeAnchorPtr | 0 | BRIDGE_SYSTEM: set by SetBridgeDirection_NESW | HIGH |
| 0x30 | 4 | int | Unknown_0x30 | 0 | Constructor | LOW |
| 0x34 | 4 | ptr | LightConvert | 0 | Ghidra struct | MED |
| 0x38 | 4 | int | IsoTileTypeIndex | 0xFFFF | Ghidra struct + IsBridge, IsClearTile, etc. | HIGH |
| 0x3C | 4 | ptr | AttachedTag | 0 | Ghidra struct, BRIDGE_SYSTEM correction | HIGH |

### Overlay & Smudge (0x40-0x4F)

| Offset | Size | Type | Name | Init | Evidence | Conf |
|--------|------|------|------|------|----------|------|
| 0x40 | 4 | int | Unknown_0x40 | 0 | Constructor | LOW |
| 0x44 | 4 | int | OverlayTypeIndex | -1 | Ghidra struct + RecalcAttributes, Reduce_Tiberium | HIGH |
| 0x48 | 4 | int | SmudgeTypeIndex | -1 | Ghidra struct | MED |
| 0x4C | 4 | int | ZoneType | 0 | RecalcZoneType writes `field_0x4c` | HIGH |

### Zone Layers & Draw Cache (0x50-0x7B)

| Offset | Size | Type | Name | Init | Evidence | Conf |
|--------|------|------|------|------|----------|------|
| 0x50 | 4 | int | Unknown_0x50 | -1 | Constructor | LOW |
| 0x54 | 4 | int | InfantryOwnerGround | -1 | INFANTRY_SUBCELL: house ID of infantry on ground subcells | MED |
| 0x58 | 4 | int | InfantryOwnerBridge | -1 | INFANTRY_SUBCELL, BRIDGE_SYSTEM: bridge layer counterpart | MED |
| 0x5C | 4 | int | LastDirtyFrame | -1 | SHROUD_DISPARITIES: frame counter dedup | MED |
| 0x60 | 4 | int | Unknown_0x60 | -1 | Constructor | LOW |
| 0x64 | 4 | int | DrawCacheFrame | -1 | BRIDGE_RENDERING: frame when cell last drawn | MED |
| 0x68 | 16 | int[4] | DrawCacheClipRect | 0 | BRIDGE_RENDERING: cached clip rect (x,y,w,h) | MED |
| 0x78 | 4 | uint | VisibleToHouses | 0 | IsVisibleToHouse: `bit(house_id)`. CLOAKING_VISUAL | HIGH |

### Per-House Sensor Array (0x7C-0xAB) — short[24]

| Offset | Size | Type | Name | Init | Evidence | Conf |
|--------|------|------|------|------|----------|------|
| 0x7C | 48 | short[24] | SensorCounts | all 0 | IncrementSensorCount: `+0x7C + house*2` | HIGH |

Each house has a 2-byte sensor count. `SensorCountForHouse` returns `count > 0`.
Array is 48 bytes (24 shorts), covering up to 24 houses.

### Per-House Disguise Detect Array (0xAC-0xDB) — short[24]

| Offset | Size | Type | Name | Init | Evidence | Conf |
|--------|------|------|------|------|----------|------|
| 0xAC | 48 | short[24] | DisguiseDetectCounts | all 0 | DecrementDisguiseDetectCount: `+0xAC + house*2` | HIGH |

### Object Occupancy (0xDC-0xEB)

| Offset | Size | Type | Name | Init | Evidence | Conf |
|--------|------|------|------|------|----------|------|
| 0xDC | 4 | uint | GapGenBitmask | 0 | BUILDING_SYSTEMS: `cell+0xDC |= (1 << player_index)` for gap gen | MED |
| 0xE0 | 4 | ptr | Jumpjet | 0 | Ghidra struct | MED |
| 0xE4 | 4 | ptr | FirstObject | 0 | Ghidra struct + AddContent (linked list head) | HIGH |
| 0xE8 | 4 | ptr | AltObject | 0 | Ghidra struct + AddContent (bridge layer list head) | HIGH |

`FirstObject` and `AltObject` are linked list heads. Objects link via offset +0x30 (NextObject).
Buildings (RTTI 6) are appended at tail; other objects are prepended at head.
`AltObject` is the bridge-layer list (used when `in_stack_00000008` is true in AddContent).

### Terrain & Radiation (0xEC-0xFB)

| Offset | Size | Type | Name | Init | Evidence | Conf |
|--------|------|------|------|------|----------|------|
| 0xEC | 4 | int | LandType | 0 (Clear) | Ghidra struct + RecalcAttributes, CheckCellPassability | HIGH |
| 0xF0 | 8 | double | RadLevel | 0.0 | Ghidra struct | MED |
| 0xF8 | 4 | ptr | RadSite | 0 | Ghidra struct | MED |

### Rendering & Z-Adjust (0xFC-0x115)

| Offset | Size | Type | Name | Init | Evidence | Conf |
|--------|------|------|------|------|----------|------|
| 0xFC | 4 | ptr | Unknown_0xFC | 0 | Constructor (freed in dtor, pointer) | LOW |
| 0x100 | 4 | int | Unknown_0x100 | 0 | Constructor | LOW |
| 0x104 | 4 | int | ZAdjust_Scale | 0x10000 | Constructor, Cell_ComputeZAdjust uses for scaling | MED |
| 0x108 | 2 | short | ZAdjust_Base | 0 | Constructor, Cell_ComputeZAdjust: `+0x108` | MED |
| 0x10A | 2 | short | ZAdjust_Ground | 1000 | Cell_ComputeZAdjust writes | HIGH |
| 0x10C | 2 | short | ZAdjust_GroundScaled | 1000 | Cell_ComputeZAdjust writes | HIGH |
| 0x10E | 2 | short | ZAdjust_Bridge | 1000 | Cell_ComputeZAdjust: height+4 variant | HIGH |
| 0x110 | 2 | short | ZAdjust_BridgeScaled | 1000 | Constructor only (corrected 2026-05-28: was "Cell_ComputeZAdjust" HIGH; binary shows Cell_ComputeZAdjust at 0x00484680 does NOT write 0x110 — only writes 0x10A/0x10C/0x10E; constructor inits to 1000 via `*(undefined2 *)(param_1 + 0x44)` — INFERENCE_HARDENED from parallel naming pattern) | LOW |
| 0x112 | 2 | short | ZAdjust_5 | 1000 | Constructor | LOW |
| 0x114 | 2 | short | ZAdjust_6 | 1000 | Constructor | LOW |

### Tube & Height (0x116-0x123)

| Offset | Size | Type | Name | Init | Evidence | Conf |
|--------|------|------|------|------|----------|------|
| 0x116 | 2 | short | TubeIndex | -1 (0xFFFF) | GetTubeAtCell, IsLowBridgeCell | HIGH |
| 0x118 | 1 | byte | Unknown_0x118 | 0xFF | Constructor | LOW |
| 0x119 | 1 | byte | Unknown_0x119 | 0 | Constructor | LOW |
| 0x11A | 1 | byte | Height | 0 | Ghidra struct + TMP raw height byte | HIGH |
| 0x11B | 1 | byte | Level | 0 | Ghidra struct + GetEffectiveHeight, CliffBackImpassability | HIGH |
| 0x11C | 1 | byte | SlopeIndex | 0 | Ghidra struct + RecalcAttributes, slope tilt | HIGH |
| 0x11D | 1 | byte | HeightInPixels | 0 | RecalcAttributes: `(height_calc / 15) % 15` | HIGH |
| 0x11E | 1 | byte | OverlayData | 0 | Reduce_Tiberium (ore amount), SetBridgeDirection (bridge frame) | HIGH |
| 0x11F | 1 | byte | Unknown_0x11F | 0 | Constructor | LOW |

### Shroud & State (0x120-0x13F)

| Offset | Size | Type | Name | Init | Evidence | Conf |
|--------|------|------|------|------|----------|------|
| 0x120 | 1 | byte | CachedShroudEdgeFrame | 0xFE | SHROUD_SYSTEM_COMPLETE, SHROUD_FOG_RENDERING | MED |
| 0x121 | 1 | byte | CachedFogEdgeFrame | 0xFE | SHROUD_FOG_RENDERING | MED |
| 0x122 | 1 | byte | OreNeighborCount | 0 | ORE_OVERLAY: decremented on adjacent ore removal | MED |
| 0x123 | 1 | byte | (padding) | — | — | — |
| 0x124 | 4 | uint | OccupationFlags | 0 | Ghidra struct + CheckCellPassability, PlaceInfantry. Bits 2-4=infantry subcells, bit 5=vehicle, bit 6=building | HIGH |
| 0x128 | 4 | uint | AltOccupationFlags | 0 | Ghidra struct + CheckCellPassability. Bridge-layer mirror of OccupationFlags | HIGH |
| 0x12C | 4 | uint | ShroudFlags | (low 5 bits cleared) | Only bits 3+4 used; bits 0,1,2,5..31 unobserved. Bit 3 (0x08)=explored, Bit 4 (0x10)=needs-redraw (dirty flag set by `Invalidate_Radius_For_Redraw`, cleared after repaint; was previously labeled "fully revealed"). See [MAPCLASS_COMPLETE_DECODE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_COMPLETE_DECODE.md) §E for the complete bit-map evidence | HIGH |
| 0x130 | 4 | int | GapConcealmentCounter | 1 | SHROUD_SYSTEM_COMPLETE: gap gen reference count. RevealShroudFlags checks > 0 | HIGH |
| 0x134 | 4 | int | GapConcealmentMax | 0 | SHROUD_SYSTEM_COMPLETE: gap counter cap | MED |
| 0x138 | 4 | int | NeedsRedrawFlag | 0 | SHROUD_DISPARITIES: per-cell dirty flag | MED |
| 0x13C | 4 | int | FogVisionCounter | 0 | OBJECT_FOG_VISIBILITY: IsFogged returns 1 when >= 1. Friendly vision refcount | HIGH |

### Flags (0x140-0x147)

| Offset | Size | Type | Name | Init | Evidence | Conf |
|--------|------|------|------|------|----------|------|
| 0x140 | 4 | uint | Flags | (low 23 bits cleared) | Ghidra struct + many funcs | HIGH |
| 0x144 | 4 | int | Unknown_0x144 | — | End of struct (328 bytes total) | LOW |

### Flags Bitmask (offset 0x140) — Complete

| Bit | Hex | Name | Evidence | Conf |
|-----|-----|------|----------|------|
| 0 | 0x0001 | FogInterior | SHROUD_FOG_RENDERING (TS legacy fog mode) | MED |
| 1 | 0x0002 | CurrentlyVisible | SHROUD_FOG_RENDERING: fog mode visibility | MED |
| 5 | 0x0020 | GapOverlayActive | SHROUD_SYSTEM_COMPLETE: gap gen active on cell | HIGH |
| 6 | 0x0040 | FogProcessingFlag | SHROUD_FOG_RENDERING: cleared during fog pass | MED |
| 7 | 0x0080 | HasBridgeOverlay | GetEffectiveHeight: adds +4 to height. SetBridgeDirection | HIGH |
| 8 | 0x0100 | BridgeStructuralCell | CheckCellPassability, IsOnBridge, PATHFINDING_ASTAR, many | HIGH |
| 9 | 0x0200 | Bridgehead | SetBridgeDirection_NESW: entry/exit point | HIGH |
| 10 | 0x0400 | BridgeRail | SetBridgeDirection_NESW. NAVAL_SYSTEM: ramp cells | MED |
| 11 | 0x0800 | BridgeOrientation | SetBridgeDirection: 0=N-S, 1=E-W | HIGH |
| 12 | 0x1000 | BridgeDirectionBit | SetBridgeDirection_NESW | MED |
| 13 | 0x2000 | BridgePavement | BRIDGE_SYSTEM: sub-tile variant selector | MED |
| 16 | 0x10000 | TallTileNeighbor | RecalcAttributes: shadow caster neighbor marker | HIGH |
| 17 | 0x20000 | HasTileAnimation | RecalcAttributes: tile anim was created | HIGH |
| 18 | 0x40000 | AlteredPassability | BRIDGE_SYSTEM: XOR-toggled, cost x4 in pathfinder | HIGH |
| 20 | 0x100000 | BridgeZone_NS | BRIDGE_SYSTEM: MapClass::PopulateZones | MED |
| 21 | 0x200000 | BridgeZone_EW | BRIDGE_SYSTEM: MapClass::PopulateZones | MED |
| 22 | 0x400000 | FogRenderFlag | SHROUD_FOG_RENDERING: gap gen fog render pass | MED |

---

## 3. RecalcAttributes Pipeline

**Function:** `CellClass::RecalcAttributes` at `0x0047d2b0`
**Size:** 273 decompiled lines, 0x0AB3 bytes
**Called by:** Map initialization, overlay changes, bridge destruction, terrain modifications

### Overview

RecalcAttributes recomputes the cell's LandType, SlopeIndex, ZoneType, and associated data
after any change to the cell's terrain, overlay, or height. It is the central "truth setter"
for cell passability.

### Pipeline stages

```
Stage 1: Validate cell (skip if dummy cell at DAT_00abdc50)
Stage 2: Cache zone map pointers for this cell
Stage 3: Branch based on overlay presence
  Stage 3a: If overlay exists → compute LandType from overlay properties
    - If overlay is Wall/Railroad/Tiberium → read SlopeIndex from TMP
    - If on slope AND overlay disallows slopes → remove overlay
    - If CliffBackImpassability != 0 → check 6 neighbors for cliff (height diff >= 4)
      → If behind cliff AND CliffBackImpassability == 2 → LandType = Rock (3)
    - Update zone, write zone map data, RETURN
  Stage 3b: If no overlay or overlay is transparent
    Stage 3b.1: If IsoTileTypeIndex is invalid → set defaults, apply CliffBackImpassability, RETURN
    Stage 3b.2: If valid tile:
      - Read SlopeIndex from TMP data
      - Compute LandType from TMP terrain byte (via FUN_00544be0)
      - Handle overlay on slope (remove if slope >= 5)
      - Handle LandType=10 (Tunnel): create TubeClass if tile is tunnel entrance
      - Set Level from TMP height
      - Compute HeightInPixels (field 0x11D): (height_raw - 30) / 15
      - Create tile animation if tile has one (flag 0x20000)
      - Set SpecialTerrainMarker flag (0x10000) on shadow caster tile neighbors
      - Apply CliffBackImpassability (same 6-neighbor check)
Stage 4: Call RecalcZoneType
Stage 5: Write Level and ZoneType to zone map cache
```

### CliffBackImpassability check (appears 3 times)

```
if RulesClass[0x664] != 0:                    // CliffBackImpassability enabled
    for neighbor in 6 isometric neighbors:     // (Y-1,X), (Y,X-1), (Y+2,X+2), (Y+1,X+1), (Y+1,X-1), (Y-1,X+1)
        if neighbor.Level >= cell.Level + 4:
            is_behind_cliff = true
            break
    if is_behind_cliff AND RulesClass[0x664] == 2:
        if cell.LandType in {Clear(0), Water(2), Beach(6), Ice(8)}:
            cell.LandType = Rock(3)            // IMPASSABLE
```

---

## 4. RecalcZoneType

**Function:** `CellClass::RecalcZoneType` at `0x00483c80`
**Writes to:** `field_0x4c` (ZoneType)

### ZoneType values

| Value | Name | Condition |
|-------|------|-----------|
| 0 | Ground | Default — passable ground |
| 1 | Road | Overlay with `+0x22D` flag (crate/road overlay) |
| 2 | Wall | Overlay with `+0x2A8` flag (IsWall) |
| 3 | Beach | LandType == 6 |
| 4 | Water | LandType == 2 |
| 5 | Building | Cell contains BuildingClass with specific conditions |
| 6 | Impassable | Speed table entry == 0.0 (impassable terrain), or wall/gate overlay |
| 7 | OutOfBounds | Cell not in playfield |

### Algorithm

```
if not in playfield → ZoneType = 7 (OOB), return

if overlay exists:
    if overlay.IsCrate (0x22D) → ZoneType = 1 (Road)
    if overlay.IsWall (0x2A8) → ZoneType = 2 (Wall)
    if speed_table[overlay.LandType * 9 + 0] == 0.0 → ZoneType = 6 (Impassable)
    if overlay.IsGate (0x2B5) → ZoneType = 6 (Impassable)
    if overlay.IsVeinholeMonster (0x2B4) → fall through to ZoneType = 0

if LandType == 2 → ZoneType = 4 (Water)
if LandType == 6 → ZoneType = 3 (Beach)
if speed_table[LandType * 9 + 0] <= 0.01 → ZoneType = 6 (Impassable)  // threshold at 0x7E3808 = 0.01

for each object in FirstObject linked list:
    if BuildingClass:
        check naval yard conditions → ZoneType = 5 or 2
    if TerrainClass (0x24):
        check passability → ZoneType = 5 (Building)

default → ZoneType = 0 (Ground)
```

---

## 5. Key Methods Summary

| Method | Address | Purpose |
|--------|---------|---------|
| Constructor | 0x0047bbf0 | Initialize all fields to defaults |
| Destructor | 0x0047bb60 | Free pointers at 0x28 and 0xFC |
| RecalcAttributes | 0x0047d2b0 | Recompute LandType, slope, zone, height |
| RecalcZoneType | 0x00483c80 | Recompute ZoneType from LandType + overlays + objects |
| GetEffectiveHeight | 0x00487d50 | `Level + (Flags & 0x80 ? 4 : 0)` |
| CheckCellPassability | 0x004834a0 | Full passability check (zone, height, occupation, speed table) |
| Can_Enter_Cell_General | 0x00481a00 | Unit entry check |
| AddContent | 0x0047e8a0 | Add object to cell linked list |
| RemoveContent | 0x0047ea90 | Remove object from cell linked list |
| IsVisibleToHouse | 0x004870b0 | `VisibleToHouses & bit(house_id)` |
| IncrementSensorCount | 0x00487150 | `SensorCounts[house] += 1` |
| DecrementSensorCount | 0x00487160 | `SensorCounts[house] -= 1` |
| IncrementDisguiseDetect | 0x00487170 | `DisguiseDetectCounts[house] += 1` |
| DecrementDisguiseDetect | 0x00487180 | `DisguiseDetectCounts[house] -= 1` |
| PlaceInfantryInCell | 0x00481180 | Sub-cell placement for infantry |
| GetSubCell | 0x004810a0 | Compute sub-cell index from coordinates |
| IsBridge | 0x00486750 | `IsoTileTypeIndex in [BridgeSet, BridgeSet+16)` |
| IsWoodBridge | 0x00486770 | `IsoTileTypeIndex in [WoodBridgeSet, ...]` |
| IsLowBridgeCell | 0x00484ab0 | `TubeIndex >= 0 AND LandType == 10` |
| IsClearTile | 0x00486380 | `IsoTileTypeIndex == 0xFFFF or 0` |
| IsShorePieceTile | 0x004865b0 | `IsoTileTypeIndex in [ShorePieces, ShorePieces+42)` |
| HasBridgeOverlay | 0x004865d0 | Checks ShorePieces + waterfall tile set ranges |
| IsOnBridgeSurface | 0x00485060 | `IsoTileTypeIndex in [WaterSet, WaterSet+14)` |
| GetTubeAtCell | 0x00484f20 | Lookup TubeClass from TubeIndex |
| Get_Tiberium_Value | 0x00485020 | `overlay.value * (OverlayData + 1)` |
| Reduce_Tiberium | 0x00480a80 | Reduce ore amount, remove overlay if depleted |
| Scatter_Objects | 0x00481670 | Force objects in cell to scatter |
| BlowUpBridge | 0x0047dd70 | Bridge destruction sequence |
| RevealShroudFlags | 0x004876f0 | Set shroud bits in ShroudRevealFlags |
| GetRadarColor | 0x0047c060 | Compute minimap pixel color |
| Cell_ComputeZAdjust | 0x00484680 | Compute Z-adjust values for rendering |

---

## 6. GetEffectiveHeight

**Function:** `0x00487d50` (3 lines)

```c
int GetEffectiveHeight(CellClass *cell) {
    return (int)(signed char)cell->Level + ((cell->Flags >> 7) & 1) * 4;
}
```

Returns `Level` plus 4 if the bridge overlay flag (bit 7 = 0x80) is set. This is the
height used by all gameplay systems (pathfinding, bullet collision, cliff detection).

---

## 7. CheckCellPassability

**Function:** `0x004834a0`

### Parameters (stack-passed)
- `param_1` = this (CellClass)
- `in_stack_00000004` = SpeedType (4 = aircraft bypasses all checks)
- `in_stack_00000008` = IgnoreInfantry flag
- `in_stack_0000000c` = IgnoreVehicle flag
- `in_stack_00000010` = RequiredZoneID (-1 = any)
- `in_stack_00000014` = MovementZone
- `in_stack_00000018` = RequiredHeight (-1 = any)
- `in_stack_0000001c` = OnBridge flag

### Algorithm

```
if SpeedType == 4 (aircraft): return PASSABLE

if RequiredZoneID != -1:
    cellZone = MapClass::GetZoneID(cell.coords, MovementZone, OnBridge)
    if cellZone != RequiredZoneID: return BLOCKED

if RequiredHeight != -1:
    if RequiredHeight == cell.Level:
        if (Flags & 0x100) AND NOT OnBridge: return BLOCKED  // bridge cell but not on bridge
    else:
        if NOT (Flags & 0x100) OR RequiredHeight != cell.Level + 4: return BLOCKED

// Determine which occupation layer to check
if (RequiredHeight == cell.Level + 4) AND (Flags & 0x100):
    use AltOccupationFlags (bridge layer)
else:
    use OccupationFlags (ground layer)

// Apply infantry/vehicle filters
if IgnoreInfantry: mask &= 0xE0 (clear infantry sub-cell bits)
if IgnoreVehicle: mask &= 0x5F (clear vehicle bit)

if masked_flags != 0: return BLOCKED  // cell occupied

// Speed table check
landType = cell.LandType
if overlay.IsWall AND MovementZone can crush walls: landType = 0 (treat as clear)
if speed_table[SpeedType + landType * 9] == 0.0 AND NOT on bridge: return BLOCKED

return PASSABLE
```

---

## 8. Open Questions (Remaining)

1. **Offset 0x28 (CellTag):** CELL_OCCUPATION says CellTag pointer. OBJECT_FOG_VISIBILITY says
   FoggedObjectClass. HOUSECLASS says occupant list. Likely multi-purpose — CellTag normally,
   FoggedObject list when fog-of-war enabled (TS legacy). **Confidence: LOW**

2. **Offset 0x34 (LightConvert):** CELL_OCCUPATION says LightConvert (freed in dtor).
   ORE_OVERLAY says "IsoTile pointer". Need disambiguation. **Confidence: LOW**

3. **Offsets 0x40, 0x50, 0x60:** Still unknown. 0x50 and 0x60 init to -1. **Confidence: LOW**

4. **Offset 0xFC (pointer, freed in dtor):** Unknown purpose. **Confidence: LOW**

5. **Offset 0x11A:** Ghidra struct says "Height" (raw TMP byte). BRIDGE_SYSTEM says
   "bridge sub-type / sub-tile index". BUILDING_SYSTEMS says "sub-position byte". May be
   repurposed depending on what occupies the cell. **Confidence: LOW**

6. **+0x4C vs +0xEC — two separate classification systems:**
   - 0x4C = ZoneType (8 values: Ground/Road/Wall/Beach/Water/Building/Impassable/OOB)
   - 0xEC = LandType (12 values: Clear/Road/Water/Rock/Wall/Tiberium/Beach/Rough/Ice/Railroad/Tunnel/Weeds)
   These are NOT the same field. ZoneType is a reduced passability classification derived from
   LandType + overlays + objects. TERRAIN_COST_FACTSHEET confirms both exist. **Confidence: HIGH**

---

## 8a. Cross-Report Conflicts Resolved

| Offset | Conflict | Resolution |
|--------|----------|------------|
| 0x2C | BRIDGE_SYSTEM (report 066) wrongly said bridge anchor at +0x3C | Corrected: bridge anchor is at +0x2C. +0x3C is AttachedTag |
| 0x4C vs 0xEC | Multiple reports confused LandType and ZoneType | +0x4C = ZoneType (8-val), +0xEC = LandType (12-val). Different systems |
| 0x10C/0x10E | CELL_OCCUPATION said PassabilityRate | Actually Z-adjust values (Cell_ComputeZAdjust). Not passability |
| 0xDC | FIND_NEARBY said OccupationFlags | Wrong — real OccupationFlags at +0x124. This is GapGenBitmask |
| 0x130 | CELL_OCCUPATION said SensorTotalCount | Actually GapConcealmentCounter (SHROUD_SYSTEM_COMPLETE, more recent) |
| 0x11C | Early BRIDGE_SYSTEM said bridge flag | Corrected: SlopeIndex (0-20). NOT a bridge flag |

---

## 9. Cross-Reference to Existing Reports

These existing reports contain verified CellClass field information:

| Report | CellClass fields covered |
|--------|------------------------|
| [TERRAIN_COST_FACTSHEET.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TERRAIN_COST_FACTSHEET.md) | LandType enum (0xEC), speed table, RecalcLandType |
| BRIDGE_SYSTEM.md | Flags bits (0x80, 0x100, 0x200, 0x800, 0x40000), height +4 |
| [ZONE_PASSABILITY_VERIFIED.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ZONE_PASSABILITY_VERIFIED.md) | ZoneType (0x4C), passability matrix |
| [COORDINATE_ATOMS_AUDIT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/COORDINATE_ATOMS_AUDIT.md) | Level (0x11B), height-to-pixel conversion |
| VOXEL_SLOPE_TILT_SYSTEM.md | SlopeIndex (0x11C), slope types 0-20 |
| [CLIFF_OBJECTS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/1dbaf80c89c9348df493ab618dbefed8663aef96/docs/research/CLIFF_OBJECTS_GHIDRA_REPORT.md) | CliffBackImpassability in RecalcAttributes, GetEffectiveHeight |
| [CELL_OCCUPATION_MARKING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELL_OCCUPATION_MARKING_GHIDRA_REPORT.md) | OccupationFlags (0x124), AltOccupationFlags (0x128) |
| [SHROUD_SYSTEM_COMPLETE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHROUD_SYSTEM_COMPLETE.md) | Shroud-related fields, VisibleToHouses (0x78) |
| [INFANTRY_SUBCELL_POSITIONING.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/INFANTRY_SUBCELL_POSITIONING.md) | Sub-cell occupation bits in OccupationFlags |
| [ORE_OVERLAY_SYSTEM_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ORE_OVERLAY_SYSTEM_GHIDRA_REPORT.md) | OverlayData (0x11E), Reduce_Tiberium |
| [UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md) | Height diff thresholds, bridge height checks |
| [PROCESS_DRIVE_TRACK_DECOMPILATION.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PROCESS_DRIVE_TRACK_DECOMPILATION.md) | Bridge height transitions |

---

## Sources

### Ghidra addresses decompiled
- `0x0047bbf0` — CellClass::Constructor (field initialization)
- `0x0047bb60` — CellClass::Constructor (destructor variant)
- `0x0047d2b0` — CellClass::RecalcAttributes (full, 273 lines)
- `0x00483c80` — CellClass::RecalcZoneType
- `0x004834a0` — CellClass::CheckCellPassability
- `0x0047e8a0` — CellClass::AddContent
- `0x00487d50` — CellClass::GetEffectiveHeight
- `0x004870b0` — CellClass::IsVisibleToHouse
- `0x00487150` — CellClass::IncrementSensorCount
- `0x00487180` — CellClass::DecrementDisguiseDetectCount
- `0x004870d0` — CellClass::SensorCountForHouse
- `0x004810a0` — CellClass::GetSubCell
- `0x00486750` — CellClass::IsBridge
- `0x00484ab0` — CellClass::IsLowBridgeCell
- `0x00485060` — CellClass::IsOnBridgeSurface
- `0x00486380` — CellClass::IsClearTile
- `0x004865b0` — CellClass::IsShorePieceTile
- `0x004865d0` — CellClass::HasBridgeOverlay
- `0x00484f20` — CellClass::GetTubeAtCell
- `0x00485020` — CellClass::Get_Tiberium_Value
- `0x00480a80` — CellClass::Reduce_Tiberium
- `0x004876f0` — CellClass::RevealShroudFlags
- `0x00484680` — Cell_ComputeZAdjust
- `0x0047e040` — CellClass::SetBridgeDirection_NESW

## Tier 2 application record (2026-08-17, Claude Code session)

Applied to the live /RA2/CellClass struct (328 B, size unchanged) after per-field
re-verification against live decompiles this session. Snapshot before mutations:
<local>/Documents/ghidra-backups/2026-08-17-tier2 (17 files, 243,261,449 bytes, verified).

Fields added (tool auto-prefixes names; offsets/types per this doc's table):
nZoneType 0x4C (RecalcZoneType writes 0-7); dwVisibleToHouses 0x78 (IsVisibleToHouse
bit test); aSensorCounts short[24] 0x7C (IncrementSensorCount +0x7c+house*2);
aDisguiseDetectCounts short[24] 0xAC; nZAdjust_Scale 0x104 / nZAdjust_Base 0x108 /
nZAdjust_Ground 0x10A / nZAdjust_GroundScaled 0x10C / nZAdjust_Bridge 0x10E
(Cell_ComputeZAdjust 0x00484680: writes 0x10A/0x10E, scales via 0x104 >>16 into
0x10C/0x10E, clamps 0..2000); nTubeIndex 0x116 (GetTubeAtCell bounds vs g_TubeCount);
bOverlayData 0x11E (Reduce_Tiberium ore density byte); dwShroudFlags 0x12C
(RevealShroudFlags |= 0x18); nGapConcealmentCounter 0x130 (>0 gates Flags 0x20);
nFogVisionCounter 0x13C (IsFogged returns <1 -> 0).

Receivers typed CellClass* __thiscall (9 new; Reduce_Tiberium and RecalcZoneType were
already typed): IncrementSensorCount, DecrementSensorCount, IncrementDisguiseDetectCount,
DecrementDisguiseDetectCount, SensorCountForHouse bool(int), IsVisibleToHouse bool(byte),
GetTubeAtCell void*(), RevealShroudFlags void(), RecalcAttributes — CORRECTED by the 2026-08-17 independent
critic pass: void __thiscall(CellClass* this, int levelOverride), NOT void(). RET 0x4
at 0047dd61; body gates `this->Level = (byte)arg` on arg != -1; caller 00480bca pushes
-1. The original void() claim came from trusting a rendered callsite instead of the
RET immediate. Critic pass verdict on all tier-2 rows: 21/22 CONFIRMED from raw bytes,
this row refuted and fixed same day.

Residuals:
- 0x11A CONFLICT unresolved: this doc says Height (HIGH); live DTM has bIsoSubTileIndex.
  Neither decompile read this session touched 0x11A. Left as-is; do not cite either
  name until resolved from RecalcAttributes/TMP loader evidence.
- 0x2C BridgeAnchorPtr and 0x11D HeightInPixels: doc-HIGH but not self-verified this
  session; left as holes.
- ComputeGroundHeightAtCoord and CheckCellPassability receivers deferred (argument
  semantics not proven this session; plate comment on the former says CellClass* ECX
  plus a coord-pointer arg whose storage was not pinned).
- ~66 labeled CellClass functions remain untyped; type on contact (land-as-you-go),
  identifying statics (InitSubCellOffsets, *_AtMapCoord) before assuming __thiscall.
- MCP add/modify_struct_field auto-applies Hungarian prefixes and silently ignores
  rename-to-unprefixed; struct now carries mixed naming convention. Cosmetic.
