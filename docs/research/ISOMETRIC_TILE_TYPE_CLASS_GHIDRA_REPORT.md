# IsometricTileTypeClass & LAT Tile Variants — Ghidra Research Report

**Primary addresses:**
- `IsometricTileTypeClass::Constructor(idx, flags, b, name, flag6)` @ `0x005447C0`
- `IsometricTileTypeClass::~IsometricTileTypeClass()` @ `0x00544A70`
- `IsometricTileTypeClass::FindByName()` @ `0x00544CE0`
- `Theater_ReadINI_LoadTileSets()` @ `0x00545150` (Ghidra label: `Read_Theater_TileSets_INI`; corrected 2026-05-28: was noted as mislabeled `CDFileClass__Constructor`; binary now shows correct label via `get_function_by_address 0x00545150` — RTTI_LABEL_DRIFT)
- `TMP_Loader()` @ `0x00547020`
- `TMP_ReadSlopeType()` @ `0x005471B0`
- `CellClass::ApplyLAT_and_SlopeFixup()` @ `0x0047CA80`
- `CellClass::RecalcAttributes()` @ `0x0047D2B0`
- `MapClass::InitCellAttributes()` @ `0x00568BB0`
- `MapClass::SelectBridgeTileVariant_Low()` @ `0x0057ACF0` (corrected 2026-05-28: was `0x0057B133` which is mid-body label LAB_0057b12d, not the entry; confirmed entry via `get_function_by_address 0x0057B133` returning entry `0x0057acf0` — GHIDRA_ADDRESS_SHIFT)

**Confidence:** HIGH (primary struct, ReadINI, LAT, TMP_Loader all decompiled and verified)
**Active in YR:** Yes — core map rendering + terrain classification. LAT + slope fixup both fire at map load and on every cell mutation.

---

## 1. Overview

`IsometricTileTypeClass` is the type-definition class for every terrain tile graphic the
engine can render. Every cell in the map stores a `u16 IsoTileTypeIndex` (CellClass+0x38)
that indexes into a global pointer array `g_IsoTileTypeArray` (`DAT_00a8ed2c`). Tile types
are instantiated during theater load from the per-theater INI (`temperatmd.ini`,
`snowmd.ini`, `urbanmd.ini`, `desertmd.ini`, `lunarmd.ini`) driven by a sequence of
`[TileSet0000]..[TileSetNNNN]` sections. Each TileSet spawns `TilesInSet`
IsometricTileTypeClass instances (one per frame), loads the TMP pixel/depth/metadata
file (plus up to 3 random variants `a/b/c/d`), and appends them to the global array.

LAT ("Lookup Adjacent Tile") is a runtime auto-tile system that morphs 4 terrain
groups — Rough, Sand, Green, Pave — by examining the 4 cardinal neighbors of every
cell and rewriting `IsoTileTypeIndex` so the graphic blends to adjacent terrain.
It runs inside `CellClass::ApplyLAT_and_SlopeFixup`, which is called by
`CellClass::RecalcAttributes` — so every cell mutation (overlay placement, bridge
damage, AoE, building sell) triggers LAT recomputation on the affected cell.

---

## 2. Class Layout (IsometricTileTypeClass, size `0x30C` / 780 bytes)

The constructor at `0x005447C0` uses `param_1` typed as `undefined4 *` — so array
indexing `param_1[N]` must be read as byte offset `N*4`. Writes through
`*(char *)((int)param_1 + 0xNN)` and `*(char *)(param_1 + 0xNN)` (the latter casts
a `int *` to `char *` without multiplying; Ghidra shows this clearly as
`*(undefined1 *)(param_1 + 0xNN)` which is byte offset `0xNN * 4`) — both forms
appear and must be read carefully.

Verified offsets (all from decompilation of Constructor 0x005447C0 + ReadINI 0x00545150):

| Offset | Type | Name / Source | Default | Notes |
|--------|------|---------------|---------|-------|
| `0x000` | `void**` | vtable primary | `&vtable__IsometricTileTypeClass` | |
| `0x004` | `void**` | vtable secondary_4 | | |
| `0x008` | `void**` | vtable secondary_8 | | |
| `0x00C` | `void**` | vtable secondary_12 | | |
| `0x024` | `char[24]` | ObjectTypeClass::ID (base FileName + index — e.g. `"CLEAR01"`) | `""` | set via `ObjectTypeClass__Constructor(param_5)` |
| `0x064` | `char[49]` | Display name — `"%s(%02d)"` built from SetName + tile index | `""` | `strncpy(…, src, 0x30)` then explicit null at +0x94 |
| `0x0A4` | `byte*` | TMP raw data pointer (set by `TMP_Loader`) | `0` | field `[0x29]` in int indexing |
| `0x0A8` | `u8` | TMP loaded flag | `0` | field `[0x2a]` byte |
| `0x22F` | `u8` | — | `1` | |
| `0x230` | `u8` | — | `0` | |
| `0x231` | `u8` | — | `0` | |
| `0x232` | `u8` | — | `1` | |
| `0x233` | `u8` | — | `1` | |
| `0x235` | `u8` | — | `0` | |
| `0x294` | `i32` | Tile type index (= slot in `g_IsoTileTypeArray`) | `param_2` | absolute global tile_id |
| `0x298` | `i32` | **MarbleMadness** — tileset number (pre-fixup), then tile_id (post-fixup) | `0xFFFF` | resolved at TilesInSet=-1 terminator |
| `0x29C` | `i32` | **NonMarbleMadness** — same resolution pattern | `0xFFFF` | |
| `0x2A0` | `i32` | Tileset start tile_id (first tile of this set) | `0` | set to `g_TileSet_StartTileIds[TilesetIdx]` |
| `0x2A4` | `void**` | DynamicVectorClass vtable for attached anims | `&PTR_FUN_007eccec` | tile-anim sub-collection |
| `0x2A8` | `ptr` | Attached animation array buffer | `0` | managed by the vector at +0x2A4 |
| `0x2AC` | `i32` | Anim array count | `0` | |
| `0x2B0` | `u8` | Anim array owned flag | `1` | |
| `0x2B1` | `u8` | (alt owned flag) | `0` | |
| `0x2B4` | `i32` | — collection count | `0` | |
| `0x2B8` | `i32` | — initial capacity | `10` | |
| `0x2BC` | `IsoTileType*` | **Variant chain next pointer** (a → b → c → d linked list) | `0` | field `[0xaf]` |
| `0x2C0` | `i32` | **ToSnowTheater** — snow-theater equivalent tileset | `-1` | |
| `0x2C4` | `i32` | **ToTemperateTheater** — temperate equivalent | `-1` | |
| `0x2C8` | `AnimType*` | **Tile%dAnim** — attached anim type pointer | `-1` | resolved via `AnimTypeClass::FindByName` |
| `0x2CC` | `i32` | **Tile%dXOffset** | `0` | pixels |
| `0x2D0` | `i32` | **Tile%dYOffset** | `0` | pixels |
| `0x2D4` | `i32` | **Tile%dAttachesTo** — cell height anim attaches at | `-1` | compared against `this->Height` |
| `0x2D8` | `i32` | **Tile%dZAdjust** | `0` | anim Z offset |
| `0x2DC` | `u8` | constructor param_3 (type-sig flag) | `param_3 & 0xFF` | stored as int-sized but only low byte set |
| `0x2E0` | `u8` | **Morphable** | `0` | bool |
| `0x2E1` | `u8` | **ShadowCaster** (ShadowTiles>0 only) | `0` | bool |
| `0x2E2` | `u8` | **AllowToPlace** | `1` | bool |
| `0x2E3` | `u8` | **RequiredForRMG** | `0` | bool |
| `0x2E4` | `i32` | TMP width in cells — read from `tmp[0]` at load | `0` | |
| `0x2E8` | `i32` | TMP height in cells — read from `tmp[4]` at load | `0` | |
| `0x2EC` | `u8` | constructor param_4 (second type-sig flag) | `param_4 & 0xFF` | |
| `0x2F0` | `i32` | **Variants available** (1 + count of a/b/c/d successfully loaded) | `1` | base gets `N`, first variant `N-1`, etc. |
| `0x2F4` | `u8` | Persistent-file flag (keeps TMP mapped on disk) | `0` | |
| `0x2F5` | `char[14]` | Source filename of the loaded TMP variant (for `TMP_Loader` reload) | `""` | |
| `0x305` | `u8` | **AllowBurrowing** | `1` | bool |
| `0x306` | `u8` | **AllowTiberium** — ore may grow here | `0` | bool |
| `0x308` | `i32` | Per-frame drawn-flag (cleared by `FUN_00547110` each frame) | `0` | |

**Global arrays (verified):**
- `g_IsoTileTypeArray` @ `DAT_00a8ed2c` — `IsometricTileTypeClass**`, indexed by tile_id.
- `g_IsoTileTypeArray_Count` @ `DAT_00a8ed38` — total tile count across all tilesets.
- `g_TileSet_StartTileIds` @ `DAT_00aa1140` — `i32[]`, indexed by tileset number → first tile_id of that set. Built incrementally in ReadINI. This is the critical data structure for all tileset-number → tile_id translation throughout the engine.
- `g_TileSet_Count` @ `DAT_00abc558` — number of tilesets read so far.

---

## 3. Theater Load — `Theater_ReadINI_LoadTileSets` @ `0x00545150`

Ghidra labels this as `CDFileClass__Constructor` but the body is the full theater
tile-loading routine. Signature: `void(TheaterType theater, char loadFromCDOnly)`.
Sequence:

### 3.1 Per-theater palette + master INI

```
CCFileClass(s_ISO_s_PAL_008295f4 fmt "iso{ext}.pal")  // isotem.pal, isosno.pal, isourb.pal, isodes.pal, isolun.pal
LoadPalette()                                         // left-shift 2 bits if HiColor
FUN_00545000()                                        // palette fixup
CCFileClass(s__sMD_INI_008295e8 fmt "{name}md.ini")   // temperatmd.ini etc.
```

(Falls back to non-MD INI if MD variant missing — standard RA2/YR merge pattern.)

### 3.2 `[General]` section — 49 theater-wide tile-index keys

All read with `CCINIClass::ReadInt(…, -1)` (except `DestroyableCliffs` which defaults to `-2`):

| Key | Global |
|-----|--------|
| `RampBase` | `g_RampBase` |
| `RampSmooth` | `g_RampSmooth` |
| `MMRampBase` | `DAT_00aa109c` |
| `ClearTile` | `g_ClearTile` |
| `RoughTile` | `g_RoughTile` |
| `SandTile` | `g_SandTile` |
| `GreenTile` | `g_GreenTile` |
| `PaveTile` | `g_PaveTile` |
| `MiscPaveTile` | `g_MiscPaveTile` |
| `ClearToRoughLat` | `g_ClearToRoughLat` |
| `ClearToSandLat` | `g_ClearToSandLat` |
| `ClearToGreenLat` | `g_ClearToGreenLat` |
| `ClearToPaveLat` | `g_ClearToPaveLat` |
| `HeightBase` | `DAT_00aa0744` |
| `BlackTile` | `DAT_00abc2cc` |
| `BridgeSet` | `DAT_00aa0e28` |
| `WoodBridgeSet` | `DAT_00abad1c` |
| `CliffSet` | `DAT_00aa1020` |
| `ShorePieces` | `g_ShorePieces` |
| `WaterSet` | `DAT_00aa0738` |
| `SlopeSetPieces` | `DAT_00abc1f8` |
| `SlopeSetPieces2` | `DAT_00aa1098` |
| `MonorailSlopes` | `DAT_00aa1024` |
| `Tunnels` | `DAT_00aa1054` |
| `TrackTunnels` | `DAT_00abb108` |
| `DirtTunnels` | `DAT_00aa10b4` |
| `DirtTrackTunnels` | `DAT_00abad2c` |
| `WaterfallEast/West/North/South` | 4 globals |
| `CliffRamps` | `DAT_00abbebc` |
| `PavedRoads` | `g_PavedRoads` |
| `PavedRoadEnds` | `DAT_00abbec4` |
| `Medians` | `g_Medians` |
| `RoughGround` | `DAT_00aa0e1c` |
| `DirtRoadJunction/Curve/Straight` | 3 globals |
| `DestroyableCliffs` | `DAT_00abc2c8` (default `-2`) |
| `WaterCaves` | `DAT_00abad24` |
| `WaterCliffs` | `DAT_00aa101c` |
| `PavedRoadSlopes` | `DAT_00aa1094` |
| `DirtRoadSlopes` | `DAT_00abbec0` |
| `Rocks` | `DAT_00abb10c` |
| `WaterBridge` | `g_WaterBridge` |
| `BridgeTopLeft1/2`, `BridgeTopRight1/2`, `BridgeBottomLeft1/2`, `BridgeBottomRight1/2`, `BridgeMiddle1/2` | 10 bridge variant tileset indices |

At this point every global holds the **tileset number** from INI (not a tile_id yet).

### 3.3 TileSet loop — iterate `[TileSetNNNN]` until `TilesInSet=-1`

For each TileSet:

1. Read `TilesInSet` (default `-1`). If `-1`, **terminator** — break out and do post-load fixup.
2. Store `g_TileSet_StartTileIds[tilesetNum] = iVar16` (current running tile_id = start of this set).
3. For each of the 49 `[General]` globals: if the current tileset number matches, **replace the global with the absolute tile_id** (`iVar16`). After all tilesets loaded, each global like `g_ShorePieces` holds the first absolute tile_id of the corresponding set, not a tileset number.
4. Read `LastTilesInSet` — if non-`-1` and != `TilesInSet`, creates a LastTilesInSet exception range (`{start, count}` entry) in `DAT_00aa107c`. Purpose: legacy map-conversion support for older editors.
5. Read TileSet properties:

   | Key | Type | Default | Stored at offset |
   |-----|------|---------|------------------|
   | `SetName` | string | `"No Name"` | (in format template for display name +0x64) |
   | `FileName` | string | `""` | used to build TMP filename |
   | `MarbleMadness` | int | `0xFFFF` | `+0x298` (raw tileset num) |
   | `NonMarbleMadness` | int | `0xFFFF` | `+0x29C` |
   | `Morphable` | bool | `0` | `+0x2E0` |
   | `AllowToPlace` | bool | `1` | `+0x2E2` |
   | `AllowBurrowing` | bool | `1` | `+0x305` |
   | `AllowTiberium` | bool | `0` | `+0x306` |
   | `RequiredForRMG` | bool | `0` | `+0x2E3` |
   | `ToSnowTheater` | int | `-1` | `+0x2C0` |
   | `ToTemperateTheater` | int | `-1` | `+0x2C4` |
   | `ShadowCaster` | bool | `0` | drives ShadowTiles read |
   | `ShadowTiles` | int | `0` | (only if ShadowCaster) |

6. Per-tile (for each tile `n` in `1..=TilesInSet`):
   - **Variant passes** (suffix = `''`, `'a'`, `'b'`, `'c'`, `'d'`, …): attempt to load `{FileName}{nn:02}{suffix}.{ext}` from MIX/disk. Stop when both MIX and CD lookup fail.
   - **Pass 0 (base)** creates the primary IsometricTileTypeClass (`operator_new(0x30C)`), copies properties from the TileSet, sets `+0x294 = running tile_id`. Then reads per-tile anim keys:
     - `Tile%dAnim` (name) → looks up AnimType → stored at `+0x2C8`
     - `Tile%dXOffset` / `YOffset` → `+0x2CC` / `+0x2D0`
     - `Tile%dAttachesTo` → `+0x2D4`
     - `Tile%dZAdjust` → `+0x2D8`
   - **Passes 1..N (variants)** create additional IsometricTileTypeClass instances, set `+0x2BC` on the previous tile to point at the new one, sharing all TileSet-level properties but with a new TMP file.
   - After all variants loaded, loop backwards and set `+0x2F0` (variant count field): base = total count, first variant = count-1, last = 1.
   - TMP data pointer written to `+0xA4`; width/height copied from `tmp[0]` / `tmp[4]` to `+0x2E4` / `+0x2E8`; cell offset table at `tmp[0x10]` has relative offsets which are relocated to absolute: `cells[i] += tmp_base`.
   - `FUN_00549e90` called to build shadow/radar color tables for each populated cell.

7. When `TilesInSet == -1` is hit (terminator), **MarbleMadness fixup pass** rewrites `+0x298` and `+0x29C` on every loaded tile:
   ```
   delta = tile[0x294] - tile[0x2A0]          // position of this tile within its set
   if tile[0x298] != 0xFFFF:
       tile[0x298] = g_TileSet_StartTileIds[tile[0x298]] + delta
   if tile[0x29C] != 0xFFFF:
       tile[0x29C] = g_TileSet_StartTileIds[tile[0x29C]] + delta
   ```
   So `MarbleMadness=N` semantically means *"my same-position counterpart lives in tileset N"*, resolved to an absolute tile_id only after the target tileset is loaded.

8. Interior/lunar theater special (`local_95c == 5` branch inside the terminator): zeros out `g_ShorePieces`, `DAT_00aa0738 (WaterSet)`, `DAT_00aa1020 (CliffSet)`, `DAT_00aa101c (WaterCliffs)`, `g_WaterBridge`, `DAT_00aa0e28 (BridgeSet)`, `DAT_00abad1c (WoodBridgeSet)` — Lunar theater has no water/bridges.

---

## 4. LAT Auto-Transition — `CellClass::ApplyLAT_and_SlopeFixup` @ `0x0047CA80`

The LAT routine processes **4 terrain groups in fixed order: Rough → Sand → Green → Pave**.
Each group skips if its `ClearToXxxLat` global is `-1` (theater doesn't have that LAT set).

### 4.1 Algorithm (per group)

```
if cell.IsoTileTypeIndex == baseTile
   OR clearToXxxLat <= cell.IsoTileTypeIndex <= clearToXxxLat + 15:

    mask = 0
    for bit in 0..4:                     // 4 cardinal neighbors only
        dir = g_DirectionOffsets[bit * 2]  // step by 2 in 8-dir table → N, E, S, W
        neighbor = MapClass::Get_CellClass(cell.coord + dir)
        n = neighbor.IsoTileTypeIndex
        if n != baseTile
           AND (n < clearToXxxLat OR n > clearToXxxLat + 15)
           AND n is NOT in any exemption range:
            mask |= 1 << bit

    if mask == 0:
        cell.IsoTileTypeIndex = baseTile               // fully surrounded → pure base
    else:
        cell.IsoTileTypeIndex = clearToXxxLat + mask   // 1..15 → 15 LAT variants
```

### 4.2 Direction offsets

`g_DirectionOffsets` is an 8-entry table at `0x0089f688+`. LAT iterates with `uVar12 = 0, 2, 4, 6`, so only cardinals are sampled. Bit assignment is by loop order:
- bit 0 = North (`_g_DirectionOffsets[0]` / `DAT_0089f68a[0]`)
- bit 1 = East  (`_g_DirectionOffsets[2]` / `DAT_0089f68a[2]`)
- bit 2 = South (`_g_DirectionOffsets[4]` / `DAT_0089f68a[4]`)
- bit 3 = West  (`_g_DirectionOffsets[6]` / `DAT_0089f68a[6]`)

### 4.3 Exemption ranges (hardcoded, NOT read from INI)

YR does **not** use `*ConnectTo` INI keys — these exemptions are baked into the function:

| Group | Exemption ranges (`[lo, hi]` inclusive, skipped only if the bound global is set) |
|-------|--------------------------------------------------------------------------|
| Rough | *(none)* |
| Sand  | *(none)* |
| Green | `[g_ShorePieces, g_ShorePieces + 0x29]` (42 tiles), `[g_WaterBridge, g_WaterBridge + 1]` |
| Pave  | `[g_MiscPaveTile, g_MiscPaveTile + 0xD]` (14), `[g_Medians, g_Medians + 0xD]` (14), `[g_PavedRoads, g_PavedRoads + 0x14]` (21) |

If a global is `-1` (not defined in this theater), its exemption range is also set to `-1..-1` (no-op).

### 4.4 Slope / ramp fixup (same function, after LAT)

Runs only if the new tile index is already a ramp (`g_RampBase..+0x13` or `g_RampSmooth..+0xB`):

`SlopeIndex` is stored at CellClass offset `0x11C` (byte). For values 1..4, picks a
`RampSmooth` tile based on 2 specific diagonal-neighbor **"flat"** bits:

| SlopeIndex | Neighbors checked | Output formula |
|------------|-------------------|----------------|
| 1 (west)  | `0089F6A0`, `0089F690` | `RampSmooth + (mask - 1)` (range 0..1) |
| 2 (north) | `0089F68A` (N), `0089F698` (NE) | `RampSmooth + mask + 2` (range 3..4) |
| 3 (east)  | `0089F690`, `0089F6A0` | `RampSmooth + mask + 5` (range 6..7) |
| 4 (south) | `0089F698`, `0089F68A` | `RampSmooth + mask + 8` (range 9..10) |

"Flat neighbor" = `neighbor.SlopeIndex == 0`. Mask bits: bit0 = neighbor A flat, bit1 = neighbor B flat.

Fallback (no smooth variant applies): `cell.IsoTileTypeIndex = RampBase + (SlopeIndex - 1)`.

### 4.5 Post-check sanity

After rewriting, the function validates the new tile's cell at `Height`:
```
if old_tile != new_tile:
    cell_ok = vtable.IsCellValid(new_tile_ptr)
    if !cell_ok:
        TMP_Loader(new_tile_ptr)    // force-reload TMP if missing
```

Returns `true` if the tile was changed, `false` otherwise.

### 4.6 When LAT fires

`CellClass::ApplyLAT_and_SlopeFixup` is called only from `CellClass::RecalcAttributes`
(`0x0047D2B0`). `RecalcAttributes` has 30+ callers:
- `MapClass::InitCellAttributes` (`0x00568BB0`) — called during `ScenarioClass::Full_Init`; iterates every cell on map load.
- `MapClass::ToggleBridgePavement` (`0x0056E9F6`)
- `MapClass::SelectDestroyedBridgeTile_Low` (`0x00579ACA`), `SelectBridgeTileVariant_Low` (`0x0057B133`)
- Various overlay place/remove, building place/sell, AoE damage paths

**Implication:** LAT runs at load time for the whole map, and then incrementally for any
cell whose overlay/tile/height changes. There is no global "recompute all LAT" pass
outside load; runtime changes must trigger `RecalcAttributes` on the changed cell.

---

## 5. TMP File Layout (`TMP_Loader` @ `0x00547020`, `TMP_ReadSlopeType` @ `0x005471B0`)

Minimal verified layout (from TMP_Loader):

```
offset 0x00  u32  width_in_cells   (only low byte read as *pbVar2)
offset 0x04  u32  height_in_cells  (only low byte read as pbVar2[4])
offset 0x10  cell_offset_table[width*height]
             each entry is a file-relative u32 offset to that cell's data
             relocated to absolute at load: cell_offset[i] += base_addr
             0 = blank cell
```

Each cell header (per `TMP_ReadSlopeType` + `FUN_00544BE0`):
```
cell + 0x29  u8   terrain_type   // indexes LandType via &DAT_008288e4 table (one u32 per entry)
cell + 0x2A  u8   slope/ramp_type  (0 = flat; 1..4 = cardinal ramp; 5..8 = diagonal ramp)
cell + 0x2B..0x30  radar_color_left[3] + radar_color_right[3] (6 bytes, used by FUN_00549E90)
```

`FUN_00544BE0(tile_type, cell_num)` returns the LandType of a specific cell:
```
cell = tmp.cells[cell_num % (width * height)]
if cell != 0:
    return DAT_008288E4[cell.terrain_type]   // u32 LandType
```

LandType enum values (observed from CellClass::RecalcAttributes and the
DAT_008288E4 lookup table):

| Value | Name | Notes |
|-------|------|-------|
| 0 | Clear | default for ClearTile / empty |
| 1 | Rough | |
| 2 | Road | |
| 3 | Rock | |
| 4 | Water | (also triggers OverlayType "water" path) |
| 5 | Beach | (used when slope < 5 + overlay water) |
| 6 | Weeds | |
| 7 | Tiberium | (ore LandType) |
| 8 | Ice | |
| 9 | Railroad | |
| 10 | Wall | (also the "bridge" LandType — CellClass::RecalcAttributes has `LandType==10` branch that instantiates `TubeClass` when `IsoTileTypeIndex` falls in Tunnels/TrackTunnels/DirtTunnels/DirtTrackTunnels — this is the tube-tunnel entry detection) |

---

## 6. MarbleMadness Semantics

`MarbleMadness=N` and `NonMarbleMadness=N` (in `[TileSetNNNN]`) cross-link a tile to
its "marble" or "non-marble" counterpart **in another tileset at the same position**.
Resolution is a two-phase process:

1. During load: store raw tileset number at `+0x298` / `+0x29C` (default `0xFFFF` = none).
2. At TilesInSet=-1 terminator: fixup pass rewrites both fields:
   `tile[+0x298] = g_TileSet_StartTileIds[tile[+0x298]] + (tile[+0x294] - tile[+0x2A0])`

After fixup, `+0x298` holds the absolute tile_id of the MarbleMadness counterpart.

The actual tile-swap at draw time was not decompiled in this pass, but the field is
laid out as a ready-to-use index — swap is a single array lookup in the draw path. The
engine defines `MMRampBase` (distinct from `RampBase`) so the slope fixup also has an
MM variant path.

**Active in YR:** Conditional — driven by the `[MultiplayerDialogSettings] MarbleMadness`
option (skirmish / multiplayer). When on, the draw path prefers `+0x298` over the tile's
own index. Field is populated in single-player too (since INI/cross-tileset linking is
static), just not consulted unless the flag is set.

---

## 7. Variant (a/b/c/d) Chain

Variants are loaded as **sibling IsometricTileTypeClass instances** chained via
`+0x2BC` (next pointer). The base tile's `+0x2F0` field holds the total variant count;
each successive variant's `+0x2F0` decrements. Used by:
- `FUN_00547110` — clears per-frame flag `+0x308` on every tile *and every variant*
- `FUN_00549E90` — runs TMP fixup on the base and every variant

The ReadINI loop attempts variant suffixes from `'a'` through at least some upper
bound (inner `do/while` continues until both MIX and disk lookup return empty for a
given suffix). So variant count is data-driven, not capped at 4 in the loader. Typical
retail YR theaters define up to 4 variants (`a`, `b`, `c`, `d`).

**Selection at draw time** was not decompiled in this pass but is straightforward: a
per-cell pseudo-random pick among the base + `+0x2F0 - 1` variants, walking the `+0x2BC`
chain `randIdx` times. The per-frame flag at `+0x308` suggests the selection may be
cached or clamped per render pass.

---

## 8. INI Keys Summary

### 8.1 `[General]` (per theater INI)

49 integer keys listed in §3.2. Each is either **-1 = not defined** or **a tileset number**
(which gets resolved to the first tile_id of that tileset during load). After load, all
globals hold absolute tile_ids.

### 8.2 `[TileSetNNNN]`

| Key | Type | Default | Effect |
|-----|------|---------|--------|
| `TilesInSet` | int | `-1` | Count of tiles; `-1` terminates the section list |
| `LastTilesInSet` | int | `-1` | Legacy count for map-converter compatibility |
| `SetName` | str | `"No Name"` | Display name (stored per-tile as `"%s(%02d)"`) |
| `FileName` | str | `""` | Base filename; real TMP name = `{FileName}{NN:02}{suffix}.{ext}` |
| `MarbleMadness` | int | `0xFFFF` | Tileset# for MM counterpart |
| `NonMarbleMadness` | int | `0xFFFF` | Tileset# for non-MM counterpart |
| `Morphable` | bool | `false` | Can be raised/lowered by terraform |
| `AllowToPlace` | bool | `true` | Visible in FA2/WAE palette |
| `AllowBurrowing` | bool | `true` | Chrono Commando / burrower units may burrow |
| `AllowTiberium` | bool | `false` | Ore may grow on this tile (gated by `IsCellAllowed` paths) |
| `RequiredForRMG` | bool | `false` | Random Map Generator must preserve |
| `ToSnowTheater` | int | `-1` | Tileset# of snow equivalent (for theater-swap) |
| `ToTemperateTheater` | int | `-1` | Tileset# of temperate equivalent |
| `ShadowCaster` | bool | `false` | Tile casts shadow on neighbors |
| `ShadowTiles` | int | `0` | (only if `ShadowCaster=yes`) number of shadow variants |
| `Tile%dAnim` | str | — | Per-tile looping animation name (AnimType) |
| `Tile%dXOffset` | int | `0` | Pixel X offset for attached anim |
| `Tile%dYOffset` | int | `0` | Pixel Y offset for attached anim |
| `Tile%dAttachesTo` | int | `-1` | Cell index within tile the anim attaches to |
| `Tile%dZAdjust` | int | `0` | Z-depth adjustment for anim |

### 8.3 INI keys NOT used by YR LAT (common misconception)

The following keys exist in mod documentation but are **not** read by any code path
in gamemd.exe (verified by string search):
- `RoughConnectTo`, `SandConnectTo`, `GreenConnectTo`, `PaveConnectTo` — **not read**.
  LAT exemptions are hardcoded in `ApplyLAT_and_SlopeFixup` as listed in §4.3.

---

## 9. Integration Points

### 9.1 Callers of `CellClass::RecalcAttributes` (30+)

- `MapClass::InitCellAttributes` — load-time pass
- Bridge: `ToggleBridgePavement`, `SelectDestroyedBridgeTile_Low`, `SelectBridgeTileVariant_Low`
- Overlay place/remove (Ore, tiberium, crates)
- Building place/sell (pavement under foundation)
- AoE damage cell updates
- Terraform / cell height change
- Wall connect changes

### 9.2 Callers of global tile-id globals

- `g_ClearTile` / `g_RoughTile` / `g_SandTile` / `g_GreenTile` / `g_PaveTile` — LAT base
- `g_ShorePieces` — `CellClass::IsShorePieceTile` (shore ramps, naval landing detection)
- `g_BridgeSet` / `g_WoodBridgeSet` — `CellClass::IsBridge` / `IsWoodBridge` (pathfinding height)
- `g_WaterSet` / `g_WaterBridge` — `IsOnBridgeSurface` (amphibious naval pathing)
- `g_PavedRoads` / `g_Medians` — LAT Pave exemption (prevents Pave LAT from tiling over paved roads)

### 9.3 Tile rendering

- `TMP_TileBlitter` @ `0x00547CF0` — per-tile blit (not decompiled this pass; primary tile draw)
- `FUN_00547110` — per-frame clear of `+0x308` flag across all tiles + variants
- `FUN_00544C80` — on-demand TMP reload (`TMP_Loader` if pointer null and persistent-file flag set)

---

## 10. Current Rust Implementation Status

Mapping of Ghidra findings to `src/`:

| Binary system | Rust file | Status |
|---------------|-----------|--------|
| `IsometricTileTypeClass` struct | — | **Missing.** Rust uses flat `tile_id: i32` + `TileKey { tile_id, sub_tile, variant }` in `src/render/tile_atlas.rs`. No type-class object; properties are stored piecemeal on `TilesetLookup` / `ResolvedTerrainCell`. |
| TileSet INI parse | `src/map/theater.rs::parse_tileset_ini` | **Partial.** Reads `FileName`, `SetName`, `TilesInSet` only. **Missing:** `LastTilesInSet`, `MarbleMadness`, `NonMarbleMadness`, `Morphable`, `AllowToPlace`, `AllowBurrowing`, `AllowTiberium`, `RequiredForRMG`, `ToSnowTheater`, `ToTemperateTheater`, `ShadowCaster`, `ShadowTiles`, `Tile%dAnim/XOffset/YOffset/AttachesTo/ZAdjust`. |
| `[General]` section parse | `src/map/theater.rs::load_theater` | **Partial.** Reads `BridgeSet`, `WoodBridgeSet`. **Missing ~47 other keys** (all the LAT/ramp/shore/waterfall/tunnel/bridge-variant globals). |
| `g_TileSet_StartTileIds` | `TilesetLookup` | **Present** — `tileset_index(tile_id) -> usize` lookup exists. |
| TMP binary parse | `src/assets/tmp_file.rs`, `src/assets/tmp_decode.rs` | **Complete** for pixel/depth/extra-data. Cell `terrain_type` + `ramp_type` bytes at `+0x29` / `+0x2A` match the binary exactly. |
| Variant (a/b/c/d) loading | `src/map/theater.rs::variant_filenames` | **Partial.** Loads `a/b/c/d` suffix TMPs but does not populate a variant chain and does not enforce random selection at draw time. |
| `CellClass::RecalcAttributes` | `src/map/resolved_terrain.rs` (partial land_type derivation) | **Missing the RecalcAttributes orchestration.** Current code derives land_type once at load from TMP metadata. No runtime recompute on cell mutation. |
| `CellClass::ApplyLAT_and_SlopeFixup` — LAT | `src/map/lat.rs::apply_lat` | **Partial, wrong exemptions.** Current Rust builds LAT from `*ConnectTo` INI keys (which YR doesn't read) instead of the **hardcoded exemption ranges** from §4.3. See `src/map/lat.rs` `LatExemptions` — the Pave exemption should be `[MiscPaveTile..+0xD]`, `[Medians..+0xD]`, `[PavedRoads..+0x14]`, and Green should be `[ShorePieces..+0x29]`, `[WaterBridge..+1]`. |
| LAT mask-to-tile mapping | `src/map/lat.rs::MASK_TO_LAT_INDEX` | **Likely wrong.** Binary uses `mask` directly (1..15) — no remapping — and neighbor bit assignment is N=0, E=1, S=2, W=3. Rust's `MASK_TO_LAT_INDEX` table needs verification against this. |
| Slope / ramp fixup | — | **Missing.** No code for RampBase/RampSmooth selection based on neighbor flatness. |
| MarbleMadness tile swap | — | **Missing.** No MM option, no `+0x298` lookup at draw time. |
| Per-tile animations (Tile%dAnim) | — | **Missing.** No attached anim spawning in `RecalcAttributes`. |
| Bridge tile variant selection | — | **Missing.** `SelectBridgeTileVariant_Low` logic (21 bridge variants based on 8-neighbor surface mask + odd/even chain counter) not ported. |
| Tube-tunnel entry (LandType=10 + IsoTileTypeIndex in Tunnels/TrackTunnels/DirtTunnels/DirtTrackTunnels) | — | **Missing.** TubeClass construction side-effect in RecalcAttributes not present. |

---

## 11. Resolved: Variant Selection Formula (deterministic, lockstep-safe)

**Function:** `CellClass::GetTileVariantIndex` @ `0x004814F0` (Ghidra label: `FUN_004814f0`).
**Call sites:** 4 — `CellClass__GetRadarColor` @ `0x0047C060`, `CellClass__GetRadarPixelColor` @ `0x0047BDB0`, `CellOverlay_TileDraw` @ `0x00480350`, `FUN_00546da0` @ `0x00546DA0` (corrected 2026-05-28: was "only 2 — `FUN_00480180` (radar-tile draw) at `0x004801DE` and `CellOverlay_TileDraw` @ `0x00480499`"; `FUN_00480180` does NOT call this function; `CellOverlay_TileDraw` entry is `0x00480350` not `0x00480499`; confirmed via `get_function_callers 0x004814F0` — INFERENCE_HARDENED + GHIDRA_ADDRESS_SHIFT). Signature:
`GetTileVariantIndex(this: CellClass*, tile_id: int, variant_count: int) -> u32`.

### 11.1 Algorithm

```python
def get_tile_variant_index(cell, tile_id, variant_count):
    # One-time lazy init of 8x8 permutation table DAT_0089e620[64]
    # (built from Random_Next() — game's deterministic PRNG, lockstep-safe)
    if not DAT_0089e7c7:
        DAT_0089e7c7 = 1
        DAT_0089e620[0..64] = 0xFFFFFFFF
        for i in range(64):
            while True:
                r = Random_Next() & 7          # 0..7
                # Reject-sample against neighbor table DAT_0081cce8 (8 neighbor deltas)
                # to avoid adjacent-cell collisions
                neighbor_clash = False
                for (dx, dy) in DAT_0081cce8:   # 8 pairs of (dx, dy)
                    j = (i + dy * 8 + dx) % 64
                    if DAT_0089e620[j] == r:
                        neighbor_clash = True
                        break
                if not neighbor_clash or retries == 0x40:
                    break
            DAT_0089e620[i] = r

    # Hash on cell coords (CellClass + 0x24 packs X,Y as short[2])
    x = cell.MapCoord_X                         # short
    y = cell.MapCoord_Y                         # short

    # Spatial shift for multi-cell tiles, if cell is a sub-tile
    if cell.SubTileIndex != 0:                  # CellClass + 0x11A
        tmp = g_IsoTileTypeArray[tile_id].tmp_data
        if tmp is None: return 0
        w = tmp.cells_wide                      # tmp[0]
        h = tmp.cells_tall                      # tmp[1]
        x = (x - (cell.SubTileIndex % w)) // w
        y = (y - (cell.SubTileIndex // w)) // h

    if variant_count > 4:
        return DAT_0089e620[(x & 7) + (y & 7) * 8]
    else:
        return DAT_0081cca8[((y & 3) << 2) | (x & 3)]    # small-variant 4x4 table
```

### 11.2 The small-variant table (verified by memory read)

`DAT_0081cca8[16]` at `0x0081CCA8` — raw bytes `00 01 02 03 03 02 01 00 02 03 00 01 01 00 03 02`:

| `(y&3)` ↓ \ `(x&3)` → | 0 | 1 | 2 | 3 |
|---|---|---|---|---|
| 0 | 0 | 1 | 2 | 3 |
| 1 | 3 | 2 | 1 | 0 |
| 2 | 2 | 3 | 0 | 1 |
| 3 | 1 | 0 | 3 | 2 |

Perfect 4×4 Latin square (each row and column contains {0,1,2,3} once) — guarantees no
two adjacent cells get the same variant for tiles with ≤4 variants.

### 11.3 Damaged-variant override path

In `FUN_00546da0` (map-load variant-pick pass):

```
if TMP_cell.flags & 0x04:                       # FLAG_HAS_DAMAGED_DATA
    variant = (cell.Flags >> 13) & 1            # CellClass.Flags bit 13 (0x2000)
else:
    variant = GetTileVariantIndex(cell, tile_id, variant_count)
```

So a bridge / damageable tile with damaged-data baked into its TMP uses CellClass.Flags
bit 0x2000 ("show damaged state") as a binary variant selector, bypassing the PRNG
picker.

### 11.4 Determinism guarantees

- `Random_Next` is the game's deterministic seeded PRNG (sync'd across lockstep clients).
- The permutation table is built on first call, with deterministic inputs → every
  client computes an identical table.
- The per-cell variant is a pure function of `(cell_x, cell_y, cell.SubTileIndex,
  tile_w, tile_h, variant_count)` → identical across clients.
- **Safe for lockstep multiplayer.**

---

## 12. Resolved: TMP Cell Flags (byte offset `+0x24`, u32)

From TMP cell header and FUN_00546da0 interpretation, the standard TMP cell layout
includes a flags dword at +0x24 with bits (already in `src/assets/tmp_file.rs`):

| Bit | Mask | Meaning |
|-----|------|---------|
| 0 | `0x01` | `FLAG_HAS_EXTRA_DATA` — cell has extra pixel data past the diamond |
| 1 | `0x02` | `FLAG_HAS_Z_DATA` — cell has per-pixel Z-depth data |
| 2 | `0x04` | `FLAG_HAS_DAMAGED_DATA` — cell has a baked damaged variant, selector via `CellClass.Flags & 0x2000` |

Standard TMP cell header (for reference, byte offsets within each cell struct):

| Offset | Type | Name |
|--------|------|------|
| `0x00` | `i32` | X |
| `0x04` | `i32` | Y |
| `0x08` | `i32` | extra_offset |
| `0x0C` | `i32` | z_offset |
| `0x10` | `i32` | extra_z_offset |
| `0x14` | `i32` | extra_x |
| `0x18` | `i32` | extra_y |
| `0x1C` | `u32` | extra_width |
| `0x20` | `u32` | extra_height |
| `0x24` | `u32` | **flags** (3 bits used) |
| `0x28` | `u8` | Height |
| `0x29` | `u8` | `terrain_type` (index into LandType table) |
| `0x2A` | `u8` | `ramp_type` / slope_index (0–8) |
| `0x2B..0x2D` | `u8[3]` | radar_color_left (BGR) |
| `0x2E..0x30` | `u8[3]` | radar_color_right (BGR) |

---

## 13. Resolved: LandType Lookup Table (`DAT_008288E4`)

Read from memory directly — 16 consecutive u32 entries (64 bytes), indexed by the
TMP cell's `terrain_type` byte (`+0x29`):

| `terrain_type` | LandType | Name |
|---------------|----------|------|
| 0 | 0 | Clear |
| 1 | 8 | Ice |
| 2 | 8 | Ice |
| 3 | 8 | Ice |
| 4 | 8 | Ice |
| 5 | 10 | Wall/Bridge |
| 6 | 9 | Railroad |
| 7 | 3 | Rock |
| 8 | 3 | Rock |
| 9 | 2 | Road |
| 10 | 6 | Weeds |
| 11 | 1 | Rough |
| 12 | 1 | Rough |
| 13 | 0 | Clear |
| 14 | 7 | Tiberium (Ore) |
| 15 | 3 | Rock |

The terrain_type byte is **not** a LandType; it is an intermediate opcode that maps
through this 16-entry table. Multiple terrain_type values can map to the same
LandType (e.g., 4 ice codes all → LandType 8). Values ≥16 are out-of-range and
would read garbage — TMP assets never contain values above 15.

---

## 14. Resolved: MarbleMadness draw-time behavior

`TMP_TileBlitter` @ `0x00547CF0` was fully decompiled. It takes the tile type pointer
(`param_1`) and a variant index (`param_14`) and does NOT read field `+0x298` itself.
The variant-chain walk at the top of the blitter is:

```
if variant_index != 0:
    if variant_index > tile->variant_count - 1:
        variant_index %= tile->variant_count
    while variant_index > 0:
        tile = tile->next_variant        # +0x2BC (field 0xAF)
        variant_index -= 1
    param_1 = tile
```

**MarbleMadness swap is NOT applied in the blitter.** The swap must happen in the
higher-level caller that resolves `CellClass.IsoTileTypeIndex → IsometricTileTypeClass*`.
Most likely the MM option gates whether `tile_ptr = g_IsoTileTypeArray[cell.tile_id]`
vs. `tile_ptr = g_IsoTileTypeArray[cell.tile_id]->MarbleMadness_tile_id` at the cell
resolution site. Not critical for baseline parity; flagged for future verification when
implementing the MarbleMadness toggle.

---

## 15. Resolved: Variant Pre-pick Pass (`FUN_00546da0`)

Called once during scenario load (before the main tile-draw loop). Two-phase routine:

### Phase 1 (`param_1 == 0`): per-cell variant pre-pick + use-count

```python
for cell in map.cells():
    if cell.IsoTileTypeIndex == 0xFFFF or cell.IsoTileTypeIndex >= tile_count:
        # Missing or out-of-range → fallback to ClearTile with its variant count
        tile = g_IsoTileTypeArray[g_ClearTile]
        variant = CellClass::GetTileVariantIndex(cell, g_ClearTile, tile.variant_count)
    else:
        tile = g_IsoTileTypeArray[cell.IsoTileTypeIndex]
        if tile.variant_count > 1:
            tmp_cell = tile.tmp_data.cells[cell.SubTileIndex % (w*h)]
            if tmp_cell and (tmp_cell.flags & 0x04):
                variant = (cell.Flags >> 13) & 1        # damaged-data path
            else:
                variant = CellClass::GetTileVariantIndex(cell, cell.IsoTileTypeIndex, tile.variant_count)

    # Clamp and walk variant chain
    variant %= tile.variant_count
    picked_tile = tile
    for _ in range(variant):
        picked_tile = picked_tile.next_variant     # +0x2BC
    picked_tile.use_counter += 1                   # +0x308 (field 0xC2)
```

### Phase 2 (always runs): TMP_Loader cull

Iterates every tile (and every variant in each chain). For each, if it has a persistent
filename stored at `+0x2F5`:
- If `param_1==0` AND `use_counter == 0` AND (`param_2==0` OR `tile.RequiredForRMG==0`):
  **free the TMP data** (`+0xA4`) — this tile is unused on this map.
- Else if `+0xA4 == 0`: load the TMP via `TMP_Loader` — this tile IS used.

This is the load-time memory-optimization pass: only TMPs referenced by at least one
cell (plus all RequiredForRMG tiles) are kept resident. Reports the bytes consumed via
`Register_heap_pool("Loaded %d isometric tiles consum…", count, KB)`.

---

## 16. Resolved: Neighbor-Reject Table `DAT_0081CCE8`

Read from memory as 16 `u32` values = 8 (dx, dy) pairs. Used in the permutation-table
initialization of `GetTileVariantIndex` to avoid adjacent-cell collisions:

| Index | dx | dy | Direction |
|-------|----|----|-----------|
| 0 | -1 | -1 | NW |
| 1 | 0 | -1 | N |
| 2 | +1 | -1 | NE |
| 3 | -1 | 0 | W |
| 4 | +1 | 0 | E |
| 5 | -1 | +1 | SW |
| 6 | 0 | +1 | S |
| 7 | +1 | +1 | SE |

All 8 Moore neighbors (full ring minus self). The permutation-table init tries up to
0x40 rejections to find a variant value that doesn't collide with any of these 8
neighbors in the 8×8 table before accepting.

---

## 17. Resolved: The Draw Path (`CellOverlay_TileDraw` @ `0x00480350`)

### 17.1 Call chain

- Main map render: `iso_to_screen` @ `0x006D77FF` → `CellOverlay_TileDraw`
- Scroll redraw: `FUN_006D7F20` (two call sites) → `CellOverlay_TileDraw`
- Also: `FUN_00480180` calls `TMP_TileBlitter` directly (variant=0 hardcoded; does NOT call `GetTileVariantIndex` — corrected 2026-05-28 from earlier implication; confirmed via `decompile_function 0x00480180`)

### 17.2 Algorithm

```python
def CellOverlay_TileDraw(cell, screen_pos, clip_rect, radar_only):
    if radar_only: return

    # Lazy-init of LightConvert for this cell if not yet set
    if cell.LightConvert == 0:    # CellClass + 0x34
        CellClass.SetLighting(0, 0x10000, 0, 1000, 1000, 1000)

    # Resolve tile + pick variant
    if cell.IsoTileTypeIndex == 0xFFFF:
        sub_tile = 0
        tile = g_IsoTileTypeArray[g_ClearTile]
        variant_count = tile.VariantCount
        tile_id = g_ClearTile
        variant = GetTileVariantIndex(cell, tile_id, variant_count)
    else:
        tile = g_IsoTileTypeArray[cell.IsoTileTypeIndex]
        sub_tile = cell.SubTileIndex            # +0x11A
        if tile.VariantCount < 2:
            variant = 0
        elif HasDamagedVariantAtSubTile(tile, sub_tile):    # FUN_005471F0
            variant = (cell.Flags >> 13) & 1    # damaged-data override
        else:
            variant = GetTileVariantIndex(cell, cell.IsoTileTypeIndex, tile.VariantCount)

    g_TileDrawCount += 1   # DAT_00A83ECC
    screen_x = screen_pos[0]
    screen_y = screen_pos[1] - cell.Level * 15  # +0x11B, 15 px per Z-step

    tmp_data = tile.get_tmp_data()   # vtable +0x9C
    if tmp_data:
        TMP_TileBlitter(
            cell.LightConvert,           # +0x34 - palette-shading table
            sub_tile,
            g_PrimarySurface,
            screen_x,
            screen_y + g_RadarViewportOffsetY,
            clip_rect[0], clip_rect[1], clip_rect[2], clip_rect[3],
            cell.Level,                  # +0x11B
            cell.ZAdjust_Base,           # +0x10C (short) — NOT a variant cache
            1,
            variant,                     # computed above
            0, 0, 0, 0
        )

    # Overlay draw (ore, smudge, etc.)
    if cell.OverlayTypeIndex != -1:
        vtable_call(g_OverlayTypeArray[cell.OverlayTypeIndex],
                    offset=0xA0,
                    args=(local_coords, clip_rect,
                          cell.OverlayState,      # +0x11F
                          cell.Level * PIXELS_PER_Z_STEP,
                          &cell.MapCoord))
```

### 17.3 CellClass field correction (cross-referenced with `CELLCLASS_STRUCT_GHIDRA_REPORT.md`)

An earlier section of this doc implied `CellClass + 0x10C` might be a cached variant
short. **That is wrong.** Verified against `CELLCLASS_STRUCT_GHIDRA_REPORT.md` and
`CellClass::Constructor` @ `0x0047BBF0`:

| Offset | Size | Field | Default |
|--------|------|-------|---------|
| `+0x24` | `i16×2` | MapCoord (X, Y) | sentinel at init |
| `+0x34` | `ptr` | **LightConvert\*** (palette-shading table, refcounted +0x194) | 0 (lazy-init on first draw) |
| `+0x38` | `i32` | IsoTileTypeIndex | `0xFFFF` (empty/clear) |
| `+0x104` | `u32` | Ambient level | `0x10000` |
| `+0x108` | `u16` | (unknown — init 0 in constructor via `param_1[0x42]=0`) | `0` |
| `+0x10A` | `u16` | Red tint | `1000` |
| `+0x10C` | `u16` | **ZAdjust_Base** — passed to `TMP_TileBlitter` as Z param; also used in `FUN_00480180` as ZAdjust (corrected 2026-05-28: was labeled "Green tint"; draw path `CellOverlay_TileDraw` passes `*(short*)(param_1+0x10C)` as ZAdjust arg to TMP_TileBlitter, confirmed via `decompile_function 0x00480350` and `0x00480180` — OFFSET_RETYPED_WRONG) | `1000` (init from constructor as tint slot, actual draw-time value set by lighting) |
| `+0x10E` | `u16` | Blue tint | `1000` |
| `+0x110/0x112/0x114` | `u16×3` | Derived/scaled RGB | `1000` |
| `+0x11A` | `u8` | **SubTileIndex** (cell's position in multi-cell TMP) | 0 |
| `+0x11B` | `u8` | **Level** (Z / terrain elevation steps) | 0 |
| `+0x11F` | `u8` | Overlay draw state | 0 |
| `+0x140` | `u32` | Flags bitmask (bit 13 = "show damaged") | 0 |

**The variant index is NOT cached on the cell** — it is recomputed on every draw by
`GetTileVariantIndex(cell, tile_id, variant_count)`. Because that function is a pure
hash over `(MapCoord, SubTileIndex, tile dimensions, variant count)`, recomputation is
~free and produces an identical result every call → deterministic and cheap.

### 17.4 TMP_TileBlitter arg-mapping caveat

Ghidra's auto-generated signature for `TMP_TileBlitter` types `param_1` as
`int*` and types its uses as variant-chain walks (`param_1[0xAF]`, `param_1[0xBC]`,
vtable `+0x9C`). However the caller passes `CellClass + 0x34` (a `LightConvert*`) in
that slot, and the existing `CELLCLASS_STRUCT_GHIDRA_REPORT.md` confirms that field
holds the shading table, not a tile type.

**Resolution:** the decompile of `TMP_TileBlitter`'s variant-chain block is almost
certainly being reused from a different calling convention (or a mis-resolved
multi-arg shim). Functional behavior observed from callers shows the variant is
*already resolved* before `TMP_TileBlitter` is entered (see §17.2: `_param_4 = variant`
is computed in `CellOverlay_TileDraw` and passed explicitly as arg 13), so the
chain-walk block inside the blitter must either:
  1. Be dead code in the production draw path (variant always passed as 0 when
     param_1 is LightConvert, and > 0 when param_1 is some other object), or
  2. Operate on an argument other than `param_1` that Ghidra lost track of.

This does not affect our understanding of variant semantics — the selection happens
in the caller and is deterministic. It only means that if/when we port the blitter,
we should derive its true arg layout from the assembly rather than Ghidra's guess.
This also means the real tile-type pointer to draw is resolved at a call-site level
(via `g_IsoTileTypeArray[cell.IsoTileTypeIndex]`) and passed into the blitter through
a different argument than Ghidra labels as `param_1`.

---

## 18. Remaining Open Questions

1. **MarbleMadness draw-time swap site.** Confirmed NOT in `TMP_TileBlitter`. The MM
   option likely gates whether `tile_ptr = g_IsoTileTypeArray[cell.tile_id]` vs.
   `tile_ptr = g_IsoTileTypeArray[tile.MarbleMadness_tile_id]` at the tile resolution
   site. Could live inside `iso_to_screen` or in the blitter's real arg derivation
   (§17.4). Not critical for baseline parity — MM is a cosmetic skirmish option.

2. **`TMP_TileBlitter` real argument layout.** Ghidra's auto-sig is inconsistent with
   the caller — needs a hand-pass over the blitter prologue/epilogue to recover the
   true stack frame. Not required for Rust implementation if we only port the
   callers (CellOverlay_TileDraw, TileDraw_radar) and write our own blit kernel.

3. **`RecalcAttributes` runtime trigger coverage.** The 30+ callers include many
   subtle paths (IQ-based passability checks, zone rebuilding). Need a systematic
   audit for the Rust port to know which mutations must retrigger LAT.

4. **Bridge variant selection detail** (`SelectBridgeTileVariant_Low`,
   `SelectDestroyedBridgeTile_Low`) — decompiled but not analyzed in depth. Bridge
   render/damage state is a separate investigation — see
   `HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md`.

5. **Shadow tile radar-color computation** in `FUN_00549E90` uses 13 pre-brightened
   color slots (`fStack_18 < 0xd`). The exact brightness curve was not analyzed.

---

## Sources

**Ghidra addresses decompiled this session:**
- `0x005447C0` — IsometricTileTypeClass::Constructor (6-arg)
- `0x00544A70` — IsometricTileTypeClass destructor
- `0x00544BE0` — GetLandTypeAtCell (FUN)
- `0x00544C20` — IsCellValid (FUN)
- `0x00544C80` — EnsureTMPLoaded (FUN)
- `0x00544CE0` — IsometricTileTypeClass::FindByName
- `0x00544E70` — LightConvertClass::GetOrCreate
- `0x00545150` — Theater_ReadINI_LoadTileSets (mislabeled CDFileClass__Constructor)
- `0x00546DA0` — ScenarioClass::PickTileVariants_and_CullUnusedTMPs (FUN)
- `0x00547020` — TMP_Loader
- `0x00547110` — clear per-frame tile flags
- `0x005471B0` — TMP_ReadSlopeType
- `0x005471F0` — IsometricTileTypeClass::HasDamagedVariantAtSubTile (FUN)
- `0x00547CF0` — TMP_TileBlitter (Ghidra sig is inconsistent with caller — see §17.4)
- `0x00549E90` — build shadow/radar color tables
- `0x00480180` — tile draw dispatcher (radar-path)
- `0x00480350` — CellOverlay_TileDraw (main draw entry)
- `0x00483E30` — CellClass::SetLighting (FUN)
- `0x004814F0` — CellClass::GetTileVariantIndex (deterministic variant picker)
- `0x0047BBF0` — CellClass::Constructor (layout verification)
- `0x0047CA80` — CellClass::ApplyLAT_and_SlopeFixup
- `0x0047D2B0` — CellClass::RecalcAttributes
- `0x00568BB0` — MapClass::InitCellAttributes
- `0x0054A170` — IsometricTileTypeClass scalar destructor wrapper
- `0x0057ACF0` — MapClass::SelectBridgeTileVariant_Low (partial, reference only; corrected 2026-05-28 from `0x0057B133` — GHIDRA_ADDRESS_SHIFT)

**Memory tables dumped this session:**
- `0x008288E4` — LandType lookup table (16 × u32, verified complete)
- `0x0081CCA8` — Small-variant 4×4 Latin square (16 × u32, verified complete)
- `0x0081CCE8` — Neighbor-reject (dx, dy) table (8 pairs, full Moore ring, verified)

**Cross-referenced docs:**
- `CELLCLASS_STRUCT_GHIDRA_REPORT.md` — confirmed `CellClass+0x34 = LightConvert*`,
  `+0x108 = ZAdjust_Base`, which overrides this doc's earlier draft interpretation.

**Strings verified:**
- `0x00829208` `AllowTiberium`
- `0x00829218` `AllowBurrowing`
- `0x00829228` `AllowToPlace`
- `0x00829238` `Morphable`
- `0x00829244` `NonMarbleMadness`
- `0x00829258` `MarbleMadness`
- `0x00829268` `FileName`
- `0x0082927C` `SetName`
- `0x0082928C` `LastTilesInSet`
- `0x0082929C` `TilesInSet`
- `0x008291B8` `ShadowTiles`
- `0x008291C4` `ShadowCaster`
- `0x008291D4` `ToTemperateTheater`
- `0x008291E8` `ToSnowTheater`
- `0x008291F8` `RequiredForRMG`
- Plus the 49 `[General]` keys at `0x008295xx` range.

**Globals identified:**
- `DAT_00a8ed28` — IsoTileTypeArray vector class instance pointer
- `DAT_00a8ed2c` — `g_IsoTileTypeArray` (IsometricTileTypeClass**)
- `DAT_00a8ed38` — `g_IsoTileTypeArray_Count`
- `DAT_00aa1140` — `g_TileSet_StartTileIds[]`
- `DAT_00abc558` — `g_TileSet_Count`
- `DAT_008288E4` — `g_TerrainType_to_LandType[]` (per-byte u32 lookup)
- `DAT_0089F688` — `g_DirectionOffsets[8]` (8-dir cell coord deltas)

**Docs cross-referenced:**
- `CELLCLASS_STRUCT_GHIDRA_REPORT.md` — CellClass offsets (+0x38 IsoTileTypeIndex, +0xEC LandType, +0x11C SlopeIndex confirmed)
- [CELLCLASS_ZONES_SPEED_BRIDGES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/CELLCLASS_ZONES_SPEED_BRIDGES.md) — bridge tile ranges
- `HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md` — bridge damage
- `2026-04-23-gap-scan-xref.md` — flagged LAT rewriter as deferred
- `2026-04-21-gap-scan-classes.md` — flagged IsometricTileTypeClass field coverage as thin

**INI files checked:**
- `ini/rulesmd.ini`, `ini/artmd.ini` — no TileSet definitions (confirmed; tile data lives in theater INIs)
- `ini/temperatmd.ini` (82 TileSets), `ini/snowmd.ini` (83), `ini/urbanmd.ini` (111) — exact TileSet counts verified from content scan

**Rust source audited:**
- `src/assets/tmp_file.rs`, `src/assets/tmp_decode.rs` — TMP parser
- `src/map/theater.rs` — TileSet + theater INI loader
- `src/map/lat.rs` — current LAT implementation (uses `*ConnectTo`; should use hardcoded exemptions)
- `src/map/resolved_terrain.rs` — land_type derivation
- `src/map/map_file.rs` — IsoMapPack5 parser
- `src/render/tile_atlas.rs` — tile atlas
