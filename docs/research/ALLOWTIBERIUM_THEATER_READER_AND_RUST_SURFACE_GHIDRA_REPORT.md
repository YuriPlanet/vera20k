# AllowTiberium Theater Reader And Rust Surface - Ghidra Research Report

**Address(es):** `0x00545150` (`Read_Theater_TileSets_INI`), `0x00544740` (`IsometricTileTypeClass__Constructor`), `0x004838E0` (`CellClass__CanPlaceTiberium`)  
**Investigation Mode:** exhaustive-slice  
**Claimed Scope:** Theater `[TileSetNNNN] AllowTiberium=` read/default/storage into `IsoTileTypeClass+0x306`, how `CellClass+0x38` indexes that runtime tile type for the TIBTRE placement gate, and the minimal current Rust parser/data-model handoff.  
**Non-Scope:** Re-proving the full eight-gate `CanPlaceTiberium` chain, CellClass flag `0x500` semantics, building exception bytes, TIBTRE timing, ore overlay placement effects, and full IsometricTileTypeClass parity.  
**Confidence:** High for the reader/default/storage and current Rust gap; Medium for retail theater file priority because the active YR binary reader was verified but the full CD/MIX archive lookup stack was not re-drained.  
**Active in YR:** Yes. `gamemd.exe` loads the active theater before map cells/overlays, stock YR theater INIs contain `AllowTiberium=true` entries, and TIBTRE `SpreadTiberium(force=1)` reaches `CanPlaceTiberium` for target validation.

## 0. Pre-Investigation Scope Gate

**Target question:** How does the YR binary read theater tile `AllowTiberium`, what default is used when it is absent, where is the byte stored, how does `CellClass+0x38` reach it at runtime, and what is the minimal Rust surface needed so TIBTRE placement can use the same gate?

**Non-goals:** Do not re-investigate all `CanPlaceTiberium` gates; do not study `CellClass+0x140 & 0x500`, building exceptions, source overlay tiberium type, TIBTRE animation timing, or generic tile rendering.

**Evidence needed to mark COMPLETE:** decompile plus assembly for `ReadBool(..., "AllowTiberium", 0)` and the write to `IsoTileTypeClass+0x306`; constructor default for `+0x306`; decompile plus assembly for `CellClass+0x38` bounds/index lookup into `g_IsometricTileTypeClass_Array[index]+0x306`; stock theater INI examples; Rust source line refs showing parser/data-model gap.

**Stop conditions:** Stop after the read/default/storage/runtime lookup and Rust handoff are proven. Defer any broader IsometricTileTypeClass, LAT, bridge, tile animation, or save/load questions.

Prior state row fired: recent reports existed, but this target was an explicit follow-up gap. This report proceeds as gaps plus verification only and does not duplicate broad tile-class coverage.

## 1. Overview

`AllowTiberium` is a per-tileset theater INI boolean, read from each `[TileSetNNNN]` section while the active theater is loaded. The value defaults false, is copied into every `IsometricTileTypeClass` object created for that tileset at byte `+0x306`, and is later consulted by `CellClass::CanPlaceTiberium` through the target cell's `CellClass+0x38` tile-type index.

For TIBTRE, this means a cell can be walkable, flat, buildable land, empty, and still reject ore if its theater tile type did not opt in with `AllowTiberium=true`.

## 2. Class Layout / Key Offsets

| Owner | Offset / global | Type | Purpose | Default / source | Active in YR |
|---|---:|---|---|---|---|
| `IsometricTileTypeClass` | `+0x306` | byte bool | `AllowTiberium`; permits ore/tiberium placement on this tile type | Constructor writes `0`; loader overwrites from `[TileSetNNNN] AllowTiberium`, default `0` | Yes |
| `IsometricTileTypeClass` | `+0x305` | byte bool | `AllowBurrowing`; adjacent field, confirms exact writer neighborhood | Constructor writes `1`; loader default `1` | Yes, but not this gate |
| `IsometricTileTypeClass` | `+0x2E2` | byte bool | `AllowToPlace`; adjacent field, confirms exact writer neighborhood | Constructor writes `1`; loader default `1` | Yes, but not this gate |
| `CellClass` | `+0x38` | signed int | runtime `IsoTileTypeIndex` into `g_IsometricTileTypeClass_Array` | Map load / LAT / bridge code writes this cell tile index | Yes |
| global | `g_IsometricTileTypeClass_Array` / count | pointer array + int | runtime tile type registry indexed by `CellClass+0x38` | rebuilt on theater load | Yes |
| Rust `TilesetLookup` | `morphable_flags` only | `Vec<bool>` | currently stores `Morphable`, but not `AllowTiberium` | `src/map/theater.rs:176`, `321` | Rust gap |
| Rust `ResolvedTerrainCell` | no equivalent | missing bool | no resolved per-cell `allow_tiberium` or lookup helper exists | `src/map/resolved_terrain.rs:74` onward | Rust gap |

## 3. Core Logic

### 3.1 Constructor default

`IsometricTileTypeClass__Constructor` initializes the tile type before the theater reader applies section data.

Key defaults:

- `+0x2E2 AllowToPlace = 1`
- `+0x305 AllowBurrowing = 1`
- `+0x306 AllowTiberium = 0`

Assembly spot-check:

| Address | Evidence |
|---|---|
| `0x005448B5` | `MOV byte ptr [EBP + 0x305],0x1` |
| `0x005448BC` | `MOV byte ptr [EBP + 0x306],BL`, with `BL == 0` on the constructor default path |

This means an absent `AllowTiberium` key rejects tiberium by default. There is no permissive fallback at construction.

### 3.2 Theater reader

`Read_Theater_TileSets_INI @ 0x00545150` iterates `[TileSet0000]`, `[TileSet0001]`, and so on until `TilesInSet` is missing or `-1`. For each tileset section, before per-tile TMP load, it reads:

| Key | Binary call default | Stored field |
|---|---:|---|
| `Morphable` | `0` | `+0x2E0` / decompiler word-alias at `piVar20 + 0xB8` |
| `AllowToPlace` | `1` | `+0x2E2` |
| `AllowBurrowing` | `1` | `+0x305` |
| `AllowTiberium` | `0` | `+0x306` |
| `RequiredForRMG` | `0` | `+0x2E3` |

Decompile evidence:

- `uVar7 = CCINIClass__ReadBool(auStack_85c, s_AllowTiberium_00829208, 0)`
- first tile object path writes `*(undefined1 *)((int)piVar20 + 0x306) = uVar7`
- variant tile object path also writes `*(undefined1 *)((int)piVar20 + 0x306) = uVar7`

Assembly spot-check for the first tile object writer:

| Address | Evidence |
|---|---|
| `0x00546442` | writes the parsed `AllowToPlace` byte to `+0x2E2` |
| `0x00546460` | writes parsed `AllowBurrowing` to `+0x305` |
| `0x0054646D` | writes parsed `AllowTiberium` to `+0x306` |

Tiny details:

- The value is per tileset, not per TMP sub-tile.
- The same parsed byte is copied to every main and replacement tile type allocated under that tileset.
- Missing key returns the call default `0`; malformed or unrecognized bool also falls back to the supplied default in `CCINIClass__ReadBool`.
- `CCINIClass__ReadBool` recognizes first character `0/F/N` as false and `1/T/Y` as true after uppercasing; stock `AllowTiberium = true` is accepted.

### 3.3 Runtime gate through `CellClass+0x38`

`CellClass__CanPlaceTiberium @ 0x004838E0` reaches the theater flag after the earlier target gates. This report only re-checks the final tile lookup:

1. Read `EAX = *(cell + 0x38)`.
2. If `EAX < 0`, return true for this final tile-flag fallback.
3. If `EAX >= g_IsometricTileTypeClass_Array_Count`, return true for this final tile-flag fallback.
4. Otherwise load `tile = g_IsometricTileTypeClass_Array[EAX]`.
5. Read `tile+0x306`.
6. Reject only if that byte is zero.

Assembly evidence:

| Address | Evidence |
|---|---|
| `0x004839C0` | `MOV EAX,dword ptr [EDI + 0x38]` |
| `0x004839C3..0x004839CD` | negative and `>= count` indices branch to success |
| `0x004839CF..0x004839D8` | load pointer from `g_IsometricTileTypeClass_Array[index]`, then `byte ptr [EDX + 0x306]` |
| `0x004839DE..0x004839E0` | `TEST AL,AL`; zero branches to reject |
| `0x004839E2..0x004839E6` | success return |
| `0x004839E9..0x004839ED` | false return |

The out-of-range fallback is easy to miss: invalid tile indices pass this final `AllowTiberium` check, but earlier gates may still reject. Rust should not use this as an excuse to default every normal resolved tile to true; ordinary in-range tiles must honor the parsed byte.

## 4. INI Keys

| File / source | Key | Example line refs | Default when absent | Effect |
|---|---|---|---:|---|
| `temperatmd.ini` | `[TileSetNNNN] AllowTiberium=true` | `ini/temperatmd.ini:181`, `:211`, etc. | false | Sets `IsoTileTypeClass+0x306=1` for that tileset |
| `snowmd.ini` | same | `ini/snowmd.ini:177`, `:206`, etc. | false | Same |
| `urbanmd.ini` | same | `ini/urbanmd.ini:184`, `:214`, etc. | false | Same |
| `urbannmd.ini` | same | `ini/urbannmd.ini:184`, `:214`, etc. | false | Same |
| `desertmd.ini` | same | `ini/desertmd.ini:181`, `:211`, etc. | false | Same |
| `lunarmd.ini` | same | `ini/lunarmd.ini:181`, `:211`, etc. | false | Same |

Stock count scan in this workspace:

| Theater INI | `AllowTiberium=true` count |
|---|---:|
| `ini/temperatmd.ini` | 28 |
| `ini/temperat.ini` | 28 |
| `ini/snowmd.ini` | 25 |
| `ini/snow.ini` | 24 |
| `ini/urbanmd.ini` | 32 |
| `ini/urban.ini` | 29 |
| `ini/urbannmd.ini` | 33 |
| `ini/desertmd.ini` | 28 |
| `ini/lunarmd.ini` | 28 |

File priority finding:

- Binary active YR reader formats and opens `<long-theater-name>MD.INI`, e.g. `TEMPERATMD.INI`, inside `Read_Theater_TileSets_INI @ 0x00545150`.
- `Init_Theater @ 0x005349C0` opens both YR/base theater MIX archive families for asset lookup, but this slice found no merge of base theater INI sections after the `*MD.INI` reader.
- Current Rust tries md INI first and base INI fallback for Temperate/Snow/Urban (`src/map/theater.rs:82`, `:93`, `:104`), which is practical for asset availability but should not be mistaken for a GameMD section merge. For standard YR, md is the authoritative first file.

## 5. Integration Points

| Integration | Verified behavior | Evidence | Active in YR |
|---|---|---|---|
| Scenario/theater load | active theater is initialized before map cells and overlays are decoded | prior `ASSET_PARSING_BRIDGES_GHIDRA_REPORT.md`, `Init_Theater @ 0x005349C0`, `Read_Theater_TileSets_INI @ 0x00545150` | Yes |
| Tile-type registry rebuild | old tile type objects are deleted, then new `IsometricTileTypeClass` objects are allocated per tileset/tile/variant | `Read_Theater_TileSets_INI @ 0x00545150` | Yes |
| Target validation | `CanPlaceTiberium` uses `CellClass+0x38` to look up `IsoTileTypeClass+0x306` after earlier gates | `0x004839C0..0x004839E3` | Yes |
| TIBTRE path | TIBTRE target selection eventually calls `CanPlaceTiberium` through `SpreadTiberium(force=1)` | prior TIBTRE reports; this report does not re-prove full call chain | Yes |

## 6. Current Rust Implementation Status

Current Rust parses theater tilesets into `TilesetLookup`, but the equivalent data stops at filenames, bounds, set names, variants, and `Morphable`:

- `src/map/theater.rs:176` documents `morphable_flags`.
- `src/map/theater.rs:321` parses `Morphable` with default false.
- There is no `AllowTiberium` parser/field in `TilesetLookup`.
- `src/map/resolved_terrain.rs:74` defines `ResolvedTerrainCell`; it has `final_tile_index`, `tileset_index`, land/slope/build/bridge fields, and `accepts_smudge`, but no `allow_tiberium`.
- `src/map/resolved_terrain.rs:499..503` computes `accepts_smudge` via `td.lookup.is_morphable(tile_key.tile_id)`, showing the exact pattern a future `allows_tiberium` boolean or lookup can follow.
- `src/sim/terrain_spawn.rs:167` `can_accept_tiberium` currently uses path-grid walkability, spawner-cell rejection, and resource-node type checks. It has no theater tile flag input.
- `src/sim/terrain_spawn.rs:166` explicitly says existing ore is not a rejection reason and placement is additive; that is a separate TIBTRE mismatch confirmed by other reports.

Minimal Rust handoff:

1. Add per-tileset `allow_tiberium_flags: Vec<bool>` to `TilesetLookup`, defaulting false on absent key.
2. Add `TilesetLookup::allows_tiberium(tile_id: u16) -> bool` mirroring `is_morphable`.
3. Expose the resolved final-cell value to `sim` either as `ResolvedTerrainCell::allows_tiberium` computed from `final_tile_index`, or through a lookup helper available to the TIBTRE validation builder before entering `sim`.
4. Use the final tile index, not the source tile index, because the binary uses runtime `CellClass+0x38`, which is the current tile type after load-time tile resolution/mutation.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| Target question / non-goals / stop conditions | verified | Section 0 | none |
| `IsometricTileTypeClass__Constructor` default for `+0x306` | verified | decompile `0x00544740`; assembly `0x005448B5..0x005448BC` | none |
| `Read_Theater_TileSets_INI` `AllowTiberium` read default | verified | decompile `0x00545150`: `ReadBool(... AllowTiberium ..., 0)` | none |
| Loader writes `+0x306` on first tile object | verified | decompile `0x00545150`; assembly `0x0054646D` | none |
| Loader writes `+0x306` on replacement/variant tile object | verified | decompile `0x00545150` second allocation path | exact assembly address not re-located; decompile is clear |
| `CellClass+0x38` final lookup | verified | decompile and assembly `0x004839C0..0x004839E3` | none |
| `CCINIClass__ReadBool` accepted values/default | verified | decompile `0x005295F0` | none |
| Stock theater `AllowTiberium=true` entries | verified | `rg AllowTiberium ini`; count scan in Section 4 | none |
| md/base theater priority | touched-not-exhausted | `Read_Theater_TileSets_INI @ 0x00545150`, `Init_Theater @ 0x005349C0`, Rust `src/map/theater.rs:82..148` | full archive precedence and absent-md runtime failure path not re-drained |
| Current Rust `TilesetLookup` parser gap | verified | `src/map/theater.rs:176`, `:321`, `rg AllowTiberium src` no hits | none |
| Current Rust `ResolvedTerrainCell` gap | verified | `src/map/resolved_terrain.rs:74`, `:499..503`, `:515..538` | none |
| Current Rust TIBTRE validator gap | verified | `src/sim/terrain_spawn.rs:167` and surrounding function | none |
| Full `CanPlaceTiberium` eight-gate chain | deferred | prior TIBTRE reports | out-of-scope; only final tile flag rechecked here |
| Dynamic mutations of `CellClass+0x38` after bridges/LAT | deferred | prior bridge/tile reports | requires separate tile mutation lifecycle pass |

## 8. Open Questions - Final State of the Investigation Log

- `[RESOLVED] OQ1 - What function reads theater `AllowTiberium`? -> `Read_Theater_TileSets_INI @ 0x00545150`.` (evidence: `0x00545150`)
- `[RESOLVED] OQ2 - What is the key's absent default? -> false/0 from both constructor default and `ReadBool(..., 0)`.` (evidence: `0x005448BC`, `0x00545150`)
- `[RESOLVED] OQ3 - Where is the key stored? -> byte `IsoTileTypeClass+0x306`.` (evidence: `0x0054646D`)
- `[RESOLVED] OQ4 - Is the value per tileset or per sub-tile? -> per tileset, copied into every tile type object created under that section.` (evidence: `0x00545150`)
- `[RESOLVED] OQ5 - How does `CellClass` index the tile type? -> signed `CellClass+0x38` indexes `g_IsometricTileTypeClass_Array` when in range.` (evidence: `0x004839C0..0x004839D8`)
- `[RESOLVED] OQ6 - What happens for negative/out-of-range `+0x38`? -> this final tile gate passes rather than rejects.` (evidence: `0x004839C3..0x004839E3`)
- `[RESOLVED] OQ7 - Does zero `+0x306` reject? -> yes, `TEST AL,AL; JZ false`.` (evidence: `0x004839DE..0x004839E9`)
- `[RESOLVED] OQ8 - Is this active in standard YR? -> yes, stock YR theater INIs set the key and TIBTRE target validation reaches `CanPlaceTiberium`.` (evidence: `ini/*md.ini`; prior TIBTRE reports; `0x004838E0`)
- `[RESOLVED] OQ9 - Which retail theater files contain the key? -> Temperate, Snow, Urban, New Urban, Desert, Lunar md INIs contain true entries; base Temperate/Snow/Urban also contain entries in repo data.` (evidence: `rg AllowTiberium ini`)
- `[RESOLVED] OQ10 - Does current Rust parse the key? -> no; parser currently stores `Morphable` but no `AllowTiberium`.` (evidence: `src/map/theater.rs:176`, `:321`; `rg AllowTiberium src`)
- `[RESOLVED] OQ11 - Does current resolved terrain expose the key? -> no field; closest pattern is `accepts_smudge` from `is_morphable`.` (evidence: `src/map/resolved_terrain.rs:74`, `:499..503`)
- `[RESOLVED] OQ12 - Does current TIBTRE validator consume a theater tile flag? -> no, it uses path grid/resource/spawner checks only.` (evidence: `src/sim/terrain_spawn.rs:167`)
- `[RESOLVED] OQ13 - Should Rust infer this from land type? -> no, binary checks both land buildable and `IsoTileTypeClass+0x306`.` (evidence: `0x0048399C..0x004839E3`)
- `[RESOLVED] OQ14 - Should Rust use final or source tile index? -> final/current tile type, because binary reads runtime `CellClass+0x38`.` (evidence: `0x004839C0`)
- `[RESOLVED] OQ15 - Are malformed bool strings special? -> `CCINIClass__ReadBool` returns supplied default unless first char maps to false or true, so absent/unrecognized means false for `AllowTiberium`.` (evidence: `0x005295F0`)
- `[DEFERRED] OQ16 - Full archive priority if md theater INI is absent.` (category: `requires-different-system-context`; reason: this slice verified active YR md open and Rust fallback, not full CD/MIX error behavior; next-step-if-pursued: trace `CCFileClass` open and `INIClass` read failure branch for missing `<theater>MD.INI`)
- `[DEFERRED] OQ17 - Every writer of `CellClass+0x38` after map load.` (category: `out-of-scope`; reason: bridge/LAT/tile mutation is a separate system; next-step-if-pursued: trace `CellClass+0x38` writers and runtime tile replacement paths)
- `[DEFERRED] OQ18 - Whether save/load persists `+0x306` or only rebuilds from theater.` (category: `out-of-scope`; reason: tile type registry save/load is not needed for TIBTRE parser handoff; next-step-if-pursued: inspect scenario/savegame theater reload path)
- `[DEFERRED] OQ19 - Full IsometricTileTypeClass field parity.` (category: `bounded-cost-too-high`; reason: existing broad report covers many fields, this target only needs `AllowTiberium`; next-step-if-pursued: verify/update `ISOMETRIC_TILE_TYPE_CLASS_GHIDRA_REPORT.md`)
- `[DEFERRED] OQ20 - Exact UI/editor meaning of `AllowToPlace` and `RequiredForRMG`.` (category: `out-of-scope`; reason: adjacent fields used only to confirm writer layout; next-step-if-pursued: targeted reader/consumer scan for those fields)

Adversarial corner-case answers:

- Missing key: rejects, because default false.
- Tile index invalid: final `AllowTiberium` fallback passes, but earlier gates still apply.
- Empty/no-tile Rust filled cells: should not be blindly allowed unless the implementation intentionally models the binary invalid-index fallback and the rest of the cell gates.
- Modded tileset with buildable land but no `AllowTiberium`: rejects.
- LAT/bridge tile replacement: use the current/final tile, not the original map source tile.

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| `AllowTiberium` defaults false and is read per `[TileSetNNNN]` with `ReadBool(..., 0)` | `0x00545150`, `0x005448BC` | missing | `src/map/theater.rs::TilesetLookup`, `parse_tileset_ini` | Parse `AllowTiberium` into a per-tileset bool vector with absent default false | `theater_parse_allow_tiberium_defaults_false` | Do not default unknown tilesets to true because common land is walkable |
| Every tile type object in a tileset receives the same `+0x306` byte | `0x0054646D`, decompile `0x00545150` | missing lookup | `TilesetLookup::allows_tiberium(tile_id)` or equivalent | Map tile id to tileset, return stored bool, false for out-of-range lookup unless a caller explicitly models binary invalid-index fallback | `theater_allow_tiberium_lookup_uses_tileset_bounds` | Do not attach the flag to filename strings only; blank slots still consume tile ids |
| `CanPlaceTiberium` checks runtime `CellClass+0x38`, not the map's original source tile | `0x004839C0..0x004839D8` | missing resolved current tile flag | `src/map/resolved_terrain.rs::ResolvedTerrainCell` or a validation-side lookup helper | Expose `allows_tiberium` for each resolved/final tile used by sim validation | `resolved_terrain_allow_tiberium_uses_final_lat_tile` | Do not use `source_tile_index` after LAT/bridge/tile replacement |
| In-range tile index with `+0x306==0` rejects TIBTRE placement | `0x004839DE..0x004839E9` | terrain spawn too permissive | `src/sim/terrain_spawn.rs::can_accept_tiberium` future validation inputs | Reject otherwise-valid candidates whose resolved tile has `AllowTiberium=false` | `tibtre_spread_rejects_tile_without_allow_tiberium` | Do not collapse this to `PathGrid::is_walkable`; pathability and tile ore permission are separate |
| Binary final tile gate passes invalid/out-of-range tile indices | `0x004839C3..0x004839E3` | unchecked | future exact `CanPlaceTiberium` helper | Decide deliberately whether no-tile cells model the invalid-index fallback or are filtered by earlier map/playfield data | `can_place_tiberium_invalid_tile_index_matches_binary_fallback` | Do not accidentally make every no-tile filled cell ore-permissive without the other gates |
| YR active reader uses md theater INI as authoritative active file | `0x00545150`; `src/map/theater.rs:82..148` for Rust md-first behavior | Rust md-first/base fallback is practical but broader than proven binary merge | `TheaterDef.ini_names`, asset loader docs/tests | Keep md-first; treat base fallback as availability fallback, not a section merge | `theater_load_prefers_md_allow_tiberium_over_base` | Do not merge base and md `[TileSet]` sections unless a separate binary trace proves it |

### Negative Facts / Do Not Do

- Do not infer `AllowTiberium` from `LandType`, `TerrainClass`, walkability, or buildability. The binary checks land buildability and tile `+0x306` separately.
- Do not default absent `AllowTiberium` to true. Both constructor and reader default false.
- Do not use the map source tile after LAT/resolution; the runtime gate reads current `CellClass+0x38`.
- Do not ignore blank or missing tilesets when building tile-id bounds; missing `TilesInSet` terminates the binary loop, while blank `FileName` still consumes slots when `TilesInSet` is present.
- Do not implement this in `sim` by depending on `map/theater` directly if that would violate layering; feed the needed resolved boolean into sim-owned validation data.

### Remaining Uncertainty

- Full absent-md theater INI behavior is not re-drained; active standard YR uses `*md.ini`.
- Full dynamic writer inventory for `CellClass+0x38` is outside this slice.
- Exact save/load persistence of tile type fields is outside this slice.

### Stale Docs / Follow-Up Docs

- `ISOMETRIC_TILE_TYPE_CLASS_GHIDRA_REPORT.md` line `489` is stale for current Rust: it says `Morphable` is missing, but current Rust parses/stores `Morphable` at `src/map/theater.rs:176` and `:321`. Replacement wording: "`parse_tileset_ini` now reads `FileName`, `SetName`, `TilesInSet`, visual variant candidate names, and `Morphable`; it still does not parse `AllowTiberium`, `AllowToPlace`, `AllowBurrowing`, `RequiredForRMG`, theater conversion keys, shadow keys, or per-tile animation keys."
- TIBTRE validation docs that say only "walkable" or "empty walkable" should use: "`CanPlaceTiberium` also requires in-range tile types to have `IsoTileTypeClass+0x306 AllowTiberium != 0`; invalid/out-of-range `CellClass+0x38` passes only this final tile gate."

## Sources

- Ghidra decompiled: `Read_Theater_TileSets_INI @ 0x00545150`
- Ghidra decompiled: `IsometricTileTypeClass__Constructor @ 0x00544740`
- Ghidra decompiled: `CellClass__CanPlaceTiberium @ 0x004838E0`
- Ghidra decompiled: `CCINIClass__ReadBool @ 0x005295F0`
- Ghidra assembly spot-checks: `0x005448B5..0x005448BC`, `0x00546442..0x0054646D`, `0x004839C0..0x004839E3`
- Existing docs referenced: `ASSET_PARSING_BRIDGES_GHIDRA_REPORT.md`, `ISOMETRIC_TILE_TYPE_CLASS_GHIDRA_REPORT.md`, [TIBTRE_CANACCEPTTIBERIUM_REJECTION_GATES_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TIBTRE_CANACCEPTTIBERIUM_REJECTION_GATES_GHIDRA_REPORT.md), [PLACETIBERIUM_SPREAD_GERMINATION_CONSTRAINTS_AND_OVERLAY_FRAME_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PLACETIBERIUM_SPREAD_GERMINATION_CONSTRAINTS_AND_OVERLAY_FRAME_GHIDRA_REPORT.md)
- INI checked: `ini/temperatmd.ini`, `ini/temperat.ini`, `ini/snowmd.ini`, `ini/snow.ini`, `ini/urbanmd.ini`, `ini/urban.ini`, `ini/urbannmd.ini`, `ini/desertmd.ini`, `ini/lunarmd.ini`
- Rust source audited: `src/map/theater.rs`, `src/map/resolved_terrain.rs`, `src/sim/terrain_spawn.rs`
