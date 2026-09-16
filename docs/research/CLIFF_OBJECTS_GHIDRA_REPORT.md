# Cliff Objects — Ghidra Research Report

**Primary addresses:** Tile loading `0x00545714`, IsCliffTile `0x004863d0`, Foot Z `0x004DAFC0`, RecalcAttributes `0x0047d2b0`
**Revision scope, 2026-09-08:** Foot rendering only. Section 8 and the associated
defaults, consumers and implementation notes were rechecked against retail
`gamemd.exe` assembly and executable fixtures. Sections 2–7 and 9, and their
non-rendering summary claims, remain historical research, **not revalidated in
this revision**. Their old labels and implementation prescriptions are not proof
of current Rust behavior or active-YR reachability.
**Active in YR:** `ZFudgeCliff` is active with constructor default **10**. The
previous zero-default/dormancy conclusion was wrong; see Sections 8 and 14.

## 1. Overview

Cliff objects in gamemd.exe are **terrain tile sets** defined in theater INI files, not standalone
game objects. The engine uses numeric tile set index ranges (not string matching) to classify
tiles as cliffs. Cliff systems span 7 interconnected subsystems: tile registration, tile
classification, variant randomization, neighbor fixup, destroyable cliff destruction, bullet
cliff blocking, rendering Z-fudge, and cliff-back impassability.

Absence of an INI override does not imply a zero value. The TechnoType constructor
sets `ZFudgeCliff=10`, retained by Unit and Infantry types. Ordinary unit draws
consume it through Foot Z. This correction does not establish or change the
separate historical conclusions about destroyable cliffs.

The bounded implementation and validation scope is in the
[cliff depth plan](../plans/2026-09-08-cliff-unit-depth-plan.md). Native fixtures
come from [render_depth_oracle.py](../../tools/render_depth_oracle.py) and
[render_depth_vectors.json](../../tools/render_depth_vectors.json).

---

## 2. Tile Set Registration (Theater INI)

**Function:** Tile loading at `0x00545714` (mislabeled `CDFileClass__Constructor`)

The tile loader reads cliff-related tile set indices from the **theater-specific INI** file
(e.g., `temperatemd.ini`) `[General]` section. These are tile set indices, not tile IDs — they
get resolved to actual tile IDs during the tile loading loop.

| INI Key | Global Address | Range (tiles) | Default |
|---------|---------------|---------------|---------|
| `CliffSet` | `0x00aa1020` | `[CliffSet, CliffSet+40)` | -1 |
| `CliffRamps` | `0x00abbebc` | `[CliffRamps, CliffRamps+20)` | -1 |
| `WaterCliffs` | `0x00aa101c` | `[WaterCliffs, WaterCliffs+28)` | -1 |
| `DestroyableCliffs` | `0x00abc2c8` | `[DestroyableCliffs, DestroyableCliffs+2)` | -2 |

All four are read via `CCINIClass::ReadInt` with default -1 (or -2 for DestroyableCliffs).
A value of -1 means "this tileset doesn't exist in this theater."

### Related tile set globals (for context)

| INI Key | Global | Range | Notes |
|---------|--------|-------|-------|
| `BridgeSet` | `0x00aa0e28` | +16 tiles | Used in IsCliffTile checks |
| `WoodBridgeSet` | `0x00abad1c` | +16 tiles | Used in IsCliffTile checks |
| `WaterCaves` | `0x00abad24` | +4 tiles | Used in IsCliffTile checks |
| `WaterfallEast` | `0x00aa073c` | +4 tiles | Directional passability exceptions |
| `WaterfallWest` | `0x00abb110` | +4 tiles | Directional passability exceptions |
| `WaterfallNorth` | `0x00aa10a0` | +4 tiles | Directional passability exceptions |
| `WaterfallSouth` | `0x00aa1050` | +4 tiles | Directional passability exceptions |
| `SlopeSetPieces` | `0x00abc1f8` | — | Used as replacement when cliffs are destroyed |
| `HeightBase` | `0x00aa0744` | — | Base tile set for height transitions |

---

## 3. Tile Classification — IsCliffOrImpassableTile

**Function:** `FUN_004863d0`
**Signature:** `bool IsCliffOrImpassableTile(IsometricTileTypeClass *tile)`
**Key fields:** `tile+0x38` = tile set index, `tile+0x11a` = sub-tile slope direction

Returns **true** if the tile belongs to any of these impassable tile set ranges:

```
CliffSet         [CliffSet,         CliffSet + 0x28)        — 40 tiles
CliffRamps       [CliffRamps,       CliffRamps + 0x14)      — 20 tiles
WaterCliffs      [WaterCliffs,      WaterCliffs + 0x1C)     — 28 tiles
DestroyableCliffs[DestroyableCliffs, DestroyableCliffs + 2) — 2 tiles
BridgeSet        [BridgeSet,        BridgeSet + 0x10)       — 16 tiles
WoodBridgeSet    [WoodBridgeSet,    WoodBridgeSet + 0x10)   — 16 tiles
WaterCaves       [WaterCaves,       WaterCaves + 4)         — 4 tiles
WaterfallEast    [WaterfallEast,    WaterfallEast + 4)      — 4 tiles (with exceptions)
WaterfallWest    [WaterfallWest,    WaterfallWest + 4)      — 4 tiles (with exceptions)
WaterfallNorth   [WaterfallNorth,   WaterfallNorth + 4)     — 4 tiles (with exceptions)
WaterfallSouth   [WaterfallSouth,   WaterfallSouth + 4)     — 4 tiles (with exceptions)
```

### Waterfall slope-direction exceptions

Waterfall tiles have **directional passability** based on the sub-tile slope (`tile+0x11a`):

| Waterfall | Passable when slope direction is... |
|-----------|-------------------------------------|
| East | 0 (flat) or 4 (south) |
| West | 1 (west) or 3 (east) |
| North | 2 (north) or 3 (east) |
| South | 0 (flat) or 1 (west) |

Only the first and last tiles of each waterfall set (index 0 and 3) can be passable based
on slope. Middle tiles (indices 1-2) are always impassable.

### IsOnBridgeRamp — `0x00578d80`

Same logic as IsCliffOrImpassableTile but only checks: CliffSet (40), CliffRamps (20), and
the 4 waterfall directions (with slope exceptions). Does NOT check bridges, WaterCliffs,
DestroyableCliffs, or WaterCaves. Used to determine height transition ramps for movement.

---

## 4. Cliff Variant Randomizer

**Function:** `FUN_005a1350`
**Purpose:** Visual randomization so cliff faces don't look repetitive.

Takes a tile set index, subtracts `CliffSet`, and returns a randomized variant within the
same visual group. The 40 cliff tiles are organized into groups:

```
Offset from CliffSet → Behavior
4-6:     Group A — 3 variants, random(0-2) + 4
5:       Pair select from Group A — random(0-1)*2 + 4 → {4, 6}
6:       Pair select from Group A — random(0-1) + 4 → {4, 5}
8-10:    Group B — same pattern as A, base offset 8
11-13:   Group C — same pattern, base offset 11
14-16:   Group D — same pattern, base offset 14
22-24:   Group E — same pattern, base offset 22
28-29:   Mirror swap — 28→29, 29→28
34-36:   Group F — random selection
Default: Returns input unchanged (not a randomizable cliff)
```

Uses `Random::Next()` with `Math::ftol()` for variant selection.

---

## 5. Cliff Neighbor Fixup

**Function:** `FUN_005a17f0`
**Purpose:** Ensures adjacent cliff tiles have matching visual edges after randomization.

Algorithm:
1. Iterate all tiles in the current view
2. For each tile in `[CliffSet, CliffSet+0x28)`:
   - Check neighbor at direction offset 4 (SE) and direction offset 2 (NE)
   - If neighbor has the same tile set index but lower sub-tile slope value
   - Call the variant randomizer (`FUN_005a1350`) on the current tile
   - If randomization changes the tile, update the adjacent cell's rendered tile

---

## 6. Destroyable Cliff Destruction

**Function:** `FUN_00581140`
**Check function:** `FUN_00486900` — returns true if `tile_set_index == DestroyableCliffs` or
`tile_set_index == DestroyableCliffs + 1`

### DestroyableCliffs tile 0 (6-wide layout)

```
sub-tile layout: 6 columns × N rows
column = sub_tile_index % 6
row    = sub_tile_index / 6  (integer division, using multiply-by-magic-number trick: *-0x2AAAAAAB)
origin_x = tile_x - column
origin_y = tile_y + row
```

### DestroyableCliffs tile 1 (4-wide layout)

```
sub-tile layout: 4 columns × N rows
column = sub_tile_index & 3  (bitwise AND)
row    = sub_tile_index >> 2  (right shift)
origin_x = tile_x - column
origin_y = tile_y - row
```

### Destruction sequence

1. Create a new overlay object from the same tile template
2. Place it at the computed origin coordinates
3. Set tile to non-blocking: clear `0x81` flag, set `0x74` to destroyed mode
4. Call virtual method `0x124` (Destroy/Remove from map)
5. Free the tile object
6. Replace with **SlopeSetPieces** tiles (`DAT_00abc1f8` and `DAT_00abc1f8+1`)
7. For each cell in the destroyed cliff's footprint:
   - Reset zone connectivity (`zone_data[idx] = 0`)
   - Call `CellClass::RecalcAttributes` to update passability
   - Call `FUN_00584550` (rebuild zone connections) for cells with zero zone
   - Stop all targeting on affected cells
   - Mark radar minimap dirty
8. Create destruction animations:
   - For each cell in a 5×3 grid around the origin:
     - Spawn 2 random animations from 3 possible anim types
     - Random X offset: [-8, +8], random Y offset: [-12, +12]
     - Random animation start frame: [0, 2]
     - Duration flag: `0x600` (1536 frames)
9. Dirty the screen rectangle (with 0x78/0x3C pixel padding)
10. Call `MapClass::UpdateBridgeZonesHelper` to recalculate bridge zones

---

## 7. Bullet Cliff Blocking — SubjectToCliffs

**BulletTypeClass field:** offset `0x296` (bool)
**Read in:** `BulletTypeClass::ReadINI` at `0x0046bfeb`
**Used in:** `FUN_004CC360` (bullet cliff/wall collision check)

### Collision check algorithm (FUN_004CC360)

```
function BulletCliffWallCheck(bullet_pos, bullet_type, source_cell, dest_cell):
    mid_cell = GetCellAt(bullet_pos / 256)

    // --- Cliff check ---
    if bullet_type.SubjectToCliffs:
        src_height = source_cell.GetEffectiveHeight()
        mid_height = mid_cell.GetEffectiveHeight()
        if mid_height - src_height > 3:               // uphill cliff face
            dst_height = dest_cell.GetEffectiveHeight()
            if dst_height != mid_height AND dst_height - mid_height >= 0:
                return mid_cell  // BLOCKED by cliff

    // --- Wall check ---
    if bullet_type.SubjectToWalls:
        if mid_cell has wall overlay with IsWall flag:
            if mid_cell != dest_cell:                  // don't block at target
                if dest_cell.height >= source_cell.height:  // not firing downhill over wall
                    if Rules.AlliedWallTransparency AND wall owner is allied:
                        return NULL  // allied walls are transparent
                    return mid_cell  // BLOCKED by wall

    return NULL  // not blocked
```

**Key constants:**
- Height difference threshold for cliff blocking: **> 3 levels** (i.e., ≥ 4)
- `CellClass::GetEffectiveHeight()` = `cell.height_0x11B + (cell.flags_0x140 >> 7 & 1) * 4`
  (adds +4 if bridge overlay flag `0x80` is set)

### Related BulletTypeClass fields

| Offset | Type | INI Key | Purpose |
|--------|------|---------|---------|
| 0x295 | bool | `Floater` | Floats in air |
| 0x296 | bool | `SubjectToCliffs` | Blocked by cliff faces |
| 0x297 | bool | `SubjectToElevation` | Range bonus from height |
| 0x298 | bool | `SubjectToWalls` | Blocked by wall overlays |
| 0x299 | bool | `VeryHigh` | Very high trajectory |
| 0x29A | bool | `Shadow` | Casts shadow |
| 0x29B | bool | `Arcing` | Arcing trajectory |

---

## 8. Rendering Z-Fudge System

**Owner:** `FootClass::GetZAdjustment`, real entry `0x004DAFC0`, tail
`0x004DAFF0..0x004DB09E`. Ghidra's `004DAFF0` boundary begins inside the
locomotor virtual call and omits the entry's null-locomotor handling.
The result adjusts the unit's per-pixel Z comparison against terrain/buildings;
it is separate from painter sorting and lighting.

### TechnoTypeClass ZFudge fields

| Offset | Field | INI Key | Default | Purpose |
|--------|-------|---------|---------|---------|
| 0xDC0 | `[0x370]` | `ZFudgeCliff` | 10 | Z bias selected by signed neighboring levels |
| 0xDC4 | `[0x371]` | `ZFudgeColumn` | 5 | Z bias near bridge column tiles |
| 0xDC8 | `[0x372]` | `ZFudgeTunnel` | 10 | Z bias selected by tube/low-bridge cells |
| 0xDCC | `[0x373]` | `ZFudgeBridge` | 0 | Z bias near a bridge while not OnBridge |

Constructor `0x710AF0` writes these at `0x711664..0x71167A`:
EDI=10, literal 5, EDI=10, EBX=0. UnitType `0x7470D0` and InfantryType
`0x5236A0` call that constructor without replacing these values. Their INI
readers call the base reader; `0x71541C..0x715437` passes the existing `+DC0`
value as the `ZFudgeCliff` fallback. Standard `rulesmd.ini` has no assignment
overriding that default. Configured sibling values are overrides, not constructor
defaults. Earlier tables claiming all four default to zero were incorrect.

### Composition and arithmetic

```
locomotor_z = Foot.Locomotor ? Foot.Locomotor.vtable[0x38]() : 0
column = wrap_i32(type.ZFudgeColumn * ColumnScore())
tunnel = wrap_i32(type.ZFudgeTunnel * TunnelScore())
cliff  = wrap_i32(type.ZFudgeCliff  * CliffScore())
bridge = NearBridge() ? type.ZFudgeBridge : 0
result = wrap_i32(AdditionalZ() + signed_max(column, tunnel, cliff, bridge)
                  + locomotor_z)
```

Native calls the selectors in the displayed order, then `AdditionalZ`. The
`IMUL` results wrap to i32 before signed `JG` maximum comparisons at
`0x4DB076..0x4DB088`; final additions wrap too. There is no extra zero clamp
besides the inactive bridge candidate. The locomotor virtual belongs to the
ILocomotion interface, not Techno's vtable. Drive `0x4B4870`, Ship `0x6A3EA0`,
and base `0x55ABA0` inherited by Walk/Hover/Fly/Jumpjet/Teleport return zero.

Type lookup is Foot's virtual `+84`. For Unit it resolves through `6F3270` and
`741490` to the current `Unit+6C4`. The temporary Harvester UnloadingClass swap
at `73D2C4` applies before both SHP and VXL body dispatch and is restored at
`73D3A2`; the depth lookup therefore sees that swapped type. Disguise and
NoSpawnAlt selection of a separate drawing type do not replace this field.

### Cell lookup and `CliffScore` — `0x00704240`

Object virtual `+1B8` (`0x41BEA0`) converts raw Location.X/Y using signed
division by 256 truncated toward zero, then narrows each coordinate to i16.
`Map::Get_CellClass` (`0x5657A0`) uses `y*512+x`, with only a combined index
range check and nullable slot check. Consequently requests can alias another
cell's canonical coordinate. Failure returns the shared dummy `ABDC50` and
stamps its coordinate `+24`; it does not return null. Neighbor addition wraps
each i16 component.

```
current = GetCell(raw_object_cell)
if Object.OnBridge: return 0                   // byte +8C, not IsInAir
first = GetCell(raw_object_cell + (1,1))
score = signed_i8(first.Level) - signed_i8(current.Level) >= 4 ? 2 : 0
second = GetCell(first.stored_coord + (1,1))
if signed_i8(second.Level) - signed_i8(current.Level) >= 4: score = 1
return score
```

The second test is independent and overrides the first. Earlier pseudocode
incorrectly nested it under the first height test and used an airborne gate.
Runtime initializer `0x49F34A` sets `89F694` to `(1,1)`; describing it as NW
was also wrong. The first request starts from the raw object cell, whereas the
second starts from the first resolved cell's stored coordinate. No tile-name
or cliff-classification predicate participates.

### Bridge and column selectors

`NearBridge` (`0x703B10`) resolves current before testing `OnBridge`; when
OnBridge is true it returns false. Otherwise it resolves S,N,E,W, then tests
current flag `0x100`, S/N `0x100` with `0x800` set, or E/W `0x100` with `0x800`
clear. `0x703CC0` has the same structure with mask `0x400` instead of `0x100`.

`ColumnScore` (`0x703E70`) requires either bridge predicate. It resolves
S `(0,1)`, E `(1,0)`, SE `(1,1)`. A matching cell has tile index other than
`0xFF/0xFFFF` and signed wrapping `tile_index - BridgeSetBase + 1` in **7..16**,
inclusive. The base is `AA0E28`. Its result is
`(S_matches || E_matches ? 1 : 0) + (SE_matches ? 1 : 0)`.
It is not a count of all three neighbors, and the old `[base+7,base+16]`
range omitted the native `+1` adjustment.

### Tube selector — `0x00704000`

OnBridge returns zero before cell lookup. Otherwise resolve N, W, current,
then select the first qualifying tube in **current,N,W** priority. The leaf
`0x484AB0` requires signed `Cell+116` index >=0 and < TubeCount, plus LandType
`+EC == 10`; it does not dereference a tube pointer. These predicates serve
YR's tube/low-bridge machinery, not only TS subterranean locomotion.

Take the chosen cell's stored coordinate; `(0,0)` is the null sentinel
initialized at `0x6F2A40`. Resolve one step N and W, then a second N/W step
from each first-step cell's stored coordinate. Return one if either second
cell satisfies the tube leaf, else zero. Retained dummy pointers observe later
dummy-coordinate stamps, so replacing them with independent coordinate snapshots
changes missing-cell behavior.

### Additional Z — `0x00704350`

Start with `b = -AdjustForZ(raw Location.Z)` (`0x6D20E0`), using the startup
double multiplier, the correction at Z>=728 and native x87 truncation. Existing
Rust `util::native_x87::adjust_for_z_standard` owns this conversion; it is not
simply `15*level` for every raw height.

Two **Unit-only** early returns precede terrain tests:

1. Unit `+418` dock-entered flag set, radio contact slot 0 (`0x65AD40`) is a
   Building, and its virtual `+184` mission is Unload (16): return `b-3`.
   Mission getter `0x5B3040` uses Current, or Queued when Current is -1.
2. Current `Unit+6C4` type has Harvester `+E0E`, and the current cell's
   `GetBuilding` (`0x47C520`) has type `+16BB Refinery`: return `b-14`.

These are not Infantry/Jumpjet/SpawnsPads cases as previously claimed.
For the ordinary terrain path:

1. If current Ramp `+11C != 0`, resolve S,E,SE. Inspect **only the first
   present overlay** in that priority. If its type `+2B5 IsARock` is true,
   return `b+2`; a present non-rock prevents later rocks from qualifying.
2. `FacingClass::Current` (`0x4C93D0`, object `+388`) gives
   `d=(((facing_u16>>12)+1)>>1)&7`. Resolve directions
   `A=d+1,B=d-1,C=d,D=d-4,E=d+3,F=d-3` modulo eight. If any of these six
   ramps is nonzero, return `b-1`; current ramp nonzero also returns `b-1`.
3. Noncardinal d returns `b-1`. For d=0/6 choose D,E,F; for d=2/4 choose
   A,B,C. Current or any chosen cell flag `0x10000` returns `b-1`.
4. If any chosen cell's `GetSubtileDimensions` (`0x547150`) height **>36**,
   return `b-2`; otherwise return `b-1`.

The native constants at `84311C/843120` are -1 and `843124` is 3 (rock path
adds that then subtracts one). `0x547150` reads the pristine TMP header through
IsoTile virtual `+9C` (`0x544CB0`), reduces positive subtile indices modulo
width*height, and returns file tile height, plus `storedY-rawExtraY` when the
subtile has extra-data flag bit 0. A blank subtile returns file tile height.
This is neither slope type nor the union atlas canvas height. Tile indices
`0xFF/0xFFFF` choose ClearTile with subtile zero before the call; other invalid
registry indices do not have a demonstrated native fallback.

### Active consumers and evidence boundary

- Unit primary vtable `7F5C70`, slot `+2EC` at `7F5F5C`, points to `4DAFC0`.
  Composite `73B140` and cached VXL `707480` pass it to native blitters.
- VXL shadows also call this virtual: cached `707706`, uncached `7073F1`.
  Shadow intensity 1000 is a separate argument, not its Z adjustment.
- `Techno_DrawSHP` (`705E00`) calls Foot only for Unit/Infantry with
  `Object::GetHeight()==0`, then includes the SHP caller adjustment -2.
  `GetHeight` (`5F5F40 → 578080 → 47B3A0`) subtracts actual interpolated
  slope ground height and, when OnBridge, the 416-lepton deck offset.
  Airborne Unit/Infantry and Aircraft use height cancellation without Foot.
- OREGATH directly calls Foot at `73D236`, subtracts two at `73D246`, and
  draws through `4AED70` before the UnloadingClass swap.

The [native oracle](../../tools/render_depth_oracle.py) executes original
functions without replacing their code/results. Its 85 recorded cases cover
ordinary mapped cells, signed levels/coefficient arithmetic, first/second cliff
samples, OnBridge, bridge/column competition, facing/TMP gates and exact-Z
boundaries. These are bounded function comparisons, not full-game or pixel
parity; its JSON documents omitted tubes, special units, aliases and dummy cases.

---

## 9. CliffBackImpassability

**RulesClass field:** offset `0x664` (1 byte, stored as `undefined1`)
**Read in:** `RulesClass::ReadGeneral` at `0x0066f1d9` via `CCINIClass::ReadInt`
**INI section:** `[General]`
**Default value:** typically `2` in standard YR

### Purpose

Controls whether cells at the base of cliffs are marked as impassable (LandType = Rock).
Prevents units from pathfinding into the "shadow" area behind cliff faces where they would
be invisible or stuck.

### Values

| Value | Behavior |
|-------|----------|
| `0` | Disabled — no cliff-back impassability |
| `1` | Enters the neighbor check code but does NOT change LandType (effectively disabled) |
| `2` | Enabled — marks cells behind cliffs as Rock (LandType 3 = impassable) |

### Algorithm (in CellClass::RecalcAttributes — `0x0047d2b0`)

The same check appears **3 times** in RecalcAttributes, covering different code paths
(overlay processing, empty tile fallback, final post-processing):

```
function CheckCliffBackImpassability(cell):
    if RulesClass.CliffBackImpassability == 0:
        return  // disabled

    // Check 6 isometric neighbors
    neighbors = [
        (cell.Y - 1, cell.X),      // N
        (cell.Y,     cell.X - 1),   // W
        (cell.Y + 2, cell.X + 2),   // SE
        (cell.Y + 1, cell.X + 1),   // S
        (cell.Y + 1, cell.X - 1),   // SW
        (cell.Y - 1, cell.X + 1),   // NE
    ]

    is_behind_cliff = false
    for each neighbor in neighbors:
        if neighbor.height >= cell.height + 4:
            is_behind_cliff = true
            break

    if is_behind_cliff AND CliffBackImpassability == 2:
        // Final check: only override certain land types
        if cell.LandType in {Clear(0), Water(2), Beach(6), Ice(8)}:
            cell.LandType = Rock (3)  // IMPASSABLE
```

**Key constant:** height difference threshold = **4 levels** (same as bridge height offset).

The 6 neighbors form the isometric adjacency ring. If ANY neighbor is ≥4 levels above the
current cell, the cell is considered "behind a cliff." Only LandTypes that are normally
passable (Clear, Water, Beach, Ice) get overridden — Rock, Road, Wall, etc. are left alone.

---

## 10. INI Keys Summary

### Theater INI `[General]` (tile set indices)

| Key | Type | Default | Purpose |
|-----|------|---------|---------|
| `CliffSet` | int | -1 | First tile set index for cliff tiles (40 tiles) |
| `CliffRamps` | int | -1 | First tile set index for cliff ramp tiles (20 tiles) |
| `WaterCliffs` | int | -1 | First tile set index for water cliff tiles (28 tiles) |
| `DestroyableCliffs` | int | -2 | First tile set index for destroyable cliff tiles (2 tiles) |

### Rules(md).ini `[General]`

| Key | Type | Default | Offset | Purpose |
|-----|------|---------|--------|---------|
| `CliffBackImpassability` | int (byte) | 2 | RulesClass+0x664 | Cliff-back cell impassability mode |

### Rules(md).ini `[BulletTypes]`

| Key | Type | Default | Offset | Purpose |
|-----|------|---------|--------|---------|
| `SubjectToCliffs` | bool | false | BulletTypeClass+0x296 | Bullet blocked by cliff faces |
| `SubjectToElevation` | bool | false | BulletTypeClass+0x297 | Range bonus from height advantage |
| `SubjectToWalls` | bool | false | BulletTypeClass+0x298 | Bullet blocked by wall overlays |

### Rules(md).ini `[TechnoTypes]`

| Key | Type | Default | Offset | Purpose |
|-----|------|---------|--------|---------|
| `ZFudgeCliff` | int | 10 | TechnoTypeClass+0xDC0 | Z-depth bias near cliffs |
| `ZFudgeColumn` | int | 5 | TechnoTypeClass+0xDC4 | Z-depth bias near bridge columns |
| `ZFudgeTunnel` | int | 10 | TechnoTypeClass+0xDC8 | Z-depth bias near tube cells |
| `ZFudgeBridge` | int | 0 | TechnoTypeClass+0xDCC | Z-depth bias near bridges while not OnBridge |

---

## 11. Integration Points

### Who calls these functions

| Function | Called by | When |
|----------|----------|------|
| `IsCliffOrImpassableTile` (0x004863d0) | Passability checks, movement | On pathfinding queries |
| `IsOnBridgeRamp` (0x00578d80) | Bridge/ramp logic | Movement height transitions |
| `Cliff Variant Randomizer` (0x005a1350) | Map loading, neighbor fixup | Map init and editor |
| `Destroyable Cliff Destruction` (0x00581140) | Damage/destruction system | When cliff takes lethal damage |
| `Bullet Cliff Check` (0x004CC360) | `BulletClass::AI` bounce check | Every bullet tick |
| `Foot Z compositor` (0x004DAFC0) | Unit VXL body/shadow, grounded Unit/Infantry SHP, OREGATH | Qualifying visible draws; see Section 8 gates |
| `CliffBackImpassability` (in 0x0047d2b0) | CellClass::RecalcAttributes | On cell attribute recalc |

---

## 12. Rust Rendering Connection and Validation Scope

The `feature/sprite-zbuffer-depth` continuation started at `51213e58` with
TMP depth decode/upload, terrain depth writes, and VXL reads of the same depth
attachment already connected. That path was not a missing cliff implementation
to rebuild. The ordinary Unit producer nevertheless supplied only elevation
cancellation to `SpriteInstance.z_adjust`; the Foot sibling selectors and
additional adjustment were absent. Painter-sort bridge bias could not substitute
for that fragment-depth input.

The authorized 2026-09-08 candidate introduces the cohesive Foot calculation in
[render/foot_depth.rs](../../src/render/foot_depth.rs), rules-owned coefficients,
and an [instance adapter](../../src/app/presentation/instances/foot_depth.rs)
that gathers current authoritative type/entity/terrain facts. The adapter must
preserve SHP caller gates, the temporary UnloadingClass semantics, slope-height
inputs and every existing VXL piece/shadow consumer. The
[plan](../plans/2026-09-08-cliff-unit-depth-plan.md) states the integration and
acceptance scenarios; existence of a helper alone is not delivery evidence.

Rendering lookups deliberately do not stamp simulation's shared dummy. The pure
helper preserves retained-dummy coordinate behavior locally, preventing camera
and frame timing from modifying state consumed by wave/projectile simulation.

`matches_unmodified_gamemd_foot_and_selector_execution` compares the pure Rust
owner to the [85 native vectors](../../tools/render_depth_vectors.json).
Separate GPU regressions exercise the actual voxel shader and retail cliff
depth input. Neither those fixtures nor earlier accepted wall screenshots prove
complete cliff/bridge rendering parity. The user deferred the manual cliff
comparison; keep that acceptance separate from code/test validation.

The previous section's path/line-based status list for classification,
passability, bullet collision, destroyable cliffs and randomization was
historical and was not re-audited here. Consult current source and fresh native
evidence before using it as an implementation backlog.

---

## 13. Open Questions

1. **Resolved, 2026-09-08 — `89F694`:** initializer `49F34A` writes `(1,1)`.
   Section 8 records both independent cliff probes and their different coordinate
   origins. The former NW inference is superseded.

Items 2–4 below are historical questions, not revalidated by this rendering audit.

2. **CliffBackImpassability value 1:** The code checks `!= 0` to enter the neighbor scan but
   only `== 2` to apply LandType change. Value 1 would scan neighbors but never mark cells
   as Rock. Is this intentional (debug mode?) or is 1 simply unused? **Confidence: LOW**

3. **Cliff variant randomizer seed:** Uses `Random::Next()` which is the global non-deterministic
   random, not the sim random. This confirms it's visual-only (not synced in multiplayer).
   **Confidence: HIGH**

4. **Theater INI parsing:** We don't currently parse cliff tile set indices from theater INI
   `[General]` section. Need to verify the exact section format and which theaters have which
   cliff sets. **Confidence: N/A — implementation gap**

---

## 14. Active Foot Rendering and Historical Legacy Claims

The earlier recommendation to omit `ZFudgeCliff` is withdrawn. It inferred a
zero default from missing INI assignments without checking constructor writes.
The active Unit vtable and ordinary composite/cache callers reach the helper;
constructor 10 produces cliff terms of 10 or 20. All four siblings belong in
the native signed maximum, with their real defaults and per-type overrides.

| Rendering setting | Established initialization/consumer | Scope |
|---|---|---|
| `ZFudgeCliff` | Default 10; `704240` samples signed levels | Ordinary stock Foot draws can activate it without any INI override |
| `ZFudgeColumn` | Default 5; `703E70` bridge gate and tile range | Configured values compete with cliff, not add to it |
| `ZFudgeTunnel` | Default 10; `704000` uses live tube-cell predicate `484AB0` | Must not be discarded merely because the name suggests TS tunnels |
| `ZFudgeBridge` | Default 0; `703B10` near-bridge gate | Explicit stock overrides such as 7 do not change other types' defaults |

The old non-Foot analysis remains historical. It reported missing theater
definitions for DestroyableCliffs, no direct callers for one fixup wrapper, and
active passability/projectile/randomization mechanisms. None of those claims
was re-established in this revision, and this correction does not authorize
implementing or deleting their systems. In particular, absence of direct
Ghidra xrefs alone is not an exhaustive reachability proof for virtual/native
dispatch. Use Sections 2–7 and 9 as leads for a separate audit, not current
delivery or dormancy certification.

---

## Sources

### Rechecked Foot rendering evidence, 2026-09-08

Retail `gamemd.exe` SHA-256:
`1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c`.

- `710AF0`, `711664..71167A`, `7470D0`, `5236A0`: constructor defaults and inheritance.
- `71541C..715437`: existing-value INI fallback for `ZFudgeCliff`.
- `4DAFC0..4DB09E`, `7F5F5C`: real entry, arithmetic and active Unit virtual.
- `704240`, `49F34A`, `41BEA0`, `5657A0`: cliff probes, direction and cell lookup.
- `703B10`, `703CC0`, `703E70`, `704000`, `484AB0`: sibling selectors.
- `704350`, `547150`, `544CB0`, `6D20E0`: additional Z and pristine TMP height.
- `5F5F40`, `578080`, `47B3A0`, `5B3040`: exact ground and mission gates.
- `73B140`, `707480`, `707280`, `705E00`, `73D236..73D3A2`: body/shadow/SHP/OREGATH consumers.
- [Executable oracle](../../tools/render_depth_oracle.py),
  [recorded inputs/results](../../tools/render_depth_vectors.json), and
  [implementation plan](../plans/2026-09-08-cliff-unit-depth-plan.md).

### Historical address inventory (non-Foot entries not rechecked in this revision)
- `0x00545714` — Tile set loading (CliffSet, CliffRamps, WaterCliffs, DestroyableCliffs read)
- `0x004863d0` — IsCliffOrImpassableTile
- `0x00578d80` — IsOnBridgeRamp
- `0x005a1350` — Cliff variant randomizer
- `0x005a17f0` — Cliff neighbor fixup
- `0x00581140` — Destroyable cliff destruction
- `0x00486900` — IsDestroyableCliff check
- `0x004CC360` — Bullet cliff/wall collision
- `0x004DAFC0` — Foot Z compositor (`0x004DAFF0` is inside its body)
- `0x00704240` — IsNearCliff (cliff proximity for ZFudge)
- `0x00703e70` — IsNearBridgeColumn
- `0x00704000` — IsNearTunnel
- `0x00703b10` — Near-bridge predicate, disabled when Object.OnBridge is true
- `0x00704350` — ComputeAdditionalZ
- `0x0047d2b0` — CellClass::RecalcAttributes (CliffBackImpassability consumer)
- `0x00487d50` — CellClass::GetEffectiveHeight
- `0x0066f1d9` — RulesClass::ReadGeneral (CliffBackImpassability read)
- `0x0046bfeb` — BulletTypeClass::ReadINI (SubjectToCliffs read)
- `0x00715423` — TechnoTypeClass::ReadINI (ZFudgeCliff read)

### Historical INI inventory
- `ini/rulesmd.ini`, `ini/rules.ini` — SubjectToCliffs, ZFudge values
- `ini/artmd.ini`, `ini/art.ini` — no cliff-specific keys found
- Theater INIs (temperatemd.ini etc.) — not present in repo, values read from MIX archives

The 2026-09-08 Foot check used the machine-local extracted `ini/rulesmd.ini`
for override presence. YR does not merge base RA2 `rules.ini` beneath it; the
older inventory above is not evidence of such a merge.

### Existing docs referenced
- [BULLET_CLASS_AI_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/BULLET_CLASS_AI_GHIDRA_REPORT.md) — SubjectToCliffs field offset verification
- [TERRAIN_COST_FACTSHEET.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TERRAIN_COST_FACTSHEET.md) — LandType enum, passability matrix
- `BRIDGE_SYSTEM.md` — Bridge flags (0x80, 0x100), height conventions
- [COORDINATE_ATOMS_AUDIT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/COORDINATE_ATOMS_AUDIT.md) — Height-to-pixel conversion constants
- [ZONE_PASSABILITY_VERIFIED.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ZONE_PASSABILITY_VERIFIED.md) — MovementZone/SpeedType passability matrix
- [UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/UNIT_CAN_ENTER_CELL_GHIDRA_REPORT.md) — Height difference thresholds
- `VOXEL_SLOPE_TILT_SYSTEM.md` — Slope type values
