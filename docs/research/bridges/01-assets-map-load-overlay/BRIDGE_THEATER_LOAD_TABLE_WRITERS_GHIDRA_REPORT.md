# Bridge Theater-Load Table Writers - Ghidra Report

**Date:** 2026-05-16
**Scope:** Requested follow-up on the writer paths at `0x005446B1`, `0x00543F36`, `0x005451DC`, `0x00543C42`, and `0x00543E02`.
**Target binary:** `gamemd.exe` / Yuri's Revenge.
**Rust code changes:** None.
**Active in YR:** Yes for `Read_Theater_TileSets_INI @ 0x005451DC`, `FUN_00547230 @ 0x00547230`, and the consumed table data. The table initializers are process static initialization, not a TS-only dormant branch.

## Summary Verdict

The table values behind the `FUN_00547230` C_SHADOW emitter are not runtime-captured mystery data. They are written by static initializer blocks:

- `DAT_00ABC210` is a 10-entry table initialized at `0x00544691`; `0x005446B1` is the first table write.
- `DAT_00ABC2D0` is a 40-entry fallback table initialized at `0x00543F10`; `0x00543F36` is the first table write.
- `DAT_00ABC554` is populated by the live theater TileSet loader at `0x005451DC`, and the loaded file name is `C_SHADOW.SHP`.
- `DAT_00ABC1F8` and `DAT_00AA1098` are runtime tile-class bases derived from `[General] SlopeSetPieces=` and `[General] SlopeSetPieces2=`.
- There is no separate wood table for this emitter. Both `[DAT_00ABC1F8,+10)` and `[DAT_00AA1098,+10)` use the same `DAT_00ABC210` entries.

This corrects the prior open question in [BRIDGE_RENDERING_REMAINING_CASES_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/06-render-presentation-audio/BRIDGE_RENDERING_REMAINING_CASES_GHIDRA_REPORT.md): the exact `DAT_00ABC210` and `DAT_00ABC2D0` values are now decompiled. A live debugger capture is still useful for visual validation, but it is no longer required to recover these values.

## Entry Format

Verified consumer: `FUN_00547230 @ 0x00547230`.

Each entry is 16 bytes:

| Offset | Meaning | Consumer evidence |
|---:|---|---|
| `+0x00` | 1-based SHP frame, `0` means skip | read from `DAT_00ABC210/2D0`, then `CC_Draw_Shape(..., frame - 1, ...)` |
| `+0x04` | required sub-tile | compared against caller `param_2`; mismatch returns |
| `+0x08` | X offset | added to screen X |
| `+0x0C` | Y offset | added to screen Y |

The final screen position is:

```text
x = base_x + entry.dx + 0x1E + g_RadarViewportOffsetX - clip_x
y = base_y + entry.dy + 0x0F + g_RadarViewportOffsetY - clip_y
```

The draw call is:

```text
CC_Draw_Shape(DAT_00ABC554, entry.frame - 1, ..., flags = 0x4601, ..., zheight = 1000)
```

**Evidence:** decompile of `FUN_00547230 @ 0x00547230`.
**Confidence:** HIGH.

## `DAT_00ABC554` Population

`Read_Theater_TileSets_INI @ 0x005451DC` starts by zeroing the five shadow-caster range bases:

```text
DAT_00AA102C = 0
DAT_00AA1030 = 0
DAT_00AA1034 = 0
DAT_00AA1038 = 0
DAT_00AA103C = 0
```

It then constructs `CCFileClass("C_SHADOW.SHP")`. If `DAT_00ABC554` is already non-null, it frees it and writes zero. It then allocates `FUN_00473C00()` bytes, stores the pointer in `DAT_00ABC554`, and loads the file into that buffer with `FUN_00473B10(DAT_00ABC554, size)`.

Important correction: this draw path's SHP pointer is `C_SHADOW.SHP`, not `RAILBRDG.<theater>`. `RAILBRDG` still exists in INI/art data, but `FUN_00547230` uses the pointer loaded from the string `C_SHADOW.SHP @ 0x0082960C`.

**Evidence:** decompile and assembly context of `Read_Theater_TileSets_INI @ 0x005451DC`; string search finds `C_SHADOW.SHP` at `0x0082960C`.
**Confidence:** HIGH.

## Theater-Loaded Base Globals

`Read_Theater_TileSets_INI @ 0x005451DC` reads theater `[General]` keys. The relevant keys are:

| Theater key | Stack var in decompile | Runtime global written during TileSet loop | Used by `FUN_00547230`? |
|---|---:|---|---|
| `BridgeSet` | `iStack_920` | `DAT_00AA0E28` | No |
| `WoodBridgeSet` | `iStack_8D0` | `DAT_00ABAD1C` | No |
| `SlopeSetPieces` | `iStack_8C0` | `DAT_00ABC1F8` | Yes |
| `SlopeSetPieces2` | `iStack_908` | `DAT_00AA1098` | Yes |
| `ShadowCaster=yes` TileSet sections | n/a | `DAT_00AA102C..DAT_00AA103C` | Yes |

During the TileSet loop, the loader compares the current TileSet section index (`iVar11`) against those `[General]` values. If matched, it writes the current running tile-class base (`iVar16` / `iStack_9EC`) to the corresponding global.

For `ShadowCaster=yes`, the loader writes the current running tile-class base to `(&DAT_00AA102C)[local_964]` and increments `local_964`. `FUN_00547230` later checks exactly five roots: `DAT_00AA102C`, `DAT_00AA1030`, `DAT_00AA1034`, `DAT_00AA1038`, and `DAT_00AA103C`.

Tiny detail: the per-tile-type flag at `IsoTileType+0x2E1` is written only when both conditions hold:

```text
ShadowCaster=yes && ShadowTiles != 0
```

The range root is written for `ShadowCaster=yes` even before the `ShadowTiles` nonzero gate.

**Evidence:** decompile of `Read_Theater_TileSets_INI @ 0x005451DC`; string addresses `SlopeSetPieces2 @ 0x008294C0`, `SlopeSetPieces @ 0x008294D0`, `WoodBridgeSet @ 0x00829504`, `BridgeSet @ 0x00829514`.
**Confidence:** HIGH.

## `DAT_00ABC210` Static Table

Writer: static initializer created at `0x00544691`; requested anchor `0x005446B1` is the first write to `DAT_00ABC210`.

Consumer dispatch:

```text
if tile_class in [DAT_00ABC1F8, DAT_00ABC1F8 + 10):
    entry = DAT_00ABC210 + (tile_class - DAT_00ABC1F8) * 0x10
else if tile_class in [DAT_00AA1098, DAT_00AA1098 + 10):
    entry = DAT_00ABC210 + (tile_class - DAT_00AA1098) * 0x10
```

Exact entries:

| Local index | Frame 1-based | Required sub-tile | DX | DY | Draws frame |
|---:|---:|---:|---:|---:|---:|
| 0 | 0 | 0 | 0 | 0 | skip |
| 1 | 0 | 0 | 0 | 0 | skip |
| 2 | 0 | 0 | 0 | 0 | skip |
| 3 | 0 | 0 | 0 | 0 | skip |
| 4 | 13 | 6 | 48 | 12 | 12 |
| 5 | 0 | 0 | 0 | 0 | skip |
| 6 | 14 | 1 | 48 | 12 | 13 |
| 7 | 0 | 0 | 0 | 0 | skip |
| 8 | 0 | 0 | 0 | 0 | skip |
| 9 | 0 | 0 | 0 | 0 | skip |

There is no second wood-specific entry table in this function. The second base (`DAT_00AA1098`) selects the same table by local index.

**Evidence:** decompile of `BridgeSlopeTable_StaticInit_00544691`; consumer decompile of `FUN_00547230`.
**Confidence:** HIGH.

## `DAT_00ABC2D0` Static Table

Writer: static initializer created at `0x00543F10`; requested anchor `0x00543F36` is the first write to `DAT_00ABC2D0`.

The initializer repeatedly calls `CRect::CRect(int,int,int,int) @ 0x0054A120`. That helper writes four ints in order, so each call maps directly to `(frame_1based, required_sub_tile, dx, dy)`.

Consumer dispatch:

```text
if IsoTileType+0x2E1 == 0:
    return
for root in DAT_00AA102C..DAT_00AA103C:
    local = tile_class - root
    if 0 <= local < 0x28:
        entry = DAT_00ABC2D0 + local * 0x10
```

Exact entries:

| Local index | Frame 1-based | Required sub-tile | DX | DY | Draws frame |
|---:|---:|---:|---:|---:|---:|
| 0 | 0 | 0 | 0 | 0 | skip |
| 1 | 0 | 0 | 0 | 0 | skip |
| 2 | 0 | 0 | 0 | 0 | skip |
| 3 | 0 | 0 | 0 | 0 | skip |
| 4 | 0 | 0 | 0 | 0 | skip |
| 5 | 0 | 0 | 0 | 0 | skip |
| 6 | 0 | 0 | 0 | 0 | skip |
| 7 | 0 | 0 | 0 | 0 | skip |
| 8 | 0 | 0 | 0 | 0 | skip |
| 9 | 0 | 0 | 0 | 0 | skip |
| 10 | 0 | 0 | 0 | 0 | skip |
| 11 | 0 | 0 | 0 | 0 | skip |
| 12 | 0 | 0 | 0 | 0 | skip |
| 13 | 0 | 0 | 0 | 0 | skip |
| 14 | 0 | 0 | 0 | 0 | skip |
| 15 | 0 | 0 | 0 | 0 | skip |
| 16 | 0 | 0 | 0 | 0 | skip |
| 17 | 0 | 0 | 30 | 30 | skip |
| 18 | 0 | 0 | 30 | 30 | skip |
| 19 | 0 | 0 | 30 | 30 | skip |
| 20 | 1 | 0 | 30 | 30 | 0 |
| 21 | 1 | 0 | 30 | 30 | 0 |
| 22 | 2 | 1 | 60 | 15 | 1 |
| 23 | 3 | 1 | 60 | 15 | 2 |
| 24 | 4 | 1 | 60 | 15 | 3 |
| 25 | 5 | 0 | 90 | 30 | 4 |
| 26 | 6 | 0 | 60 | 15 | 5 |
| 27 | 7 | 1 | 30 | 0 | 6 |
| 28 | 8 | 0 | 60 | 15 | 7 |
| 29 | 9 | 0 | 60 | 15 | 8 |
| 30 | 10 | 1 | 0 | -15 | 9 |
| 31 | 11 | 1 | 0 | -15 | 10 |
| 32 | 12 | 0 | 60 | 15 | 11 |
| 33 | 0 | 0 | 0 | 0 | skip |
| 34 | 0 | 0 | 0 | 0 | skip |
| 35 | 0 | 0 | 0 | 0 | skip |
| 36 | 0 | 0 | 0 | 0 | skip |
| 37 | 0 | 0 | 0 | 0 | skip |
| 38 | 0 | 0 | 0 | 0 | skip |
| 39 | 0 | 0 | 0 | 0 | skip |

Tiny detail: indices 17-19 carry nonzero offsets but frame zero, so the consumer still returns before using those offsets.

**Evidence:** decompile of `BridgeShadowTable_StaticInit_00543F10`; helper decompile of `CRect::CRect @ 0x0054A120`; consumer decompile of `FUN_00547230`.
**Confidence:** HIGH.

## Requested Adjacent Addresses

`0x00543C42`:

```text
DAT_00AA1040 = 0
DAT_00AA1044 = 0
DAT_00AA1048 = 0
DAT_00AA104C = 0
```

This is adjacent to the five roots used by `FUN_00547230`, but it is not part of that root scan. The consumer stops at `DAT_00AA103C`.

`0x00543E02`:

```text
DAT_00AA105C = 0
DAT_00AA105E = 0
```

This is also adjacent setup data, but no evidence ties it to `FUN_00547230` or the `DAT_00ABC210` / `DAT_00ABC2D0` tables.

**Evidence:** assembly context around the requested anchors.
**Confidence:** HIGH for "not used by this emitter"; LOW for the unrelated gameplay meaning of those adjacent globals, which was outside this scope.

## Current Rust Comparison

Observed in current Rust:

- `src/render/bridge_railing_atlas.rs` still has all-zero `CONCRETE_RAILING_VALUES` and `WOOD_RAILING_VALUES`. That does not match the recovered `DAT_00ABC210` table.
- Rust models two separate 10-entry bridge railing tables. The binary path investigated here has one 10-entry table used by both `DAT_00ABC1F8` and `DAT_00AA1098` ranges.
- Rust loads `railbrdg.<theater>` in `src/render/bridge_railing_atlas.rs`; the binary path investigated here loads `C_SHADOW.SHP` into `DAT_00ABC554`.
- Rust derives railing kind from overlay names and `final_sub_tile`; the binary path investigated here derives the entry from `IsoTileTypeClass+0x294` range membership plus caller sub-tile.

These are renderer parity risks, but this report does not implement fixes.

## Implementation Implications

1. Do not wait for a live debugger capture to fill `DAT_00ABC210`; the static initializer gives exact values.
2. Treat `DAT_00AA1098` as a second tile-class base into the same `DAT_00ABC210` table, not as a second table.
3. Treat `DAT_00ABC2D0` as a 40-entry fallback table, not a five-entry table.
4. Do not call this path `RAILBRDG` without qualification. The verified pointer is `C_SHADOW.SHP`; if `RAILBRDG` railings are rendered elsewhere, that path needs a separate trace.
5. Any Rust port of this exact path needs to key from theater tile-class bases and sub-tile, not from overlay names alone.

## Remaining Open Questions

1. Where exactly are `RAILBRDG1` / `RAILBRDG2` overlay visuals drawn, if at all, relative to this `C_SHADOW.SHP` emitter? This report only proves that `FUN_00547230` is not using `RAILBRDG.<theater>`.
2. What visual role do the `C_SHADOW.SHP` frames play for bridge/slope cells in stock theater assets? The table and draw path are recovered, but a screenshot or frame dump should label the visible effect.
3. The unrelated globals zeroed at `0x00543C42` and `0x00543E02` were not traced beyond proving they are not consumed by `FUN_00547230`.

## Functions And Addresses Verified

- `0x005451DC` - `Read_Theater_TileSets_INI`, live theater TileSet loader.
- `0x00544691` - static initializer for `DAT_00ABC210`; includes requested `0x005446B1`.
- `0x00543F10` - static initializer for `DAT_00ABC2D0`; includes requested `0x00543F36`.
- `0x0054A120` - `CRect::CRect(int,int,int,int)`, four-int helper used by the static initializer.
- `0x00547230` - consumer/emitter for `DAT_00ABC210`, `DAT_00ABC2D0`, `DAT_00ABC554`, `DAT_00ABC1F8`, `DAT_00AA1098`, and `DAT_00AA102C..103C`.
- `0x004802A0` - immediate caller of `FUN_00547230`.
- `0x00543C42` - adjacent zero initializer for `DAT_00AA1040..104C`, not consumed by `FUN_00547230`.
- `0x00543E02` - adjacent zero initializer for `DAT_00AA105C..105E`, not consumed by `FUN_00547230`.
