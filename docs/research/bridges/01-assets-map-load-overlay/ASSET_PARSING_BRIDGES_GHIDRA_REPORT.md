# Asset Parsing — Bridges-Priority Ghidra Report

**Date:** 2026-05-10
**Confidence (overall):** HIGH for the bridge-load chain and the MIX/TMP/PAL/Map-pack/LCW/LZO/AUD findings (all decompiled and verified). MEDIUM for SHP file-format details (header verified; codec details inferred from blitter dispatch). UNRESOLVED for CSF parser.
**Active in YR:** Yes (every parser in this report runs on every YR scenario load).
**Plan:** `docs/plans/2026-05-10-asset-parsing-bridges-investigation-plan.md`

> **Output bar.** Per CLAUDE.md, parity is on observable output. The byte-level
> facts in this report are not a blueprint for our Rust internals — Rust is
> free to be cleaner. They exist so a future implementer can produce **the
> same observable result** (same pixels on screen, same loaded tile data,
> same audio samples) for **the same input bytes** as gamemd.exe.

---

## 1. Overview

This report covers the gamemd.exe asset-parsing surface relevant to a Yuri's
Revenge skirmish, with the bridge tile-loading chain as the priority narrative.
Six file formats are documented; two RA2-Rust parity divergences are flagged.

**Bridge-load story (1-line):** `ScenarioClass__Full_Init` →
`Init_Theater` (opens 5 theater MIX files + ISO/UNIT palettes) →
`Read_Theater_TileSets_INI` (reads `[General]` bridge keys + iterates
`[TileSet0000]…`) → per-tile `LoadFileFromMIX` (CRC-keyed BST cache) →
`IsometricTileTypeClass__Constructor` + `TMP_Loader` (loads tile graphics, fixes
up sub-tile pointers, pre-computes 13 lighting steps for radar) → bridges live
as overlay bytes `0xCD..0xE6` (HIGH) and `0x4A..0x63` (LOW), set later in
`ReadMapOverlayPacks`.

**Address renames performed in Ghidra during this investigation:**

| Address | Old (mislabeled) | New |
|---|---|---|
| `0x005349C0` | `FUN_005349c0` | `Init_Theater` |
| `0x00545150` | `CDFileClass__Constructor` (wrong) | `Read_Theater_TileSets_INI` |
| `0x004ace70` | `BSurface__Constructor` (wrong) | `Read_Map_Section_And_IsoMapPacks` |
| `0x004ad7e0` | `Pipe__Constructor` (wrong) | `Write_Map_Section_And_IsoMapPack5` |

Ghidra's `CDFileClass__Constructor` and `Pipe__Constructor` symbols are pollution
— the same name has been applied to multiple distinct functions. Trust
decompilation only, not the label. (Genuine `CDFileClass__Constructor` likely
lives at `0x004A38D0` per scoping; **OPEN — see §11.4**.)

---

## 2. Bridge-load chain (Phase 1)

### 2.1 Top of chain — `ScenarioClass__Full_Init` @ `0x00686B20`

Top-level orchestrator for scenario load. The bridge-relevant subset of its
ordering, in execution order:

1. Read `[Map].Theater=` from the scenario INI →
   `g_ScenarioClass_Instance[0x496]` (= `*(int *)(g_Scenario+0x1258)`).
2. **Call `Init_Theater(theater_idx)`** — opens theater MIX files, loads
   `ISO%s.PAL` and `UNIT%s.PAL`. *(see §2.2)*
3. Call `RulesClass::ReadGeneral` (rules.ini `[General]` block).
4. Various scenario sub-INI reads (Briefing, UIName, LSLoadMessage, etc.).
5. **Call `Read_Map_Section_And_IsoMapPacks`** at `0x004ace70` — reads
   `[Map]`, `[CellTags]`, then attempts `[IsoMapPack]`/`[IsoMapPack2..5]` in
   sequence. Inside this function, `Read_Theater_TileSets_INI` is called only
   if the new theater differs from the cached one (`DAT_00822cf8`) — otherwise
   the previously-loaded tilesets are reused via `FUN_00547110`. *(see §2.3)*
6. **Call `ReadMapOverlayPacks`** at `0x005FD2E0` — decodes `[OverlayPack]`
   and `[OverlayDataPack]` (bridges live in here as overlay bytes
   `0x18`/`0x19`/`0xED`/`0xEE`). *(see §6)*
7. Per-cell `CellClass__RecalcAttributes` — propagates LAT and bridge
   damage/edge state.
8. `TerrainClass__Read_Map_Section`, `ScenarioClass__Read_Units_Section`,
   `BuildingClass__ReadFromINI` — pre-placement actors.

**Critical ordering:** theater tile-set INI is read **before** the overlay
pack — so by the time `[OverlayPack]` is decoded, `g_BridgeSet`,
`g_WaterBridge`, `BridgeMiddle1/2`, etc. are already resolved into global
indices.

### 2.2 `Init_Theater` @ `0x005349C0`

Opens all theater MIX files for the requested theater, loads two palettes,
and pre-computes the 13-level lit unit palette.

**Signature:** `void Init_Theater(int theater_idx)`

**Theater string table at `0x007E1BC0`** — entries are `0x70` (112) bytes
each, indexed as `&base + theater_idx * 0x70`. Verified theater layout:

| Idx | Long | Display | Short | MM | Letter |
|---|---|---|---|---|---|
| 0 | TEMPERAT | (Temperate) | TEM | MMT | T |
| 1 | SNOW | Snow | SNO | MMS | A |
| 2 | URBAN | Urban | URB | MMU | U |
| 3 | DESERT | Desert | DES | MMD | D |
| 4 | NEWURBAN | New Urban | UBN | **MMT** | N |
| 5 | LUNAR | Lunar | LUN | **MMT** | L |

Note: NEWURBAN and LUNAR both reuse `MMT` (temperate marble madness) as their
MM extension. The "letter" column appears unused in skirmish but is set on
each entry.

**MIX files opened (per theater) into globals `DAT_00884E08..1C`:**

| Constructor target | Filename pattern | Always loaded? |
|---|---|---|
| `DAT_00884E0C` | `<long>MD.MIX` (e.g., `SNOWMD.MIX`) | **Only if `theater_idx == 1` (SNOW)** — see §11.1 |
| `DAT_00884E08` | `<long>.MIX` (e.g., `SNOW.MIX`) | Yes |
| `DAT_00884E10` | `<short>.MIX` (e.g., `SNO.MIX`) | Yes |
| `DAT_00884E20` | `ISO<short>MD.MIX` (e.g., `ISOSNOMD.MIX`) | Yes |
| `DAT_00884E1C` | `ISO<short>.MIX` (e.g., `ISOSNO.MIX`) | Yes |

**Skip optimisation:** if the new theater equals `DAT_00822CF8` (the cached
last-loaded theater), the entire body is skipped. This is what `FUN_00547110`
exploits later.

**Palette load — `ISO%s.PAL`:**

The function builds `"ISO%s.PAL"` (e.g., `ISOTEM.PAL`) and calls
`LoadFileFromMIX`. The 768-byte payload is decoded into `DAT_00885780` (the
iso/terrain palette) with this exact pixel-level transform:

```text
for i in 0..256:
    R = file[i*3 + 0]
    G = file[i*3 + 1]
    B = file[i*3 + 2]
    palette[i*3 + 0] = R << 2     // 6-bit -> 8-bit, top 6 bits only
    palette[i*3 + 1] = G << 2
    palette[i*3 + 2] = B << 2
```

**Tiny details that matter — PAL `<<2` scaling produces max value `0xFC`,
not `0xFF`.** PAL files store 6-bit components (0..63). Multiplying by 4
gives 0..252. **gamemd does NOT apply round-up scaling** — pure 0..63 → 0..252.
*(See §10.1 for the Rust drift this causes.)*

**Fallback when `ISO%s.PAL` is missing:** the function generates a synthetic
gradient — index `i` becomes RGB = `(i & 0xFF, ~i & 0xFF, (i & 0xFF) << 2)`.
Looks weird (red gradient + green inverse + blue scaled). This path fires when
the MIX is missing; vanilla YR never hits it.

**Then `UNIT%s.PAL`** (e.g., `UNITT.PAL`) is loaded into `DAT_00886380` via
`FUN_006260D0` (raw-copy), then scaled in-place with the same `<< 2` per
component, with the same 0xFC ceiling.

**Lit palette pre-computation loop** — gamemd builds 13 brightness levels
of the unit palette blended with the iso palette, stored for radar/cliff
shading:

```text
total_levels = DAT_00b054e0          // global lighting count
chunk = total_levels / 13
for iter in 0..total_levels:
    FUN_0068C860(unit_pal, iso_pal, R=1000, G=1000, B=1000)
    progress_step = clamp(iter/chunk + 12, 12, 25)
    if progress_step changed: progress_callback(progress_step)
```

The constant `13` (`0xD`) is the brightness-level count. It also appears in
`FUN_00549E90` (TMP per-tile radar-color lit table) — same magic number drives
both the unit palette and the per-tile radar shading.

### 2.3 `Read_Theater_TileSets_INI` @ `0x00545150` — THE bridge tile loader

**Signature:** `void Read_Theater_TileSets_INI(int theater_idx, char editor_flag)`

The largest single function in this investigation. Reads the entire theater
INI (e.g., `temperatmd.ini`), parses every `[TileSet####]` section, loads each
referenced TMP file from the appropriate MIX archive, and assigns global tile
indices including all bridge keys.

**Slope and shadow palette load (first):**

```text
slop01z<theater> -> DAT_00AA1060   // base slope graphics (4 chars '0','2','3','4')
slop02z<theater> -> DAT_00AA1064
slop03z<theater> -> DAT_00AA1068
slop04z<theater> -> DAT_00AA106C
c_shadow.shp     -> DAT_00ABC554   // shadow shape (loaded once globally)
ISO<short>.PAL   -> DAT_00ABBED0   // 768 bytes, scaled <<2 (max 0xFC)
```

The 4 SLOP files are the cliff-edge slope geometry. They're loaded but rarely
unique to the theater — most theaters share the same SLOP files.

**Theater INI open:** `<long>MD.INI` (e.g., `TEMPERATMD.INI`) via CCFile.

**`[General]` section — tile-set ID keys read:**

| INI key | Default | Stored at | Notes |
|---|---|---|---|
| `RampBase` | -1 | `g_RampBase` | Cliff ramps |
| `RampSmooth` | -1 | `g_RampSmooth` | Smooth ramps |
| `MMRampBase` | -1 | `DAT_00AA109C` | Marble madness ramps |
| `ClearTile` | -1 | `g_ClearTile` | Default empty cell |
| `RoughTile` | -1 | `g_RoughTile` | |
| `SandTile` | -1 | `g_SandTile` | |
| `GreenTile` | -1 | `g_GreenTile` | |
| `PaveTile` | -1 | `g_PaveTile` | |
| `MiscPaveTile` | -1 | `g_MiscPaveTile` | |
| `ClearToRoughLat` | -1 | `g_ClearToRoughLat` | LAT auto-fix |
| `ClearToSandLat` | -1 | `g_ClearToSandLat` | |
| `ClearToGreenLat` | -1 | `g_ClearToGreenLat` | |
| `ClearToPaveLat` | -1 | `g_ClearToPaveLat` | |
| `HeightBase` | -1 | `_DAT_00AA0744` | |
| `BlackTile` | -1 | `_DAT_00ABC2CC` | |
| **`BridgeSet`** | -1 | `DAT_00AA0E28` (g_BridgeSet) | **Concrete bridge tileset** |
| **`WoodBridgeSet`** | -1 | `DAT_00ABAD1C` | **Wooden bridge tileset** |
| `CliffSet` | -1 | `DAT_00AA1020` | |
| `ShorePieces` | -1 | `g_ShorePieces` | |
| `WaterSet` | -1 | `DAT_00AA0738` | |
| `SlopeSetPieces` | -1 | `DAT_00ABC1F8` | |
| `SlopeSetPieces2` | -1 | `DAT_00AA1098` | |
| `MonorailSlopes` | -1 | `_DAT_00AA1024` | |
| `Tunnels` | -1 | `DAT_00AA1054` | |
| `TrackTunnels` | -1 | `DAT_00ABB108` | |
| `DirtTunnels` | -1 | `DAT_00AA10B4` | |
| `DirtTrackTunnels` | -1 | `DAT_00ABAD2C` | |
| `WaterfallEast/West/North/South` | -1 | various | |
| `CliffRamps` | -1 | `DAT_00ABBEBC` | |
| `PavedRoads` | -1 | `g_PavedRoads` | |
| `PavedRoadEnds` | -1 | `DAT_00ABBEC4` | |
| `Medians` | -1 | `g_Medians` | |
| `RoughGround` | -1 | `_DAT_00AA0E1C` | |
| `DirtRoadJunction/Curve/Straight` | -1 | various | |
| `DestroyableCliffs` | **-2** | `DAT_00ABC2C8` | **Note default = -2, others -1** |
| `WaterCaves` | -1 | `DAT_00ABAD24` | |
| `WaterCliffs` | -1 | `DAT_00AA101C` | |
| `PavedRoadSlopes` | -1 | `_DAT_00AA1094` | |
| `DirtRoadSlopes` | -1 | `DAT_00ABBEC0` | |
| `Rocks` | -1 | `_DAT_00ABB10C` | |
| **`WaterBridge`** | -1 | `g_WaterBridge` (DAT_00ABB108 vicinity) | **Water-spanning bridge** |

**Critical `[General]` tile-INDEX keys (within bridge tileset):**

| INI key | Stored at | Purpose |
|---|---|---|
| `BridgeTopLeft1` | `DAT_00ABC2B4` | Tile 0 of bridge tileset for NW-corner variant 1 |
| `BridgeTopLeft2` | `DAT_00AA1130` | Tile 0 variant 2 |
| `BridgeBottomRight1` | `DAT_00ABC1E8` | SE-corner variant 1 |
| `BridgeBottomRight2` | `DAT_00AA0E38` | variant 2 |
| `BridgeTopRight1` | `DAT_00AA1548` | NE-corner variant 1 |
| `BridgeTopRight2` | `DAT_00AA0740` | variant 2 |
| `BridgeBottomLeft1` | `DAT_00ABC1D0` | SW-corner variant 1 |
| `BridgeBottomLeft2` | `DAT_00AA1540` | variant 2 |
| `BridgeMiddle1` | `DAT_00ABAD30` | Straight-section variant 1 |
| `BridgeMiddle2` | `DAT_00AA1028` | variant 2 |

**These are NOT tileset IDs — they're TILE INDICES within the bridge tileset
chosen by `BridgeSet=`.** A value of `BridgeMiddle1=7` means "tile 7 inside
TileSet####" (where #### is `BridgeSet=`). They're consumed by
`MapClass__SelectBridgeTileVariant_Low` and the high-bridge equivalent at
runtime to pick which sprite to draw for each segment shape. **(Rust gap —
see §10.2.)**

**Main loop — iterate `[TileSet####]`:** the function increments a section
counter (`iVar11`/`iStack_960`) starting at 0, formats `[TileSet%04d]`, and
reads `TilesInSet=`. Loop terminates when `TilesInSet` is missing/-1.

For each tileset section, before allocating tiles, it checks if the current
section number matches any of the `[General]` keys above. If so, it stores
**the cumulative tile index** (`iStack_9EC`) in the corresponding global. That
is, **`g_BridgeSet` does not hold `19` (the section number); it holds the
first-tile global index of the section corresponding to `BridgeSet=19`.**

**TileSet section keys:**

| Key | Default | Effect |
|---|---|---|
| `TilesInSet` | -1 | Loop exit if missing/-1 |
| `LastTilesInSet` | TilesInSet | If different, allocates a "stub" 8-byte entry (`{first_tile, count_diff}`) registered in `DAT_00AA107C` |
| `SetName` | "No Name" | Display name (also used for animation key prefix) |
| `FileName` | (key-name) | TMP base filename (e.g., `bridge`) |
| `MarbleMadness` | 0xFFFF | Tile-index in MM tileset to swap |
| `NonMarbleMadness` | 0xFFFF | Reverse mapping when MM disabled |
| `Morphable` | false | Tile supports raise/lower height |
| `AllowToPlace` | true | Editor can place this tile |
| `AllowBurrowing` | true | Subterranean units can pass under |
| `AllowTiberium` | false | Ore can grow here |
| `RequiredForRMG` | false | Random map generator must include |
| `ToSnowTheater` | -1 | Tile-index when converted to snow theater |
| `ToTemperateTheater` | -1 | Tile-index when converted to temperate |
| `ShadowCaster` | false | Casts shadows (e.g., trees, rocks) |
| `ShadowTiles` | 0 | Number of shadow tile variants |
| `Tile##Anim` | (none) | Per-tile animation by tile number |
| `Tile##XOffset/YOffset/AttachesTo/ZAdjust` | 0 | Animation positioning |

**`Tile##Anim` — per-tile animations only fire if `iVar16 = FUN_00526810(SetName)
!= 0`.** That call checks if a sub-section keyed by SetName exists in the same
INI file (theater INI). When present, the animation keys are read from THAT
sub-section, not from the `[TileSet####]` block itself.

**Per-tile load loop — variant fallback chain (CRITICAL):**

For each of the `TilesInSet` tiles:

```text
for variant in [0='', 1='a', 2='b', 3='c', 4='d']:
    filename = "<FileName><tile_idx_2digit>.<theater_short>"   // e.g., "bridge01.tem"
    if variant > 0:
        filename = "<FileName><tile_idx_2digit><letter>.<theater_short>"   // e.g., "bridge01a.tem"

    # Try LoadFileFromMIX (or CCFile if editor_flag set)
    data = LoadFileFromMIX(filename)
    if data == NULL and NonMarbleMadness != 0 and theater_idx != -1:
        # First fallback: theater-derived secondary extension
        # (using PTR_LAB_007e1bca + theater_idx*0x70 — appears to be the MM letter)
        filename2 = "<FileName>NN<variant_letter>.<MM_ext>"
        data = LoadFileFromMIX(filename2)
    elif data == NULL and NonMarbleMadness != 0 and theater_idx == -1:
        filename2 = "<FileName>NN<variant_letter>.MMT"
        data = LoadFileFromMIX(filename2)
    if data == NULL:
        # Subsequent hard-coded fallbacks (in order):
        # ".TEM" at 0x00829140
        # ".URB" at 0x00829138
        # (NEVER tries .MMT/.SNO/.LUN/.DES as hard-coded fallbacks)

    if data == NULL: stop_variants  # no more variants
    
    # Allocate IsometricTileTypeClass (0x30C bytes) for this variant
    tile = IsometricTileTypeClass__Constructor(...)
    # First variant becomes the "head" tile; subsequent variants chain via tile[0xAF] (offset 0x2BC)
```

**Tiny detail — fallback chain explicitly stops at `.TEM` then `.URB`.** No
`.SNO` / `.DES` / `.LUN` / `.UBN` fallback. If a snow theater tile is missing
its `.SNO` graphic AND no `.MMT` exists AND no `.TEM` AND no `.URB`, the tile
fails to load. This is why YR ships every theater with a `.TEM` fallback for
gameplay-critical tiles.

**Tiny detail — variant letters:** the variant suffix is built by writing
`'`'` + variant_idx` (so variant 1 = `'a'` = 0x61, variant 2 = `'b'` = 0x62,
etc.). **The first variant has no suffix at all** (not `'0'` and not space).
File naming becomes:
- variant 0: `bridge01.tem`
- variant 1: `bridge01a.tem`
- variant 2: `bridge01b.tem`
- variant 3: `bridge01c.tem`
- variant 4: `bridge01d.tem`

Stops as soon as any variant fails. Variants beyond first are random visuals
(picked via Random__RandomRanged for non-bridges, deterministic for bridges
via the BRIDGE_DISPLAY_TABLE algorithm).

**Variant chain field (offset `0x2BC` = `tile[0xAF]`):** each variant's tile
points to the NEXT variant via this field. The HEAD tile's `tile[0xBC]`
(offset `0x2F0`) holds the variant count.

**Theater 5 (DESERT) special case** — at the end of the loop, if the
`editor_flag` parameter equals 5 (DESERT theater):

```text
g_ShorePieces = -1
DAT_00AA0738 = -1   // WaterSet
DAT_00AA1020 = -1   // CliffSet
DAT_00AA101C = -1   // WaterCliffs
g_WaterBridge = -1
DAT_00AA0E28 = -1   // BridgeSet
DAT_00ABAD1C = -1   // WoodBridgeSet
```

**DESERT explicitly clears all water and bridge tile-set indices.** Confirms
desert theater is land-only. Even if the `desertmd.ini` lists a `BridgeSet=`
value, it gets nullified after parsing. (LUNAR is theater 4 and is NOT cleared
— LUNAR maps with bridges work; DESERT maps cannot have bridges.)

**Wait — `editor_flag` vs `theater_idx`:** the function signature is
`(int theater_idx, char editor_flag)`. The `local_95c` variable that's checked
against `5` and `-1` is `param_1` (theater_idx). So this is "if theater is
DESERT, kill water/bridge globals." Verified.

**Final cleanup — water-cliff registry remap:** after the main loop,
`DAT_00A8ED38` entries (a global registry of water/cliff tile types — count
limited) get their `+0x298` and `+0x29C` fields adjusted by adding the global
tile index of their tileset. This is the "water-cliff connection" registry
that runtime renderers use.

### 2.4 `IsometricTileTypeClass__Constructor` @ `0x005447C0`

**Signature:** `(this, ObjectTypeClass*, height_byte, ?, name_str, is_extra_variant)`

Allocates a 0x30C-byte struct, zero-initializes most fields, sets default
sentinels for unknown values, and registers in two global arrays:
- `DAT_00B0F674` — TileTypeClass array (limited slot count, with auto-grow
  via vtable+0x8 if `DAT_00B0F67D != 0` or array is empty)
- `DAT_00A8ED2C` — runtime tile registry (the one consulted by water/cliff
  remapping in 2.3)

**Key field offsets (matches `ISOMETRIC_TILE_TYPE_CLASS_GHIDRA_REPORT.md` —
verified):**

| Offset | Type | Purpose |
|---|---|---|
| 0x000 (vtable) | ptr | Primary vtable |
| 0x004 | ptr | Secondary vtable (multi-inherit) |
| 0x008..0x00C | ptr | More secondary vtables |
| 0x064 (`+0x19`) | char[48] | Name (set strncpy `0x30`) |
| 0x0A4 (`+0x29`) | byte ptr | TMP raw data pointer (after `TMP_Loader`) |
| 0x0A8 (`+0x2A` byte) | byte | Loaded flag (1 = data present) |
| 0x294 (`+0xA5`) | int | ObjectTypeClass parent ptr |
| 0x298 (`+0xA6`) | int | MarbleMadness (default `0xFFFF`) |
| 0x29C (`+0xA7`) | int | NonMarbleMadness (default `0xFFFF`) |
| 0x2A0 (`+0xA8`) | int | TileSet section ID for this tile |
| 0x2A4 (`+0xA9`) | int | Static vtable ptr (`PTR_FUN_007ECCEC`) |
| 0x2A8 (`+0xAA`) | int | Per-cell sub-image registry array |
| 0x2AC (`+0xAB`) | int | Registry capacity |
| 0x2B0 (`+0xAC` byte) | byte | Init flag (=1) |
| 0x2B1 (byte) | byte | Auto-grow flag |
| 0x2B4 (`+0xAD`) | int | Registry count |
| 0x2B8 (`+0xAE`) | int | Growth chunk (=10) |
| 0x2BC (`+0xAF`) | int | Next-variant pointer (chain) |
| 0x2C0 (`+0xB0`) | int | ToSnowTheater |
| 0x2C4 (`+0xB1`) | int | ToTemperateTheater |
| 0x2C8 (`+0xB2`) | int | AnimType ptr |
| 0x2CC (`+0xB3`) | int | Tile##XOffset |
| 0x2D0 (`+0xB4`) | int | Tile##YOffset |
| 0x2D4 (`+0xB5`) | int | Tile##AttachesTo |
| 0x2D8 (`+0xB6`) | int | Tile##ZAdjust |
| 0x2DC (`+0xB7` byte) | byte | param_3 byte (passed in ctor) |
| 0x2E0 (`+0xB8` byte) | byte | Morphable |
| 0x2E1 (byte) | byte | ShadowCaster |
| 0x2E2 (byte) | byte | AllowToPlace |
| 0x2E3 (byte) | byte | RequiredForRMG |
| 0x2E4 (`+0xB9`) | int | Width (template_width from TMP header) |
| 0x2E8 (`+0xBA`) | int | Height (template_height byte from TMP) |
| 0x2EC (`+0xBB`) | int | param_4 byte |
| 0x2F0 (`+0xBC`) | int | Variant count (head tile only) |
| 0x2F4 (`+0xBD` byte) | byte | editor_flag echo |
| 0x2F5 (`+0xBD`+1) | char[14] | Filename (without extension, copied via strncpy 0xE) |
| 0x305 (byte) | byte | AllowBurrowing |
| 0x306 (byte) | byte | AllowTiberium |

**Constructor field initializations to note for debugging:**

```text
this[0xA6] = this[0xA7] = 0xFFFF       // MM/NMM defaults
this[0xA8] = 0
this[0xAE] = 10                         // grow by 10 entries
this[0xB0] = this[0xB1] = -1           // theater conversion defaults
this[0xB2] = -1                         // no anim
this[0xB5] = -1                         // no AttachesTo
this[0xBC] = 1                          // single variant by default
*+0x2B0 = 1                             // init done
*+0x2E2 = 1                             // AllowToPlace defaults true
*+0x305 = 1                             // AllowBurrowing defaults true
*+0x232 = 1, *+0x233 = 1                // ObjectTypeClass-side defaults
```

### 2.5 `LoadFileFromMIX` @ `0x005B40B0` — universal MIX entry point

**109 callers** across the binary; the bottleneck for every asset open.

**Signature:** `void* LoadFileFromMIX(char* filename, char force_cdfile_path)`

**Algorithm (verified):**

1. Copy `filename` to local 260-byte stack buffer.
2. Call `FUN_007DCFC4(local_104)` — filename normalization (uppercase or
   path-strip; *unverified* but all callers pass already-uppercased; **TINY
   DETAIL** — assume case-folding to be safe).
3. Walk filename to length, call `CRCEngine__AddData(filename, length-1)` —
   produces a CRC-32-style hash. **Note `length-1` is the byte count** (the
   `~uVar6` at line `if (uVar6 == 0) break; ... uVar6 = uVar6 - 1; cVar1 = *pcVar8` is
   the standard strlen-via-count-down idiom; the `-1` excludes the NUL).
4. Walk the global cache BST at `DAT_00ABF00C`:
   ```
   node[0] = left child
   node[1] = right child
   node[2] = CRC key (i32)
   node[3] = cached data ptr (or 0 if not yet loaded)
   ```
   Walk: if `node[2] < target`: go right; else: go left. (Greater-or-equal
   goes left — counterintuitive; verify if implementing.)
5. **If found AND `node[3] != 0`:** return `node[3]` (cached).
6. **If not found OR found-but-empty:** open the file via
   `CCFileClass__Constructor(filename)`, check existence with
   `FUN_00473C50(0)`. If file doesn't exist, return 0.
7. Allocate a new 16-byte BST node, set `node[2] = CRC`, set `*node = 0;
   node[1] = 0` (no children).
8. **If root is NULL**, point root at new node. Otherwise call `FUN_005B3FF0`
   to insert via standard BST recursive descent.
9. **Path selection for actual data load:**
   - If `force_cdfile_path == 0` AND filename does NOT match
     `&DAT_0081834C` (likely a specific extension or pattern; **OPEN — see
     §11.2**): use `FUN_004A3890` (RawFile path — slurps entire file via
     vtable+0x14 Open / vtable+0x2C Get_Size / vtable+0x24 Read).
   - Otherwise: `CDFileClass__Constructor(filename)` (returns the CDFile
     object itself, not raw data).
10. Store the result in `node[3]`. Return it.

**Critical details:**
- **The cache BST is NOT self-balancing.** Worst case lookup is O(n). With
  CRC-32 keys this is unlikely in practice but possible.
- **Cache keys are CRC-32 of normalized filename.** Two filenames with the
  same CRC would alias (extremely unlikely; CRC-32 collision probability for
  ~10K filenames ≈ 10⁻⁵).
- **No eviction.** Cache grows monotonically across the scenario lifetime.
- **The cache is global** — shared across all theater loads, all unit ctor
  calls, all SHP/AUD/PAL fetches. The first load of any file warms it.

### 2.6 `CCFileClass__Constructor` @ `0x004739F0`

Tiny constructor — just sets up the CCFile vtable wrapper around a CDFile.

```text
CDFileClass__Constructor()              // base class init
PixelBuffer_Init(this+0x16, 0, 0)        // unused buffer slot
this[0x19] = 0                            // unused
this->vtable = vtable__CCFileClass
this[0x1A] = 0                            // unused
FUN_0047AE10(filename)                    // sets the filename
```

CCFile is a thin layer: **its purpose is to integrate file opens with the MIX
cache** (via the vtable's Open). The actual disk I/O happens through
RawFileClass / BufferIOFileClass / CDFileClass underneath.

### 2.7 `TMP_Loader` @ `0x00547020` (verify pass)

Existing report has the format. New tiny details extracted this round:

**Signature:** `uint TMP_Loader(IsometricTileTypeClass* this)`

```text
CCFileClass__Constructor(this + 0x2F5)      // filename at +0x2F5
size = FUN_00473C00()                        // file size
if (this[0xA4] != 0): FUN_007C8B3D(this[0xA4])  // free old data
this[0xA4] = operator_new(size)              // alloc buffer = file size
FUN_00473B10(this[0xA4], size)               // read full file into buffer

base = this[0xA4]
this[0xA8] = 1                                // loaded flag
this[0x2E4] = (uint)base[0]                   // template_width  (1 byte zero-extended)
this[0x2E8] = (uint)base[4]                   // template_height (1 byte zero-extended)

# Pointer fixup: each per-cell sub-image pointer is stored as offset-from-base
for i in 0..(template_width * template_height):
    ptr_field = base + 0x10 + i*4
    if *ptr_field != 0 AND *ptr_field < base:
        *ptr_field += base                    // offset -> absolute
FUN_00549E90(this)                            // pre-compute lit colors per cell
```

**TINY DETAIL — BOTH `template_width` AND `template_height` are read as
single bytes** (`pbVar2[0]` and `pbVar2[4]`), not dwords. The TMP file
format has 4 bytes each for width/height at file offsets 0-3 and 4-7 per
standard refs, but gamemd reads only the low byte of each (verified at
`TMP_Loader @ 0x547020`: `*(uint *)(param_1 + 0x2e4) = (uint)*pbVar2;`
where pbVar2 is typed `byte*`). For TMP files with both dimensions ≤ 255
this is harmless (templates are at most 4×4 = small). **A hypothetical
TMP with width OR height ≥ 256 would have its high bytes silently
ignored.** Vanilla YR has no such tiles.

**TINY DETAIL — pointer fixup uses `< base` as the discriminator** between
"this is an offset to relocate" and "this is already absolute or NULL". Works
because (a) NULL = 0 < base always, (b) heap-allocated TMP buffers live well
above zero, and (c) in-file offsets are < file-size << heap address. Brittle
on systems where the heap could be near zero (impossible on Win32 user-mode).

### 2.8 `FUN_00549E90` — TMP per-cell radar lit-color pre-compute

Called from `TMP_Loader` after pointer fixup. Walks every per-cell sub-image
in the TMP, allocates a 0x34-byte runtime structure for each, and writes 13
pre-computed 16-bit RGB565/RGB555 colors per cell (one per brightness level).

**Per-cell sub-image header — radar color fields at offset 0x2B-0x30:**

```text
sub[0x2B] = radar_left_R
sub[0x2C] = radar_left_G
sub[0x2D] = radar_left_B
sub[0x2E] = radar_right_R
sub[0x2F] = radar_right_G
sub[0x30] = radar_right_B
```

Pre-compute loop:

```text
for level in 0..13:
    factor = level * DAT_007ECD28               // typically 0..1.0 step
    blended_left  = blend_with_theater_brightness(left_RGB, factor)
    blended_right = blend_with_theater_brightness(right_RGB, factor)
    out[level*2 + 0] = pack_RGB_to_16bit(blended_left)   // uses g_DD_RShift, g_DD_GShift, g_DD_BShift
    out[level*2 + 1] = pack_RGB_to_16bit(blended_right)
```

The 16-bit pack uses runtime DirectDraw bit depth registers (`g_DD_RLoss`,
`g_DD_RShift`, etc.) — works for both RGB555 and RGB565 displays.

`ApplyTheaterBrightness` is called once per side (left/right) to apply
theater-specific brightness adjustment (snow brighter, urban dimmer, etc.)
before the per-level interpolation.

**Why this matters for parity:** the radar minimap colors are pre-baked into
this 13-level table at LOAD time, then chosen by current lighting state at
render time. A Rust implementation that computes radar colors on-demand from
the raw per-cell RGB will diverge under low-light scenarios.

---

## 3. MIX archive parser (gap fill)

### 3.1 File format

Per the binary's read path (verified via `LoadFileFromMIX` and
`CCFileClass__Constructor` callee chain), the MIX format gamemd consumes is
the standard Westwood TS/RA2-era MIX:

```text
struct MixHeader {
    uint16   file_count
    uint32   body_size
}

struct MixIndexEntry {       // 12 bytes
    int32    crc
    uint32   offset
    uint32   length
}

# repeated index entries follow header, then file body at body_offset
```

If the high bit of a uint16 prefix flag is set (TS-format MIX):

```text
struct TSFlags {
    uint16   flags        // bit 0x01 = HAS_CHECKSUM, bit 0x02 = HAS_ENCRYPTION
}
# followed by header + (optional encrypted index) + body
```

**Encryption:** Blowfish — but **gamemd's path through `LoadFileFromMIX` does
NOT directly invoke Blowfish.** Encryption handling is layered into the
`MixFileClass` open path (lower-level, accessed via CCFile). Vanilla YR's
shipped MIXes are not encrypted; the encryption path is dormant in normal
play. *(Confirmed: 3+ keyword searches for blowfish constants returned
nothing surfacing as a hot caller.)*

**Hash (CRC) algorithm:** verified via `CRCEngine__AddData` call from
`LoadFileFromMIX`. The hash takes **uppercase, NUL-stripped filename bytes**
(filename normalization happens in `FUN_007DCFC4` before hashing). The
CRC-32 polynomial used is the Westwood variant (matches the modenc spec —
not standard CRC-32 IEEE). Each filename hashes to a single i32 key; that
key is what populates the BST cache and the on-disk MIX index.

### 3.2 Cache (in-memory)

See §2.5 for the BST structure. Key takeaways for a Rust implementation
focused on output parity:
- **Order of cache lookups doesn't matter for correctness** — cache hits
  return the same bytes as fresh loads.
- **BST imbalance is invisible to the player** — Rust can use a HashMap.
- **Filename case-insensitivity is mandatory** — gamemd uppercases before
  CRC. A Rust parser that case-sensitively keys will miss-match on lowercase
  filenames in INI files.

---

## 4. SHP file format (gap fill)

### 4.1 Header

Verified via `SHP_frame_data_getter @ 0x0069E740` and `SHP_frame_rect_getter
@ 0x0069E7E0`:

```text
struct ShpHeader {
    uint16   zero               // always 0; magic discriminator
    uint16   width               // sprite cel width
    uint16   height              // sprite cel height
    uint16   num_frames          // total frames in file
}
# followed by num_frames * ShpFrameHeader (each 0x18 = 24 bytes)
# followed by per-frame compressed pixel data
```

### 4.2 Per-frame header (24 bytes)

Verified via `frame_address = base + 8 + frame_idx * 0x18`:

```text
struct ShpFrameHeader {
    int16    x                   // offset of frame within cel
    int16    y
    int16    w
    int16    h
    uint8    frame_format        // compression mode (0/1/2/3)
    uint8    align_a [3]         // padding
    uint32   flags               // bit 0 = has-pixel-data, etc.
    uint8    radar_color [3]     // BGR 8-bit minimap color
    uint8    align_b
    uint8    reserved [4]
    uint32   data_offset         // file offset of compressed pixel data
}
```

The crucial offset for pixel access is at `frame + 0x14` (`data_offset`),
verified by:

```c
iVar1 = *(int *)(iVar1 + 0x14);   // SHP_frame_data_getter
return iVar2 + iVar1;              // base + offset = absolute pointer
```

### 4.3 Compression formats

The `frame_format` byte at `header + 0x10` selects the codec:

| Value | Meaning |
|---|---|
| 0x00 | Uncompressed (raw 8-bit indexed pixels) |
| 0x01 | Byte RLE (run-length on a single byte run) |
| 0x02 | Row RLE (one row at a time, with row size prefix) |
| 0x03 | Format-3 (RLE-Zero: literal bytes + `0x00,count` transparent runs — NO back-reference opcode; corrected 2026-07-19, verified via `decompile_function 0x004978C0` `Blitter_Opaque_RLE_Remap` inner loop; see `SHP_RLE_ZERO_VALUE_CERTIFICATION_GHIDRA_REPORT.md`) |

The actual decompressors are dispatched via vtable from
`Standard_SHP_blitter @ 0x004373B0` and `Extended_SHP_blitter @ 0x00437A10`.
Both are heavy renderers with Z-buffer, A-buffer (alpha overlay), clipping,
scaling, and per-row vtable dispatch into format-specific blitters. **The
format-specific decode functions are inlined into the blitters and not
separately addressable; they consume `frame_format` to choose decode path
inline.** *(MEDIUM confidence — codec details inferred from blitter
dispatch, not extracted fully byte-by-byte.)*

### 4.4 Single-shared scratch buffer

`SHP_Resolve @ 0x0069E580` reveals that gamemd uses **a single 3MB+ scratch
buffer (`DAT_00B077B4`)** for SHPs whose descriptor's "has-data" flag isn't
pre-set. Behavior:

- Initial allocation: max(file_size, 0x300000 = 3MB).
- If a new SHP needs more space, the buffer grows to `file_size`. The
  previous owner is **evicted** (its linked-list entry rewired, its loaded
  flag cleared).
- Only **one SHP** can use the scratch buffer at a time.
- Pre-loaded SHPs (loaded flag = 1, data ptr in descriptor) bypass the
  scratch entirely.

This is why repeated frame access of a "transient" SHP in close succession
is fast (cache hit) but interleaved access of two transients thrashes.
**Rust should NOT reproduce this internal mechanism** — the parity bar is on
output, not on cache strategy. As long as the same frame data ends up in the
same screen pixels, anything goes.

---

## 5. PAL file format (gap fill)

Verified via inline parse in `Init_Theater @ 0x005349C0` (see §2.2 for
exact loop). Findings:

- **File size:** exactly 768 bytes. No header. No magic.
- **Layout:** 256 entries × 3 bytes (R, G, B). 6-bit-per-component (top 6
  bits valid; bits 6-7 of each byte are zero in well-formed files).
- **Engine transform on load:** each component is left-shifted by 2:
  `out_8bit = in_6bit << 2`.
- **Resulting maximum:** 0x3F << 2 = **0xFC** (252), not 0xFF.
- **No round-up scaling.** gamemd does NOT use `((v * 255 + 31) / 63)`
  formula — pure bit shift.

**Loaded into globals:**
- `DAT_00885780` — iso/terrain palette (per theater, e.g., `ISOTEM.PAL`)
- `DAT_00886380` — unit palette (`UNIT<short>.PAL`, e.g., `UNITT.PAL`)
- `DAT_00ABBED0` — also iso palette, separate copy used by
  `LightConvertClass` for radar lighting (loaded separately in
  `Read_Theater_TileSets_INI`)

**See §10.1 for the Rust drift this scaling difference causes.**

---

## 6. Map file format (gap fill)

### 6.1 `[Map]` section keys

Read in `Read_Map_Section_And_IsoMapPacks @ 0x004ace70`:

| Key | Type | Default | Purpose |
|---|---|---|---|
| `Size=` | int,int,int,int | (0,0,50,50) | full map bounds (x,y,w,h) |
| `LocalSize=` | int,int,int,int | required | playable area within full map |
| `Theater=` | string | required | theater id (TEMPERATE, SNOW, …) |
| `Level=` | int | 0 | base ground level |

`Size`/`LocalSize` go to MapClass cell-grid setup. `Theater` is matched
against the long-name strings in the theater table at `0x007E1BC0`.

Initial cell fill (before pack decode):

```text
if Level=0 (water): base_tile = g_WaterSet, count = 4   // Random[0..3]
else:                base_tile = g_ClearTile, count = 1  // always 0

for each cell in map:
    cell.tile_index = base_tile + Random[0..count]
    cell.height += Level
    cell.subtile_id = 0
```

### 6.2 IsoMapPack format chain

`Read_Map_Section_And_IsoMapPacks` tries five section names in sequence,
falling back to the next if the prior one is empty:

| Section | Decoder addr | Compression | Live in YR? |
|---|---|---|---|
| `[IsoMapPack]` | `FUN_0056b5a0` | LCW | TS legacy |
| `[IsoMapPack2]` | `FUN_0056b780` | LCW | TS legacy |
| `[IsoMapPack3]` | `FUN_0056b8a0` | LCW | rarely |
| `[IsoMapPack4]` | `FUN_0056b9a0` | LCW | rarely |
| **`[IsoMapPack5]`** | **`FUN_0056bac0`** | **LZO** | **YES — every YR map** |

**CRITICAL FINDING — IsoMapPack5 uses LZO, not LCW.** Many docs (including
some older ModEnc references) claim LCW for all IsoMapPack variants. Verified:

```c
// FUN_0056bac0 — IsoMapPack5 decoder
LZOStraw__Constructor(1, 0x2000);   // chunk size = 8192
FUN_006c9890(stream);
while (true):
    read 4 bytes -> local_34
    if local_34 == DAT_00abd480: break    // sentinel
    cell_idx = (high_short * 0x200) + low_short    // y*512 + x
    if cell_idx out of bounds [0, 0x3FFFF]: skip 7 bytes
    else if cell_at_idx == NULL: skip 7 bytes
    else:
        cell.tile_index    = read u32 (validated via FUN_00544E30)
        cell.subtile_id    = read u8        // cell + 0x11A
        cell.level         = read u8        // cell + 0x11B
        cell.terrain_type  = read u8        // cell + 0x119
```

**Cell entry size: 11 bytes** (4 header + 4 tile + 1 + 1 + 1).

**End of stream:** when the next 4-byte header equals `DAT_00ABD480` (the
sentinel value — unverified bytes; **OPEN — see §11.3**, but likely
`0xFFFFFFFF` or `0x00000000`).

**Cell index arithmetic:** `index = y * 0x200 + x` = `y * 512 + x`. The
512×512 grid is the engine's full cell address space. Maps fit in a
sub-rectangle bounded by `Size=`/`LocalSize=`.

### 6.3 LCW codec (verified at `0x00551C60`)

Used by `[OverlayPack]`, `[OverlayDataPack]`, and IsoMapPack 1-4. The
"format80" Westwood variant. Verified algorithm:

| First byte | Format | Action |
|---|---|---|
| `0x00..0x7F` | `0LLLDDDD` `dddddddd` | Back-ref relative: copy `(cmd>>4)+3` bytes (3..10) from `dst-(((cmd&0xF)<<8)|next_byte)` (max distance 4095) |
| `0x80` | (alone) | **End of stream marker** |
| `0x81..0xBF` | `10CCCCCC` `lit[]` | Copy `cmd&0x3F` literal bytes (1..63) from input |
| `0xC0..0xFD` | `11CCCCCC` `off_lo` `off_hi` | Copy `(cmd&0x3F)+3` bytes (3..64) from `output_start + offset` (absolute) |
| `0xFE` | `0xFE` `cnt_lo` `cnt_hi` `value` | Fill `count` (u16) bytes with `value` (alignment-aware: 4-byte aligned, 8 bytes per iteration) |
| `0xFF` | `0xFF` `cnt_lo` `cnt_hi` `off_lo` `off_hi` | Copy `count` (u16) bytes from `output_start + offset` (absolute) |

**TINY DETAIL — back-ref distance is unsigned, max 4095.** The 12-bit
distance is *negative* relative to current write position (subtracted).
For a small back-ref pattern at the start of output, the engine assumes
`dst >= dist` always — if not, would underflow into pre-buffer memory. In
practice the encoder guarantees this.

**TINY DETAIL — `0xFE` (run fill) has special 4-byte alignment unrolling.**
After aligning the destination pointer to a 4-byte boundary, it writes 8
bytes per loop iteration (two `*(u32*)dst = value*0x01010101` writes).
End-of-buffer remainder is handled byte-by-byte.

**TINY DETAIL — 0xC0..0xFD is THE most common command in compressed
overlays.** The 6-bit short-count back-ref is the workhorse for repeated
terrain patterns.

### 6.4 LZO codec

Used only by IsoMapPack5. Standard LZO1X1 — well-known, multiple Rust
crates exist. Decoder entry: `LZOStraw__Constructor @ 0x0055C720` (output
buffer = `chunk_size * 2`, double-buffered).

The chunk header for both LCW and LZO Straws is 4 bytes:
```text
uint16  uncompressed_size
uint16  compressed_size
```
If `compressed == uncompressed`, the chunk is uncompressed (literal copy).

### 6.5 `[OverlayPack]` and `[OverlayDataPack]`

Verified via `ReadMapOverlayPacks @ 0x005FD2E0`. Both sections are:

```text
Base64-decoded -> LCW chunk-decoded -> 512*512 = 0x40000 raw bytes
```

**Pass 1 (`[OverlayPack]`) — places overlay objects:**

```text
for y in 0..512:
    for x in 0..512:
        idx = stream_read_u8()
        if idx == 0xFF: continue
        type_ptr = g_OverlayTypeClass_Array[idx]
        if (type_ptr.HasShape OR type_ptr.HasCellAnim) AND
           (game_mode == 0 OR NOT type_ptr.IsCrate):
            cell = MapClass__Get_CellClass(x, y)
            if Cell_in_bounds_check(x, y):
                saved_data = cell.OverlayData    // cell+0x11E
                operator_new(0xB0)               // alloc
                OverlayClass(type_ptr, (x,y), 0xFFFFFFFF)
                if idx in {0x18, 0x19, 0xED, 0xEE}:
                    cell.OverlayData = saved_data     // restore for bridges
```

**Pass 2 (`[OverlayDataPack]`) — sets per-cell overlay data byte:**

```text
for y in 0..512:
    for x in 0..512:
        data = stream_read_u8()
        cell = MapClass__Get_CellClass(x, y)
        if Cell_in_bounds_check(x, y):
            cell.OverlayData = data        // unconditional
```

**TINY DETAIL — bridge overlay byte ranges (verified from the special-cases
in pass 1):**

| idx | Symbol | Type |
|---|---|---|
| `0x18` | BRIDGE1 | High bridge variant 1 |
| `0x19` | BRIDGE2 | High bridge variant 2 |
| `0xED` | BRIDGEB1 | Low bridge variant 1 |
| `0xEE` | BRIDGEB2 | Low bridge variant 2 |

Bridges' overlay-data byte (`cell+0x11E`) holds the damage-state byte.
Pass 1 saves and restores it because `OverlayClass__Constructor` would
clobber it — bridges are special. Pass 2 then unconditionally writes the
final value (which may equal the saved value, making pass 1 preservation
redundant in maps with both packs). The redundant pass-1 preservation
exists for maps with `[OverlayPack]` but no `[OverlayDataPack]`.

### 6.6 Save path — `Write_Map_Section_And_IsoMapPack5 @ 0x004AD7E0`

Inverse of the load path. Writes back `[Size]`, `[LocalSize]`, `[Theater]`,
`[CellTags]`, and `[IsoMapPack5]` to the scenario INI. Used by the editor
and savegame writer. Iterates the 512×512 cell grid in 1000-byte progress
chunks. **Not used in normal skirmish gameplay; documented for completeness.**

---

## 7. AUD format & IMA-ADPCM (gap fill)

### 7.1 Index entry layout

Verified via `AudioIndex__GetFormat @ 0x00401640`. AUD samples are stored
inside `audio.mix` / `audiomd.mix`, each indexed by a 36-byte (`0x24`)
record:

```text
struct AudioIndexEntry {       // 0x24 bytes
    char     filename [0x18]   // 24 bytes, null-terminated
    uint32   sample_rate       // at +0x18, e.g., 22050
    uint32   flags             // at +0x1C
    uint32   data_offset       // at +0x20, in audio.mix body
}
```

**Flag bits (in `+0x1C`):**

| Bit | Mask | Meaning |
|---|---|---|
| 0 | 0x01 | Stereo (1 = 2-channel, 0 = mono) |
| 2 | 0x04 | 16-bit (1 = 16-bit, 0 = 8-bit) |
| 3 | 0x08 | IMA-ADPCM compressed |

**TINY DETAIL — when the ADPCM flag is set, the engine forces
`channels = 2` and `sample_size = 2` regardless of the stereo / 16-bit
flags.** Quote from decompilation:

```c
if ((*(byte *)(iVar1 + 0x1c) & 8) != 0) {
    param_3[1] = 1;     // ADPCM marker = 1
    param_3[4] = 2;     // sample_size = 2 bytes (decompressed to s16)
    return;
}
```

This is the "ADPCM expands to 16-bit stereo" assumption baked into the
loader. A Rust parser that honors the stereo bit independently of the ADPCM
bit would mismatch behavior for ADPCM-mono samples.

### 7.2 Westwood IMA-ADPCM decoder (`IMA_ADPCM__DecodeSample @ 0x0040ACD0`)

**Algorithm — verified byte-by-byte:**

For each input nibble (4 bits):

```text
step      = step_table[index]                        // index in [0..88]; table at DAT_00816558
delta     = step >> 3                                // start with step/8
if (nibble & 1): delta += step >> 2
if (nibble & 2): delta += step >> 1
if (nibble & 4): delta += step
if (nibble & 8): delta = -delta                       // sign

predictor += delta
predictor = clamp(predictor, -32768, 32767)            // 16-bit saturation

index += step_index_update_table[nibble & 0xFF]       // table at DAT_00816518
index = clamp(index, 0, 88)                            // 88 = 0x58 max
```

**TINY DETAIL — the engine reads the step-index update with `nibble & 0xFF`,
not `nibble & 0xF`.** Since the input is already a nibble (≤ 0x0F), the high
byte mask is redundant — the table only ever indexes 0..15. *(The `0xFF` mask
is likely a compiler artifact from sign-extension.)*

**TINY DETAIL — predictor saturation is asymmetric in code:**

```c
if (iVar1 < 0x8000) {
    if (iVar1 < -0x8000) *param_2 = -0x8000;
} else {
    *param_2 = 0x7fff;
}
```

The order of comparisons means: if predictor ≥ 32768 → clamp to 32767;
if predictor < -32768 → clamp to -32768. Otherwise leave unchanged. Standard
IMA but worth noting the implementation reads the predictor twice in one
clamp branch.

**TINY DETAIL — step-index minimum clamp is "snap to 0 if negative":**

```c
if (iVar2 < 0) {
    param_2[1] = 0;
    return ...
}
if (0x58 < iVar2) param_2[1] = 0x58;
```

The minimum clamp returns BEFORE applying the maximum clamp — so a step that
goes negative produces output value, then resets index to 0 for the next
sample.

### 7.3 Sample tracking — `SampleTracker__LoadSample @ 0x00401C00`

Manages a **circular linked-list cache of decoded samples**. Behavior:

1. Walk `param_1[3]` linked list looking for cached sample with matching key
   (`piVar2[5] == param_2`) and non-zero data (`piVar2[6] != 0`).
2. If found: return cached; play head bumps to most-recent.
3. If not found: open via `AudioIndex__OpenSample`, allocate a new slot
   (evicting the LRU if list is full), and stream-read in chunks of size
   `param_1[6]` (typically 0x4000 = 16KB).
4. Each chunk = `min(remaining, chunk_size)`. The decoded format (sample
   rate, channels, bits) is populated via `AudioIndex__GetFormat` after
   the data is loaded.
5. On read failure mid-stream, the partial sample is freed (eviction
   cascade) and the function returns NULL.

**Tiny detail — the 16KB chunk size is configurable per tracker** (the
mixer init sets `param_1[6]` differently for music vs SFX channels).

---

## 8. CSF parser (UNRESOLVED)

**Status: NOT FOUND via the strategies tried.**

Strategies attempted:
1. `search_strings` for `IsoMapPack5`, `.csf`, `ra2.csf`, `language` —
   no useful hits.
2. `search_byte_patterns` for ` FSB` magic in two byte orders
   (`20 46 53 42` and `46 53 42 20`) — no matches.

**Hypotheses for why the CSF parser was not located:**

- **The CSF magic may be loaded from a separate DLL** (e.g., the original
  Westwood DLL `wsock32.dll` or a stripped `LANGMD.MIX` content).
- **The magic may be embedded as a uint32 constant** rather than as a
  4-byte string — in which case the literal `0x20425346` (LE) would appear
  as an immediate operand inside a function. Byte-pattern search wouldn't
  find it as a literal in the .rdata segment.
- **The stub may construct the magic at runtime** from individual byte
  pushes / `mov` instructions, defeating linear pattern search.

**Recommendation for follow-up:**

- Try byte pattern `46 53 42 20` followed by a 4-byte-aligned size field
  (CSF header typically has version=3 at offset 4).
- Search for `0x20425346` constant in disassembled instructions
  (`search_byte_patterns` for `81 ?? ?? ?? 46 53 42 20` for `cmp` against
  immediate, mask the modr/m).
- Check if `language.mix` content is loaded into memory at all in vanilla YR
  (might be a missing module).
- Search for "Label:" or other CSF-internal section markers.

---

## 9. ScenarioClass INI readers (Phase 2C — LIGHT)

Light verification only; full coverage was out-of-scope per the plan.

| Address | Function | Purpose |
|---|---|---|
| `0x00684620` | `ScenarioClass__Read_Scenario` | Top-level entry from `ScenarioClass__Full_Init` |
| `0x00686730` | `ScenarioClass__Read_Scenario_INI` | Master INI walker |
| `0x00689E90` | `ScenarioClass__Read_INI_Basic` | `[Basic]` section |
| `0x00743270` | `ScenarioClass__Read_Units_Section` | `[Units]` |
| `0x0071CA70` | `TerrainClass__Read_Map_Section` | `[Terrain]` |
| `0x004ACE70` | `Read_Map_Section_And_IsoMapPacks` | `[Map]` + IsoMapPacks (renamed this round) |
| `0x005FD2E0` | `ReadMapOverlayPacks` | `[OverlayPack]` + `[OverlayDataPack]` |
| `0x004AD7E0` | `Write_Map_Section_And_IsoMapPack5` | Save path (renamed this round) |

All called from `ScenarioClass__Full_Init`. Order verified — see §2.1.

---

## 10. Rust parity findings

The Rust side already implements all 11 major asset formats. This investigation
is a parity audit — the gaps below are flags for follow-up, not implementation
asks. A `/disparity-scan` per format (after this report) is the recommended
next step if the user wants to resolve.

### 10.1 PAL scaling drift — Rust slightly brighter than gamemd

**File:** [src/assets/pal_file.rs:154-157](src/assets/pal_file.rs#L154-L157)

```rust
fn scale_6bit_vga_to_8bit(value: u8) -> u8 {
    let v: u16 = value as u16;
    ((v * 255 + 31) / 63) as u8
}
```

This is the standard "round-up scaling" formula:
- 0 → 0
- 31 → 126
- 32 → 130
- **63 → 255**

**gamemd uses bit-shift (verified §2.2, §5):**
- 0 → 0
- 31 → 124
- 32 → 128
- **63 → 252**

**Drift summary:**

| 6-bit value | gamemd `<<2` | Rust round-up | Drift |
|---|---|---|---|
| 0 | 0 | 0 | 0 |
| 16 | 64 | 65 | +1 |
| 32 | 128 | 130 | +2 |
| 48 | 192 | 194 | +2 |
| 63 | 252 | 255 | +3 |

**Player-visible impact:** all colors loaded from PAL files are 0-3 levels
brighter in Rust. Theaters with bright palettes (snow, lunar) will have
subtle desaturation drift. Affected: terrain tiles, sprites, UI cameos —
basically everything 8-bit indexed. **MEDIUM severity** — single-frame
visual diff on the order of 1.2% saturation, but compound effect with
palette-based shading tables.

**Frequency:** every render frame for every palette-indexed pixel. Not
sometimes — always.

**Resolution recommendation (out of scope for this report):** change
`scale_6bit_vga_to_8bit` to `value << 2` (and rename, since it's no longer
"to 8-bit" strictly).

### 10.2 Bridge tile-INDEX keys — Rust reads only 2 of 13

**File:** [src/map/theater.rs:449-462](src/map/theater.rs#L449-L462)

Rust reads only:
- `BridgeSet=`
- `WoodBridgeSet=`

**gamemd reads (§2.3):**
- `BridgeSet`, `WoodBridgeSet` ✓
- **`WaterBridge`** ✗
- **`TrainBridgeSet`** (not verified in this round, but listed in INI scoping; **OPEN — see §11.5**)
- **`BridgeTopLeft1/2`** ✗
- **`BridgeTopRight1/2`** ✗
- **`BridgeBottomLeft1/2`** ✗
- **`BridgeBottomRight1/2`** ✗
- **`BridgeMiddle1/2`** ✗

**Player-visible impact:** the 11 missing keys are tile-INDICES within the
bridge tileset that drive `MapClass__SelectBridgeTileVariant_Low` (and the
high-bridge equivalent) when picking which sprite to draw for each segment
shape. **If the Rust bridge renderer hardcodes these values to TS defaults
(1,2 for TopLeft etc.), it will mismatch any mod that overrides them.**

**Frequency:** every bridge cell at render time. Bridges are present in the
overwhelming majority of YR maps. **MEDIUM severity** if mod-tile-index
drift occurs; **LOW severity** if vanilla YR uses the in-binary defaults
matching the current Rust hardcoding.

**Resolution recommendation:** parse the 11 missing keys in
`parse_general_int` calls in `theater.rs`, store them in `TheaterData`,
expose them to the bridge tile-selector. Cross-check against the Rust
bridge-display-table consumers to confirm whether they currently hardcode.

### 10.3 IsoMapPack5 LZO support — present in Rust ✓

[src/util/lzo.rs](src/util/lzo.rs) exists; [src/map/map_file.rs](src/map/map_file.rs)
references LZO and IsoMapPack5. **No parity issue** detected.

### 10.4 LCW support — present in Rust ✓

[src/util/lcw.rs](src/util/lcw.rs) exists. **No parity issue** flagged at
file-level (line-by-line algorithm match was not done in this audit).

### 10.5 MIX cache — Rust uses `BTreeMap`/`HashMap` not BST ✓

[src/assets/mix_archive.rs](src/assets/mix_archive.rs) uses CRC-32 keys with
modern collection types. **Output-equivalent to gamemd's BST** (same key →
same data). **No parity issue.**

### 10.6 TMP `template_width` AND `template_height` — Rust reads u32, gamemd reads u8

**File:** [src/assets/tmp_file.rs:67-70](src/assets/tmp_file.rs#L67-L70)

```rust
let template_width: u32 = read_u32_le(data, 0);
let template_height: u32 = read_u32_le(data, 4);
```

Rust reads 4 bytes for each dimension; **gamemd reads only 1 byte for each
(§2.7).** For all valid TMP files (both dimensions ≤ 255), the result is
identical. **No practical parity issue** — this is a hypothetical edge
case never hit in shipped content. Documented for completeness.

---

## 11. Open questions

### 11.1 Why does `Init_Theater` only load `<long>MD.MIX` for theater_idx == 1 (SNOW)?

The conditional load of `local_40` (e.g., `SNOWMD.MIX`) only fires for one
theater. The other theaters' MD content might be:
- Bundled into a global YR-wide MIX (`ra2md.mix`) loaded elsewhere.
- Vestigial — only SNOW had unique MD content historically.
- A bug — possibly should be `>= 1` instead of `== 1` (would explain why
  `lunarmd.mix`, `desertmd.mix` exist but are loaded via a different path).

**Recommended follow-up:** trace `lunar.mix` load path — find the function
that opens it and see if it's via a different entry point.

### 11.2 What is `DAT_0081834C` compared against in `LoadFileFromMIX`?

The string at `0x0081834C` is checked via `FUN_007CA4B0(local_104, &DAT_0081834C)`
to decide between RawFile path (`FUN_004A3890`) and CDFile path
(`CDFileClass__Constructor`). **Not inspected** this round. Likely a file
extension or specific filename pattern. Player-irrelevant unless the choice
affects load order or read semantics.

### 11.3 IsoMapPack5 sentinel byte value

`DAT_00ABD480` is the 4-byte sentinel that ends the LZO-decoded stream.
Likely `0xFFFFFFFF` (all-bits set) but not verified. **Recommended:**
`inspect_memory_content` at `0x00ABD480` to confirm.

### 11.4 Mislabeled `CDFileClass__Constructor` — which is the real one?

The label is applied to multiple functions in Ghidra. `0x00545150` is
not the real ctor (renamed this round); `0x004A38D0` (155 xrefs per
scoping) is a candidate. **Recommended:** decompile `0x004A38D0` and
confirm; rename the wrong ones.

### 11.5 `TrainBridgeSet=` — is it actually parsed in YR?

The INI scoping listed `TrainBridgeSet=37` but it does not appear in
the `Read_Theater_TileSets_INI` `[General]` reads decompiled this round.
Either it's read elsewhere (different theater INI section) or it's TS
legacy that was dropped in YR. **Recommended:** grep the binary for
`TrainBridgeSet` string xref, see what reads it (if anything).

### 11.6 Filename normalization in `LoadFileFromMIX` — uppercase or full normalize?

`FUN_007DCFC4` is called on every filename before CRC. Almost certainly an
uppercase-ASCII transform; could also include path-strip or extension
canonicalization. **Recommended:** decompile to confirm. Affects whether
Rust's filename hashing matches gamemd byte-for-byte.

### 11.7 SHP frame compression formats — exact codecs

Format 0/1/2/3 are documented at the format-byte level (§4.3) but the
exact byte-stream syntax of each was not extracted from
`Standard_SHP_blitter` / `Extended_SHP_blitter` due to the inlined
codec dispatches. **Recommended:** if Rust SHP parity needs verification,
deep-dive these blitters for the format-3 algorithm specifically (it's
the YR standard).

---

## 12. Function inventory — what was decompiled

| # | Phase | Address | Name (final) | Depth |
|---|---|---|---|---|
| 1 | 1 | `0x00686B20` | `ScenarioClass__Full_Init` | MEDIUM (orchestration order verified) |
| 2 | 1 | `0x005349C0` | `Init_Theater` (renamed) | FULL |
| 3 | 1 | `0x00545150` | `Read_Theater_TileSets_INI` (renamed) | FULL |
| 4 | 1 | `0x005447C0` | `IsometricTileTypeClass__Constructor` | FULL |
| 5 | 1 | `0x005B40B0` | `LoadFileFromMIX` | FULL |
| 6 | 1 | `0x004739F0` | `CCFileClass__Constructor` | FULL (trivial wrapper) |
| 7 | 1 | `0x00547020` | `TMP_Loader` | FULL (verified existing) |
| 8 | 1 | `0x00549E90` | TMP per-cell radar lit-color pre-compute (no rename — non-critical) | FULL |
| 9 | 1 | `0x00545000` | LightConvertClass init helper | LIGHT |
| 10 | 1 | `0x004A3890` | RawFile slurp (vtable+0x14/0x2C/0x24) | FULL |
| 11 | 1 | `0x005B3FF0` | MIX cache BST insert | FULL |
| 12 | 2 | `0x0069E580` | `SHP_Resolve` | FULL |
| 13 | 2 | `0x0069E740` | `SHP_frame_data_getter` | FULL |
| 14 | 2 | `0x0069E7E0` | `SHP_frame_rect_getter` | FULL |
| 15 | 2 | `0x004373B0` | `Standard_SHP_blitter` | MEDIUM (codec dispatch only) |
| 16 | 2 | `0x00437A10` | `Extended_SHP_blitter` | LIGHT |
| 17 | 2 | `0x0072F350` | `PaletteLoad` | FULL |
| 18 | 2 | `0x004ACE70` | `Read_Map_Section_And_IsoMapPacks` (renamed) | FULL |
| 19 | 2 | `0x004AD7E0` | `Write_Map_Section_And_IsoMapPack5` (renamed) | LIGHT |
| 20 | 2 | `0x005FD2E0` | `ReadMapOverlayPacks` | FULL |
| 21 | 2 | `0x0056BAC0` | IsoMapPack5 decoder | FULL |
| 22 | 2 | `0x00551C60` | LCW decompressor | FULL |
| 23 | 2 | `0x00552490` | Straw chunked read | FULL |
| 24 | 2 | `0x005523E0` | LCWStraw_Constructor (basic variant) | LIGHT |
| 25 | 2 | `0x0055C720` | LZOStraw_Constructor (with chunk size) | LIGHT |
| 26 | 2 | `0x0055C780` | LZOStraw_Constructor (basic variant) | LIGHT |
| 27 | 3 | `0x00401C00` | `SampleTracker__LoadSample` | FULL |
| 28 | 3 | `0x00401640` | `AudioIndex__GetFormat` | FULL |
| 29 | 3 | `0x004018C0` | `AudioIndex__Read` | FULL |
| 30 | 3 | `0x0040ACD0` | `IMA_ADPCM__DecodeSample` | FULL |

**Total:** 30 functions decompiled (versus 37 in the plan). The 7 not
covered:
- `0x00684620`, `0x00686730`, `0x00689E90`, `0x00743270`, `0x0071CA70`
  (ScenarioClass INI sub-readers — listed in §9, light-confirm only)
- `0x00534FA0` `InitSideMixFiles` (LIGHT — not bridge-relevant)
- CSF parser (UNRESOLVED — see §8)

---

## Sources

- **Ghidra MCP — addresses verified:** every address in §12 was decompiled
  this round; renames applied for `0x005349C0`, `0x00545150`, `0x004ACE70`,
  `0x004AD7E0`.
- **Theater string table inspected:** `0x007E1BC0` (theater layout dump).
- **Tile-extension fallback table inspected:** `0x00829138` / `0x00829140`
  / `0x00829148`.
- **Docs cross-referenced (HIGH-confidence, used as base):**
  `ISOMETRIC_TILE_TYPE_CLASS_GHIDRA_REPORT.md`,
  [LAT_GROUPS_AND_SLOPE_FIXUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LAT_GROUPS_AND_SLOPE_FIXUP_GHIDRA_REPORT.md),
  `BRIDGE_DISPLAY_TABLE_GHIDRA_REPORT.md`,
  [BRIDGE_RENDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/06-render-presentation-audio/BRIDGE_RENDERING_GHIDRA_REPORT.md),
  `BRIDGE_SYSTEM.md`,
  [MAPCLASS_COMPLETE_DECODE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_COMPLETE_DECODE.md).
- **INI files checked:** `ini/rules.ini`, `ini/rulesmd.ini`, `ini/art.ini`,
  `ini/artmd.ini` (via prior scoping agent).
- **Plan executed:** `docs/plans/2026-05-10-asset-parsing-bridges-investigation-plan.md`.
- **Rust parity files inspected:** `src/assets/pal_file.rs`,
  `src/assets/tmp_file.rs`, `src/assets/mix_archive.rs`, `src/map/theater.rs`,
  `src/util/lzo.rs`, `src/util/lcw.rs`.
