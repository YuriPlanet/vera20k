# Asset Parsing — Bridges-Priority Investigation Plan

> **For Claude:** This plan scopes a `/re-investigate` pass on gamemd.exe asset
> parsing, with the bridge tile-loading chain as the priority focus. Execute by
> running `/re-investigate asset-parsing-bridges` with this plan loaded as
> context, OR dispatch the function inventory to subagents in batches grouped
> by phase.

**Topic:** gamemd.exe asset parsing — bridge tile-loading chain (priority) + format-parsing gaps (MIX, SHP, PAL, CSF, AUD, .MAP packs)
**Scope Size:** Large — ~35-40 functions, ~25 INI keys, 6 file formats with documentation gaps
**Est. Effort:** ~10-14 hours of `/re-investigate` work
- Phase 1: ~3-4 hours (FULL on 8 functions: MIX entry + bridge TMP chain)
- Phase 2: ~4-5 hours (FULL/MEDIUM on ~15 functions: SHP, PAL, Map packs/LCW)
- Phase 3: ~3-5 hours (MEDIUM/LIGHT on ~12 functions: AUD, CSF byte-search, callers, parity audit)

**Prior Research:**
- HIGH (covered, **out of scope** for this investigation):
  - [VXL_HVA_FILE_FORMAT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_HVA_FILE_FORMAT_GHIDRA_REPORT.md) — VXL/HVA on-disk format + loaders
  - `ISOMETRIC_TILE_TYPE_CLASS_GHIDRA_REPORT.md` — TMP file format + slope type + class layout
  - [LAT_GROUPS_AND_SLOPE_FIXUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LAT_GROUPS_AND_SLOPE_FIXUP_GHIDRA_REPORT.md) — theater LAT-table init at `0x00545150`
- HIGH (related but runtime, **out of scope**):
  - `BRIDGE_DISPLAY_TABLE_GHIDRA_REPORT.md` — runtime tile picker
  - [BRIDGE_RENDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/06-render-presentation-audio/BRIDGE_RENDERING_GHIDRA_REPORT.md) — render pipeline
  - `BRIDGE_SYSTEM.md` — CellClass bridge fields + state machine
  - [MAPCLASS_COMPLETE_DECODE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_COMPLETE_DECODE.md) — runtime class layout (NOT .map file parsing)
  - `LAT_RETRIGGER_AND_BRIDGE_DAMAGE_VARIANT_GHIDRA_REPORT.md`
- **Confirmed gaps** (in scope):
  - **No** MIX archive parser doc (header, file index, blowfish-encrypted MIX, hash table)
  - **No** SHP file-format doc (only rendering integration covered in [PARACHUTE_SHP_RENDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARACHUTE_SHP_RENDERING_GHIDRA_REPORT.md))
  - **No** PAL file-format doc (referenced inline; never broken out)
  - **No** CSF (string-table) parser doc
  - **No** AUD file-format / IMA-ADPCM-Westwood doc
  - **No** `.map` / `.mmx` file-format doc (`IsoMapPack5`, `OverlayPack`, LCW decompression)
  - **No** doc of the *bridge-specific TMP-loader xref chain* (`Init_Theater @ 0x005349C0` → `Read_Theater_TileSets_INI @ 0x00545150` → `LoadFileFromMIX @ 0x005B40B0`) as a single end-to-end story

**Expected Output:** research document at
`docs/research/ASSET_PARSING_BRIDGES_GHIDRA_REPORT.md`
Plus optional sub-docs split by format if the report grows past ~2000 lines:
- `MIX_ARCHIVE_FORMAT_GHIDRA_REPORT.md`
- `SHP_FILE_FORMAT_GHIDRA_REPORT.md`
- `MAP_FILE_FORMAT_GHIDRA_REPORT.md`

**Next Pipeline Step:** parity-audit only — the Rust side already has all 11
format parsers implemented. After this investigation, the user decides whether
findings warrant a `/disparity-scan` per format, or targeted fixes via
`/brainstorm` + `/write-plan` for any specific divergences found.

---

## 1. Goal

When this investigation finishes, the report must answer:

1. **How does gamemd.exe load a bridge tile from disk to a renderable pixel?**
   Trace the full chain: scenario init → theater MIX open → tile-set INI read →
   per-tile TMP load via MIX → `IsometricTileTypeClass` populated → tile blitted.
   Every step's exact function, address, file-format byte layout, and edge
   cases must be documented.
2. **What are the byte-level on-disk formats** of MIX, SHP, PAL, CSF, AUD, and
   the map packs (`IsoMapPack5`, `OverlayPack`) — so our Rust parsers can be
   audited against them?
3. **Are there any bridge-special-cased asset paths** that diverge from the
   generic terrain-tile load? (e.g., `BridgeMiddle1/2`, `BridgeBottomLeft1/2`,
   `WaterBridge` — does the loader treat these differently or just stash IDs?)
4. **For each format**, does our current Rust parser at `src/assets/<format>.rs`
   produce the same output for the same input bytes? (Identify discrepancies, do
   not fix them — fixes are a separate task.)

## 2. Prior Research Inventory

| Report | Scope | Confidence | Known Gaps |
|--------|-------|------------|------------|
| [VXL_HVA_FILE_FORMAT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_HVA_FILE_FORMAT_GHIDRA_REPORT.md) | VXL/HVA on-disk + parser | HIGH | MIX-extraction not covered (out of scope for this plan, this gap is *in* our scope) |
| `ISOMETRIC_TILE_TYPE_CLASS_GHIDRA_REPORT.md` | TMP_Loader `0x00547020` + slope type + class layout | HIGH | How TMP files are *located* in MIX — covered upstream by `0x00545150` (in scope here) |
| [LAT_GROUPS_AND_SLOPE_FIXUP_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/LAT_GROUPS_AND_SLOPE_FIXUP_GHIDRA_REPORT.md) | Theater INI loader at `0x00545150` (LAT focus) | HIGH | Bridge-specific keys (`BridgeMiddle1`, `WaterBridge`, etc.) read by same function but not enumerated there — **must extend coverage** |
| `BRIDGE_DISPLAY_TABLE_GHIDRA_REPORT.md` | Runtime tile picker (overlay byte ranges) | HIGH (HIGH/MED/LOW noted) | No asset-loading coverage |
| [BRIDGE_RENDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/06-render-presentation-audio/BRIDGE_RENDERING_GHIDRA_REPORT.md) | Renderer integration | HIGH | No asset-loading coverage |
| [MAPCLASS_COMPLETE_DECODE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_COMPLETE_DECODE.md) | Runtime MapClass class layout | HIGH | **Does NOT cover .map file parsing** (recurring point of confusion — verify in this report) |
| [PARACHUTE_SHP_RENDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PARACHUTE_SHP_RENDERING_GHIDRA_REPORT.md) | SHP runtime rendering | HIGH | No SHP file-format coverage |

**Conflicts between reports:**
- `BRIDGE_DISPLAY_TABLE` corrects `BRIDGE_RENDERING` on `FUN_004D1890` being a fogged-object walker, not the live render path. Note for executor: in this asset-parsing pass, **do not** rely on `BRIDGE_RENDERING` for caller chains; use `BRIDGE_DISPLAY_TABLE` and live decompilation.
- Several functions are mislabeled `CDFileClass__Constructor` (147+ xrefs of one symbol applied to multiple functions). Treat that label as suspect; verify each instance.

## 3. Function Inventory

| # | Phase | Address | Current Name | Scope Reason | Depth Target | TS-Legacy Risk |
|---|-------|---------|--------------|--------------|--------------|----------------|
| 1 | 1 | `0x005B40B0` | `LoadFileFromMIX` | Universal MIX-by-name lookup; 109 callers; primary asset entry | FULL | Low — used by every YR asset load |
| 2 | 1 | `0x004739F0` | `CCFileClass__Constructor` | CCFile wrapper around MIX cache + buffered IO; 147 xrefs | FULL | Low |
| 3 | 1 | `0x005349C0` | `FUN_005349C0` (mislabeled) — actually `Init_Theater` | Opens `temperat.mix`/`isotemp.mix`/`<theater>md.mix`/`isotemmd.mix`; loads `ISO%s.PAL` and `UNIT%s.PAL`; chains to #4. **Rename during execution.** | FULL | Medium — chains to TS-era tile categories; verify YR tile-set count |
| 4 | 1 | `0x00545150` | `CDFileClass__Constructor` (mislabeled) — actually `Read_Theater_TileSets_INI` | THE BRIDGE TILE LOADER. Reads `[General]` `BridgeSet`/`WoodBridgeSet`/`BridgeMiddle1/2`/`BridgeBottomLeft1/2`/etc., iterates `TileSet%04d`, calls #5 + `LoadFileFromMIX`. **Rename during execution.** | FULL | Medium — `WaterBridge` and other tile-set IDs default to TS values; verify YR overrides |
| 5 | 1 | `0x005447C0` | `IsometricTileTypeClass__Constructor` | Per-tile-type ctor, struct size 0x30C; called from #4 | FULL | Low |
| 6 | 1 | `0x00686B20` | `ScenarioClass__Full_Init` | Top of bridge-load chain; calls #3, then map-pack readers | MEDIUM | Low |
| 7 | 1 | `0x00547020` | `TMP_Loader` | TMP file parser (already documented HIGH; verify integration with #4) | LIGHT (verify, don't redo) | Low |
| 8 | 1 | `0x005471B0` | `TMP_ReadSlopeType` | TMP slope-type init (already documented HIGH; verify) | LIGHT | Low |
| 9 | 2 | `0x0069E580` | `SHP_Resolve` | SHP descriptor resolver — loads file via CCFile if absent | FULL | Low |
| 10 | 2 | `0x0069E740` | `SHP_frame_data_getter` | Per-frame compressed pixel data ptr (13 callers) | FULL | Low |
| 11 | 2 | `0x0069E7E0` | `SHP_frame_rect_getter` | Per-frame bounds (49 callers) | MEDIUM | Low |
| 12 | 2 | `0x004373B0` | `Standard_SHP_blitter` | RLE-Zero decompressor at draw time | FULL — codec details matter | Low |
| 13 | 2 | `0x00437A10` | `Extended_SHP_blitter` | Format-3 SHP decompressor | FULL | Low |
| 14 | 2 | `0x0072F350` | `PaletteLoad` | Loads `palette.pal` + 5 side palettes via CCFile | FULL | Low |
| 15 | 2 | (inline @ `0x005349C0`) | n/a — inline 256-iter unpack | The `pcVar5 = LoadFileFromMIX(...PAL); ...256-iter; RGB << 2` block | FULL — extract exact unpack | Low |
| 16 | 2 | `0x00684620` | `ScenarioClass__Read_Scenario` | Top-level scenario INI walker | MEDIUM | Low |
| 17 | 2 | `0x00686730` | `ScenarioClass__Read_Scenario_INI` | Main scenario walker | MEDIUM | Low |
| 18 | 2 | `0x00689E90` | `ScenarioClass__Read_INI_Basic` | `[Basic]` parser | LIGHT | Low |
| 19 | 2 | `0x00743270` | `ScenarioClass__Read_Units_Section` | `[Units]` parser | LIGHT | Low |
| 20 | 2 | `0x0071CA70` | `TerrainClass__Read_Map_Section` | `[Terrain]` parser | LIGHT | Low |
| 21 | 2 | `0x005FD2E0` | `ReadMapOverlayPacks` | `[OverlayPack]`+`[OverlayDataPack]` Base64→LCW→bytes | FULL — bridges live in overlay range `0xCD..0xE6` HIGH, `0x4A..0x63` LOW | Low |
| 22 | 2 | `???` | `Read_IsoMapPack5` (paged-out, byte-search needed) | `[IsoMapPack5]` decoder. String at `0x0081FFF4`, xrefs landed in data pages — must locate via byte-search or follow runtime caller. | FULL | Low — but failure to locate is a known risk |
| 23 | 2 | `0x00551FF0` / `0x00552060` / `0x00552390` | `LCWPipe__Constructor` (3 variants) | LCW-pipe ctors (decompression) | MEDIUM | Low |
| 24 | 2 | `0x005523E0` / `0x00552450` | `LCWStraw__Constructor` | LCW-straw ctors | MEDIUM | Low |
| 25 | 3 | `0x004018C0` | `AudioIndex__Read` | vtable+0x24 read of compressed sample bytes | MEDIUM | Low |
| 26 | 3 | `0x004016F0` | `AudioIndex__OpenSample` | Opens sample by name (CCFile path) | MEDIUM | Low |
| 27 | 3 | `0x00401640` | `AudioIndex__GetFormat` | AUD header reader (sample rate/format/flags) | FULL — need byte layout | Low |
| 28 | 3 | `0x00401C00` | `SampleTracker__LoadSample` | Full sample load: opens, allocates, streams in chunks | FULL | Low |
| 29 | 3 | `0x0040ACD0` | `IMA_ADPCM__DecodeSample` | Westwood IMA-ADPCM codec | FULL — codec details | Low |
| 30 | 3 | `???` | CSF parser entry (NOT FOUND via keyword search) | String-table reader for `*.csf` files. Locate via 4-byte ` FSB` magic byte-pattern (`20 46 53 42`) or follow chain from a string-translation lookup at runtime. | FULL — must locate first | **HIGH unknown — could be inlined in main UI init; flag risk** |
| 31 | 3 | `0x00534FA0` | `InitSideMixFiles` | Loads side-specific MIXes per faction during init | LIGHT | Medium — TS may have had different sides; verify |
| 32 | 3 | `0x004A38D0` | Real `CDFileClass__Constructor` (155 xrefs — distinct from #2/#4 mislabel) | Verify which is actual ctor; resolve mislabel pollution | LIGHT | Low |
| 33 | 3 | (caller of #1) | Misc MIX-cache callers (sample 5-10 from `LoadFileFromMIX` xref tree) | Confirm bridge TMPs and bridge SHPs flow through #1 vs #2 | LIGHT | Low |
| 34 | 3 | `0x0057B440` | `MapClass__ApplyBridgeTile` | Live consumer — confirm it uses tile data populated by #4/#5 | LIGHT (read-once, no decompile-deep) | Low |
| 35 | 3 | `0x00576200` / `0x00570AE0` | `MapClass__UpdateBridgeEdgeTiles_High/Low` | Live consumers, confirm tile-data source | LIGHT | Low |
| 36 | 3 | `0x00756590` | `VXL_Section_Rasterizer` | Verify our VXL parser output feeds this correctly (parity check, not re-investigation) | LIGHT (verify only) | Low |
| 37 | 3 | `0x005BD5C0` | `HVA_Load_File` | Verify our HVA parser output matches (parity check) | LIGHT | Low |

**Phase 1 checkpoint rule:** After functions #1-#8, pause and summarize the
bridge-load chain end-to-end before starting Phase 2. If Phase 1 reveals the
chain branches differently than mapped (e.g., `Init_Theater` is a thin wrapper),
the plan is revised before burning effort on the format gaps.

**Sizing check:** 37 functions, in the "Large" band but well-grouped. Phase 1 is
the bridge-load story (8 functions, FULL); Phase 2 fills format gaps (16
functions, mixed depth); Phase 3 covers stragglers + parity verification (13
functions, mostly LIGHT). Skipping VXL/HVA/TMP-detail keeps scope honest.

## 4. Detail Checklist

The executor must extract these specifics during research:

### Magic numbers and file signatures
- MIX header magic / flag byte (`HasChecksum=0x00010000`, `HasEncryption=0x00020000`)
- MIX index entry size (12 bytes: 4-byte CRC + 4-byte offset + 4-byte length — verify)
- SHP frame header layout, compression-format byte (0/1/2/3 — verify all 4)
- TMP magic (already in existing report — verify it's still authoritative)
- PAL — confirmed 768-byte raw RGB×256, with `<<2` upscale. Verify no header.
- CSF magic ` FSB ` — 4 bytes, then header layout
- AUD header (sample rate u16, sample size u8, flags u8, compression u8, ...)
- LCW (.MAP pack) chunk format — segment headers
- IsoMapPack5 — Base64 alphabet, segment size, LCW chained?

### Bit flags and masks
- MIX `HasChecksum`/`HasEncryption` flag interaction
- SHP per-frame flag byte (compression mode)
- Tile-set INI flag keys: `MarbleMadness=`, `NonMarbleMadness=`, `MorphRange=`, `AllowToPlace=`, `AllowBurrowing=`, `AllowTiberium=`
- CellClass overlay byte ranges: `0xCD..0xE6` (HIGH bridge), `0x4A..0x63` (LOW bridge)
- AUD compression types (1=ADPCM-IMA-WW, 99=ADPCM-IMA, 0=raw)

### State machine states
- None expected for parsers (they're loaders, not state machines). Confirm.
- LCW decoder *does* have a state machine (cmd-byte → run-length / literal /
  back-reference / done) — extract all 4 commands and their byte encodings.

### INI keys to verify
- All `Bridge*` keys (see Section 5)
- `BridgeSet=`, `WoodBridgeSet=`, `WaterBridge=`, `TrainBridgeSet=`
- Theater suffix keys (`Suffix=`, `Extension=`, `MixExtension=`, `IsoExtension=`, `IsoPaletteName=`)
- TileSet keys: `SetName=`, `FileName=`, `TilesInSet=`, `MarbleMadness=`, `NonMarbleMadness=`, `MorphRange=`, `AllowToPlace=`, `AllowBurrowing=`, `AllowTiberium=`

### Struct offsets to extract
- `IsometricTileTypeClass` — 0x30C bytes (already partially documented; verify bridge-relevant fields)
- `MixFileClass` — header pointer, index pointer, file count, encryption flag
- `CCFileClass` — wraps `RawFileClass` + `BufferIOFileClass` + cache hit
- AUD header — verify ALL fields, not just sample-rate
- Map pack section descriptor (length, base64-decoded length, LCW-decoded length)

### Clamps, rounding, off-by-ones
- TMP per-tile pixel buffer sizing (existing report covers; verify edge-case slopes)
- LCW back-reference clamp (max distance / max length)
- AUD chunk-size clamp at end of file
- PAL `<<2` upscale — verify no `min(255)` clamp needed (top 6 bits only)
- IsoMapPack5 segment size — first segment may be smaller than the rest

### Edge cases to test
- MIX with no checksum, no encryption (most YR `*.mix`)
- MIX with encryption (some legacy paths)
- SHP frame with compression byte 0 (uncompressed)
- TMP tile with no extra graphics (no damaged variant)
- TMP tile **with** extra graphics (bridge-damage tiles — the priority case)
- Empty `OverlayPack=` (some maps)
- Map with no bridges (no high-bridge or low-bridge overlay bytes present)
- AUD with 0-length first chunk
- CSF with empty values

### Timing / ordering
- Theater init runs once per scenario load — confirm not per-tick
- MIX cache populated lazily per file open
- Bridge tile data must be loaded BEFORE map `[OverlayPack]` is decoded (or the
  overlay bytes have nothing to render against). Verify ordering in
  `ScenarioClass__Full_Init @ 0x00686B20`.
- Confirm the load order: `Init_Theater` → tile-set INI → tileset MIX file loads → map packs → unit/terrain placements

### TS-legacy flags
- See Section 7

### Vtable dispatches
- `CCFileClass` vtable — `Read` is at vtable+0x24 (used by HVA parser per existing doc; confirm consistent for SHP/TMP/AUD/PAL too)
- `MixFileClass` vtable — index lookup vs sequential scan
- LCW pipe/straw — abstract class hierarchy (Pipe/Straw split)

## 5. INI Keys in Scope

| Key | Section | Default | Suspected Purpose | Currently Parsed in Rust? |
|-----|---------|---------|-------------------|----------------------------|
| `BridgeSet` | [General] | 19 | Tile-set ID for standard concrete bridge | Yes (verify against gamemd default) |
| `WoodBridgeSet` | [General] | 80 | Tile-set ID for wooden bridge | Yes |
| `TrainBridgeSet` | [General] | 37 | Tile-set ID for railroad bridge | Yes |
| `WaterBridge` | [General] | 76 | Tile-set ID for water-spanning bridge | Yes |
| `BridgeTopLeft1` | [General] | 1 | Bridge corner variant 1 | Yes |
| `BridgeTopLeft2` | [General] | 2 | Bridge corner variant 2 | Yes |
| `BridgeTopRight1/2` | [General] | 4/5 | Bridge corner | Yes |
| `BridgeBottomLeft1/2` | [General] | 6/6 | Bridge corner | Yes |
| `BridgeBottomRight1/2` | [General] | 3/3 | Bridge corner | Yes |
| `BridgeMiddle1` | [General] | 7 | Bridge straight section variant 1 | Yes |
| `BridgeMiddle2` | [General] | 12 | Bridge straight section variant 2 | Yes |
| `BridgeStrength` | [General] | 1500 | HP per bridge segment | Yes (different report) |
| `WoodBridgeStrength` | [General] | (verify) | HP for wood bridge | Verify |
| `BridgeVoxelMax` | [General] | 3 | Max debris pieces on destruction | Yes |
| `BridgeExplosions` | [General] | TWLT026,TWLT036,TWLT050,TWLT070 | Anim list on destruction | Yes |
| `DestroyableBridges` | [General] | yes | Master toggle | Yes |
| `Suffix=` | [Theaters] | per theater | Theater file suffix (e.g., "T" for temperate) | Yes |
| `Extension=` | [Theaters] | per theater | File extension override | Yes |
| `MixExtension=` | [Theaters] | per theater | MIX file extension | Yes |
| `IsoExtension=` | [Theaters] | per theater | Isometric tile suffix | Yes |
| `IsoPaletteName=` | [Theaters] | per theater | Tile palette filename | Yes |
| `[TileSet0000]..[TileSetXXXX]` | own sections | n/a | Per-tile-set definitions (`SetName`, `FileName`, `TilesInSet`, etc.) | Yes |
| `BridgeRepairHut=` (on [Building] type) | per BuildingType | varies | Marks a building as a bridge repair hut | Yes (different module) |
| `TooBigToFitUnderBridge` | per Unit | varies | Vertical clearance | Yes |
| `ZFudgeBridge` | [General] | 7 | Z-height fudge for tall units under bridges | Verify |

The executor should **read the actual default values from the binary** (the
constructor at `RulesClass::Constructor` sets these before INI parse), not just
trust ini/ files. The `[Theater]` section names and structure should be cross-
checked against `Init_Theater @ 0x005349C0`.

## 6. Caller & Integration Map

| Caller Address | Calls Into | When Invoked | Should Executor Decompile? |
|----------------|------------|--------------|----------------------------|
| `0x00686B20` (`ScenarioClass__Full_Init`) | `Init_Theater @ 0x005349C0` | Once per scenario load | YES — top of chain |
| `Init_Theater @ 0x005349C0` | `Read_Theater_TileSets_INI @ 0x00545150` | Inside scenario init, after MIX open | YES — bridge entry |
| `Read_Theater_TileSets_INI @ 0x00545150` | `LoadFileFromMIX @ 0x005B40B0` (per tile) | Per-tile during theater init | YES — confirm bridge TMPs flow here |
| `Read_Theater_TileSets_INI @ 0x00545150` | `IsometricTileTypeClass__Constructor @ 0x005447C0` | Per tile entry | YES |
| `LoadFileFromMIX @ 0x005B40B0` (109 callers) | (callees: MIX cache, CCFile fallback) | Universal | NO bulk — sample 3-5 callers (ScenarioRead, ArtINI, AudioInit) to confirm the function is path-agnostic |
| `0x004018C0` (`AudioIndex__Read`) | (vtable+0x24 read) | During sample stream | YES — for codec |
| `0x005FD2E0` (`ReadMapOverlayPacks`) | LCW pipe/straw | During scenario load | YES — bridges live in overlay |
| (TBD) `Read_IsoMapPack5` | (paged-out) | During scenario load | YES — but locate first |

**Where this hooks into Rust today:**
- MIX cache: [src/assets/mix_archive.rs](src/assets/mix_archive.rs)
- TMP loading: [src/assets/tmp_file.rs](src/assets/tmp_file.rs), [src/assets/tmp_decode.rs](src/assets/tmp_decode.rs)
- Theater glue: [src/map/theater.rs](src/map/theater.rs)
- Map file: [src/map/map_file.rs](src/map/map_file.rs)
- Bridge runtime: [src/bridge_re.rs](src/bridge_re.rs), [src/sim/bridge_state/](src/sim/bridge_state/)
- Bridge atlases: [src/render/bridge_atlas.rs](src/render/bridge_atlas.rs), [src/render/bridge_railing_atlas.rs](src/render/bridge_railing_atlas.rs)

**Callers explicitly NOT investigated (with justification):**
- VXL section rasterizer `0x00756590` and runtime VXL draw — already covered in [VXL_RASTERIZER_DISPATCH_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VXL_RASTERIZER_DISPATCH_GHIDRA_REPORT.md) and [VOXEL_RENDERING_ANALYSIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/VOXEL_RENDERING_ANALYSIS.md). Just verify Rust output feeds it correctly.
- TMP per-pixel blitter `0x00547CF0` — runtime render, not asset parse.
- Building-type `BridgeRepairHut` reader at `0x00460E8D` — building-type INI, not asset parse.
- All 109 `LoadFileFromMIX` callers — sample only, the function is generic.
- Anything in [MAPCLASS_COMPLETE_DECODE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_COMPLETE_DECODE.md) — runtime layout.

## 7. TS-Legacy Risk Register

Consolidated TS-legacy concerns the executor must verify before reporting:

- **`BridgeRepairHut=` on building types** — confirmed YR-active (used to flag
  buildings that can repair bridges). NOT TS-legacy.
- **TileSet IDs default to TS values** — `BridgeSet=19`, `WoodBridgeSet=80`,
  `TrainBridgeSet=37`, `WaterBridge=76` — these IDs are loaded from TS-era
  defaults. **Verify YR's `temperatmd.ini`/`urbanmd.ini`/`snowmd.ini` override
  them, and that the loaders read the *md* files not the base files.**
- **`TrainBridgeSet=37`** — YR doesn't ship trains in skirmish. The tile set may
  exist but never be referenced by maps. Confirm whether the loader still
  processes it (allocates memory) or short-circuits when no tiles reference it.
- **Lunar / Desert theaters** — TS-era theaters that YR repurposes. Confirm
  bridge-tile loading works the same way for them as for Temperate/Snow/Urban.
  `desertmd.ini`/`lunarmd.ini` may have bridge-tile IDs that point to nonexistent
  tilesets. Don't assume Temperate behavior generalizes.
- **`MarbleMadness=` and `NonMarbleMadness=` tile-set keys** — TS-era debug/cheat
  feature. Verify whether YR still parses these (likely yes, harmlessly) and
  whether it loads alternate tile data for them.
- **`MorphRange=` on tile sets** — TS slope-morph feature. The TMP loader may
  allocate slope-variant frame data even when not needed. Verify YR uses this.
- **CSF parser** — not located via keyword search. **Risk:** the CSF reader may
  be heavily inlined inside the UI init path, or it may use `xcc.csf` /
  `language.csf` paths that aren't indexed by string search. The executor must
  locate it via 4-byte byte-pattern search (` FSB` = `20 46 53 42`) before
  proceeding. If the parser cannot be located in 1 hour of effort, **document
  it as unresolved** — do not invent.
- **AUD compression formats** — gamemd supports both `WW IMA ADPCM` and `IMA
  ADPCM`; confirm only one is actively used in YR audiomd.mix to avoid
  documenting a code path that's never hit.
- **Mislabeled `CDFileClass__Constructor`** at multiple addresses — Ghidra
  symbol pollution. Trust decompilation only, not the label.

## 8. Current Rust Implementation Surface

All 11 major formats are already implemented in Rust. This investigation is a
**parity audit**, not an implementation kickoff. Per-file mapping:

| Format | Rust file(s) | Bridge-relevant special handling? |
|--------|--------------|------------------------------------|
| MIX | `src/assets/mix_archive.rs`, `mix_crypto.rs`, `mix_hash.rs` | No |
| SHP | `src/assets/shp_file.rs`, `shp_decode.rs` | No |
| TMP | `src/assets/tmp_file.rs`, `tmp_decode.rs` | **Yes** — `has_damaged_data` flag for bridge damage variants |
| VXL | `src/assets/vxl_file.rs`, `vxl_decode.rs` | No |
| HVA | `src/assets/hva_file.rs` | No |
| PAL | `src/assets/pal_file.rs` | No |
| CSF | `src/assets/csf_file.rs` | No |
| AUD | `src/assets/aud_file.rs` | No |
| INI | `src/rules/ini_parser.rs` | No |
| Map | `src/map/map_file.rs` | Partial — bridge state in `sim/bridge_state/` |
| Theater | `src/map/theater.rs` | **Yes** — loads `isotem.mix`, `isosnow.mix`; theater-specific TMP/PAL chains |
| VPL | `src/assets/vpl_file.rs` | No |
| Bink | `src/assets/bink_*.rs` | No (out of scope here) |
| Bridge RE helpers | `src/bridge_re.rs` | Direct — overlay damage stepping, connected sections |

The executor's job is to compare each gamemd parser's behavior to the matching
Rust file and **flag** divergences (not fix them). Bink is out of scope (FMV
playback is post-load and not bridge-relevant).

## 9. Deferred Open Questions

The scoping pass surfaced these but couldn't resolve them — the executor must
explicitly answer or re-document as unresolved:

1. **Where does `Read_IsoMapPack5` actually live?** String at `0x0081FFF4` xrefs
   into data pages (`0x007E6038` / `0x007E6054`). Either follow runtime
   call-site, or byte-search for the function that consumes that string.
2. **Where is the CSF parser?** Not surfaced via keyword search. Locate via
   ` FSB ` magic byte-search.
3. **Is blowfish actually used in any vanilla YR MIX?** The codebase supports
   it, but if no shipped MIX uses it the path may be cold. Confirm by checking
   one or two retail `*.mix` headers.
4. **Are the two `CDFileClass__Constructor` mislabels** (at `0x00545150` and
   `0x004A38D0`) — which is the real ctor? Resolve label pollution.
5. **Does `Init_Theater` chain differently for Lunar/Desert** vs Temperate/
   Snow/Urban? The two theater families may take different code paths.
6. **What is the *exact* bridge-tile load ordering** within
   `Read_Theater_TileSets_INI`? Is `WaterBridge=76` loaded before or after the
   `BridgeSet=19` group? Determines whether map overlay decoding can reference
   them in any order.
7. **Does any tile set's `FileName=` point to a missing TMP** in vanilla? If so,
   what's the failure mode (silent skip / error / fallback)?
8. **AUD: is there a difference between `audio.mix` and `audiomd.mix`** in
   header layout, or is `*md` just additional samples?

## 10. Execution Strategy

**Recommended:** Multi-phase execution with subagent batching per phase.

- **Phase 1 (Core, ~3-4 hours):** Single `/re-investigate` session focusing on
  the bridge-load chain (functions #1-#8). Do NOT batch this — it's a single
  story that needs continuous context. Pause at the checkpoint and write a
  Phase-1 summary before Phase 2.
- **Phase 2 (Depth, ~4-5 hours):** Batched subagents — split the 16 functions
  into 3 groups of ~5 functions each (group A: SHP `0x0069E580`/`0069E740`/
  `0069E7E0`/`004373B0`/`00437A10`; group B: PAL `0072F350` + Map packs
  `005FD2E0` + IsoMapPack5 hunt; group C: ScenarioClass readers `00684620`/
  `00686730`/`00689E90`/`00743270`/`0071CA70` + LCW `00551FF0`/`00552060`/
  `00552390`/`005523E0`/`00552450`).
- **Phase 3 (Context & Edges, ~3-5 hours):** Single session for AUD chain (#25-
  #29), CSF byte-search (#30), straggler verification (#31-#33), and parity
  spot-checks against Rust (#34-#37).

**Document split rule:** if Phase 2 + 3 produce more than ~2000 lines combined,
split into per-format `*_GHIDRA_REPORT.md` files instead of one monolithic
`ASSET_PARSING_BRIDGES_GHIDRA_REPORT.md`. Bridge-load chain and the Phase 1
findings stay in the main file.

**Rename-during-execution checklist** (have the executor do these in Ghidra
during Phase 1, save_program after):
- `0x005349C0`: `FUN_005349C0` → `Init_Theater`
- `0x00545150`: `CDFileClass__Constructor` (wrong) → `Read_Theater_TileSets_INI`
- Resolve which of `0x00545150` and `0x004A38D0` is the true `CDFileClass__Constructor` — leave the wrong one with a `_NotCtor` suffix or `FUN_*` until verified.

## 11. Success Criteria

The executed research document(s) must:

- [ ] Answer every question in Section 1 — especially the end-to-end bridge-load
      chain narrative (#1).
- [ ] Include every function from Section 3 (or explicitly justify omission with
      "covered in <existing report>" or "could not locate, see Section 9").
- [ ] Resolve every deferred question from Section 9, or re-document each as
      unresolved with what was tried.
- [ ] State **"Active in YR: Yes / No / Conditional (which flag)"** for every
      finding.
- [ ] Cite Ghidra addresses for every HIGH-confidence claim. Distinguish
      verified-from-decompilation vs inferred-from-string-search.
- [ ] For each format, end with a **"Rust parity status"** subsection naming
      every divergence found between gamemd's parser and our Rust parser.
- [ ] Include the bridge-tile defaults table (`BridgeSet`, `WoodBridgeSet`,
      etc.) **as set by gamemd's RulesClass constructor** — not just from INI.
- [ ] Document the LCW codec end-to-end (4 commands, byte encoding) since both
      `OverlayPack` and `IsoMapPack5` use it for bridges.

## Sources

- **Ghidra addresses sampled (verified by decompilation during scoping):**
  - `0x005B40B0` (LoadFileFromMIX), `0x004739F0` (CCFileClass ctor),
  - `0x00545150` (Read_Theater_TileSets_INI mislabeled),
  - `0x005349C0` (Init_Theater), `0x005447C0` (IsometricTileTypeClass ctor),
  - `0x0069E580` (SHP_Resolve), `0x0072F350` (PaletteLoad),
  - `0x004018C0` (AudioIndex__Read), `0x00401C00` (SampleTracker__LoadSample),
  - `0x005BD5C0` (HVA_Load_File — already in existing report)

- **Docs searched:**
  - `docs/` (in-repo)
  - `docs/research/` (145+ standalone)

- **INI files checked:**
  - `ini/rules.ini`, `ini/rulesmd.ini`, `ini/art.ini`, `ini/artmd.ini`
  - Theater files (referenced, parsed during execution): `temperatmd.ini`,
    `snowmd.ini`, `urbanmd.ini`, `desertmd.ini`, `lunarmd.ini`, `newurbanmd.ini`

- **Related plans:**
  - `2026-05-06-bridges-tier1-ini-parser-plan.md` — runtime bridge parsing (different scope)
  - `2026-05-07-bridges-tier2-*` — runtime damage state machine (different scope)
  - `2026-05-07-bridge-display-table-investigation-plan.md` — runtime display table (different scope)
  - `2026-05-10-vxl-hva-file-format-investigation-plan.md` — VXL/HVA format (already executed; out of scope for this plan)
