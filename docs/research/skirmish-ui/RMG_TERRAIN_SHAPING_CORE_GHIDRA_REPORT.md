# RMG Terrain-Shaping Core (post-water → pre-radar phases) — Ghidra Research Report

**Address(es):** `0x0059B740` (green spread), `0x005A35F0` + `0x005A2F50`/`0x005A33F0`/`0x006B2A70`/`0x006B3E60`/`0x006B3A80`/`0x006B3850`/`0x006B4100`/`0x006B4240` (hills), `0x005A38C0`/`0x005A3AE0`/`0x005A4280`/`0x005A4B60`/`0x005A45E0` (LAT/trees/rocks), `0x0058EBC0`/`0x0058D620`/`0x0058EF10`/`0x005A19E0`/`0x005A17F0`/`0x005A1350` (map-type-3/4 passes), `0x0059C580` (water-3/4 driver), `0x005A95B0` (tech buildings), `0x005981F0` (RMG settings loader), `0x00595680` (MapSeedClass constructor defaults)
**Investigation Mode:** coverage-map
**Claimed Scope:** Formula-level behavior of the generation phases inside `FUN_00598960` between the water/base-terrain seeding stage and the radar rebuild: green-LAT spread, hill/height-field generation, LAT/rough/sand patching, tree scattering, rock overlays, map-type-3/4 region terracing + bridge/water-edge passes, tech-building placement, and the RMG settings (`RMGMD.INI`) loader with defaults.
**Non-Scope:** water seeding internals (`0x0059A6C0` — RMG_WATER_SEED doc; `0x0059C920`/`0x0059D510` internals deferred here), region partition internals (RMG_REGION_PARTITION doc), tiberium placement (RMG_TIBERIUM doc), start scoring (RMG_START_POINT_SCORING / RMG_START_GENERATION docs), RNG internals (RMG_RNG_SEED doc), `.SED`/dialog/preview plumbing (SKIRMISH_* docs).
**Confidence:** High for decompiled formulas/constants cited by address; Medium where labeled; deferred items listed in §8.
**Active in YR:** Conditional — all of this runs only on the random-map (`.SED`) launch/preview path via `FUN_00598960`. Within that path, per-phase conditions are listed inline.

## 1. Overview

Between water seeding and the radar rebuild, `FUN_00598960` runs a fixed stage
pipeline (§5) that converts the water/region skeleton into finished terrain:
a green-LAT spread pass, a corner-height-field hills system driven by
`Ruggedness`, theater-dependent LAT/rough/sand patch painting with tree
scattering and (TEMPERATE only) rock overlays, extra region-terracing and
bridge passes for map types 3/4, and neutral tech-building placement.
All randomness comes from `g_MapGenRng @ 0x00ABE890` (verified per call site
below), so the whole pipeline is deterministic from `MapSeed+0x74`.

A dedicated settings file **`RMGMD.INI`** (string `0x0082BDCC`, loaded by
`0x005981F0`) supplies `[General]` RMG keys (`MaxTrees`,
`RMGMinimumTiberium`, `RMGMaximumTiberium`, vegetation/light vectors, ore-patch
lamp lists). None of these keys exist in `rules(md).ini` — this resolves why
local INI scans find nothing.

## 2. State Layouts / Key Fields

### 2.1 RMG scratch cell array `DAT_00ABED10` (stride 0x50, W×W where W = `g_PathfinderLinearMapWidth @ 0x0089C2DC`)

| Offset | Type | Purpose | Evidence |
|---|---|---|---|
| +0x00 | packed (i16 x, i16 y) | cell coord; (0,0) = unused slot (rejected via coord-equality helper `0x0050E470`) | decompile `0x005A35F0`, asm `0x005A3F9F..0x005A3FBA` |
| +0x08 | f64 | hills: target height delta (levels); water-adjacent seeded 0.5 | decompile `0x005A2F50`, `0x005A33F0` |
| +0x10 | f64 | hills: random-walk velocity | decompile `0x005A2F50` |
| +0x18 | f64 | P(rough patch) per cell | decompile `0x005A38C0`, `0x005A3AE0` |
| +0x20 | f64 | P(green patch) per cell (non-TEMPERATE main-fn fill: 0.001, unread by `0x005A4280`) | decompile `0x00598960`, `0x005A3AE0`, `0x005A4280` |
| +0x28 | f64 | P(sand patch) per cell (TEMPERATE); P(rough) read by `0x005A4280` (non-TEMPERATE fill: 0.005) | decompile `0x005A3AE0`, `0x005A4280` |
| +0x38 | i32 | region id (−1 none; `0x0058D620` uses −2 pending / −3 carved) | RMG_REGION doc; decompile `0x0058D620` |
| +0x3C | i32 | patch id / region scratch (patch id = y*0x200+x) | decompile `0x005A4B60` |
| +0x45 | u8 | water/shore-adjacency lock flag | decompile `0x005A33F0` |
| +0x47 | u8 | tree-region visited flag | decompile `0x005A45E0` |

Accessor `0x0058C2A0` (coord→entry); diamond bounds test `0x005AC230` against
`DAT_00ABED04` (min) / `DAT_00ABED08` (max) with the four-way test
`min < x+y && x−y < min && y−x < min && x+y <= max`.

### 2.2 Corner height grid `DAT_00B0B6EC` (hills; (W+1)×(H+1) nodes of 8 bytes, dims `DAT_0087F914`+1 × `DAT_0087F918`+1, origin `DAT_0087F90C/0x0087F910` cached to `DAT_008759A8/AC`, dims cached to `DAT_008759A4/A0`)

| Offset | Type | Purpose |
|---|---|---|
| +0x00 | i32 | corner height in 1/15-level units (level*0xF), clamped 0..0xB4 (12 levels) |
| +0x04 | u8 | locked (adjacent overlay/occupier/water-flag/non-morphable tile or out-of-diamond) |
| +0x05 | u8 | modified/visited this operation |

Evidence: decompile `0x006B2A70`, `0x006B3E60`, `0x006B3850`.
Undo stack for one corner operation: `DAT_00B0B654` (12-byte entries {x,y,old_h},
standard DynamicVector append pattern with object `DAT_00B0B650`, capacity
`DAT_00B0B658`, count `DAT_00B0B660`, growth flag/step `DAT_00B0B65D`/`DAT_00B0B664`).

### 2.3 MapSeedClass (`0x00ABDFD8`) fields new to this report

| Field | Meaning | Default (constructor `0x00595680`) | Evidence |
|---|---|---|---|
| +0x2BC | `RMGMinimumTiberium` | 2500 (0x9C4, outer ctor `0x00595740`; RMGMD.INI overrides to 900) | verified via decompile_function 0x00595740 2026-07-20 |
| +0x2C0 | `RMGMaximumTiberium` | 5500 (0x157C, outer ctor `0x00595740`; RMGMD.INI overrides to 1050) | verified via decompile_function 0x00595740 2026-07-20 |
| +0x2C4 | vector<BuildingType> `TemperateOrePatchLamps` | empty | decompile `0x005981F0`; PUSH `0x0059879D` |
| +0x2E0 | vector<BuildingType> `SnowOrePatchLamps` | empty | decompile `0x005981F0`; PUSH `0x00598871` |
| +0x2FC | `MaxTrees` (tree-count multiplier) | 500 (outer ctor `0x00595740`; retail RMGMD.INI sets 600) | verified via decompile_function 0x00595740 2026-07-20; consumers `0x005A3F5B`, `0x005A4437` |
| +0x304/+0x308 | stage/attempt counters (water driver increments +0x308) | 0 | decompile `0x0059C580`, `0x00598960` tail clears |

Constructor option defaults (dword writes `0x00595680`): theater=0, maptype=1,
Resources=1, Ruggedness=0, Time=1, WaterAmount=0, NumPlayers=2, Tiberium=0,
TiberiumLayout=0, Vegetation=0, UrbanPresence=0, Width=0, Height=0,
Accessibility=0, RegionSize=0, Seed=−1. (verified via decompile_function 0x00595680)

### 2.4 Theater tile/tileset globals used by these phases

Loaded from theater INI `[General]` (temperatmd.ini values shown; key names
verified in `ini/temperatmd.ini:46..104`):

| Global | Address | Theater key |
|---|---|---|
| `g_ClearTile` | (named global; used `0x006B3850`) | `ClearTile` (=0) |
| `g_RampBase` | (named global; used `0x006B3850`) | `RampBase` (=9) |
| `g_RoughTile` | `0x00ABC2B8` | `RoughTile` (=13) |
| `g_SandTile` | `0x00ABB104` | `SandTile` (=33) |
| `g_GreenTile` | `0x00AA0E18` | `GreenTile` (=41) |
| `g_ClearToSandLat` | (used `0x00486790`, span 0x10) | `ClearToSandLat` (=34) |
| `g_ClearToGreenLat` | (used `0x004867B0`, span 0x10) | `ClearToGreenLat` (=42) |
| CliffSet base | `0x00AA1020` (span 0x28 tiles) | `CliffSet` (=10) — **CORRECTED 2026-07-20**: this global is CliffSet, NOT WaterSet; the water-set base is `0x00AA0738` (verified via disassemble_bytes 0x00545DC0..0x00545E23 write-binding pattern + LAT_GROUPS_AND_SLOPE_FIXUP doc's 0x005455B5 key table + decompile_function 0x004863D0 whose structure matches the cliff/impassable predicate already modeled by `TheaterCliffRanges::is_cliff_or_impassable_tile`) |

`0x004863D0` ("cell is cliff-or-impassable" — NOT "water-ish"; corrected
2026-07-20, see row above; consequence for §3.2(a): the hills `+0x45` flag
marks shore/cliff/impassable adjacency, and plain WaterSet water cells are
protected from hills by the corner-lock's non-morphable-tile test instead)
tests tile index against CliffSet(+0x28),
four 4-tile sets with subtile exceptions (`DAT_00AA073C`, `DAT_00ABB110`,
`DAT_00AA1050`, `DAT_00AA10A0` — waterfall sets, identities YELLOW),
`DAT_00ABBEBC`(+0x14), `DAT_00ABAD24`(+4), `g_BridgeSet_TileSetBase`(+0x10),
`g_WoodBridgeSet_TileSetBase`(+0x10), `DAT_00ABC2C8`(+2), `DAT_00AA101C`(+0x1C).
(verified via decompile_function 0x004863D0; unnamed set identities in §8 YELLOW)

## 3. Core Logic

RNG note: every `Random__Next 0x0065C780` call site cited below loads
`ECX = 0x00ABE890` (`g_MapGenRng`) — verified in asm for `0x005A3AE0`
(all sites in `0x005A3B58..0x005A4226`), `0x005A4B60` (`0x005A4E6E`),
`0x005A45E0` (`0x005A47F2`, `0x005A481C`, `0x005A49B8`). Gaussian helper
`0x005980C0` (Box-Muller at `0x00ABDFB8` scratch; verified in RMG_TIBERIUM doc)
draws from the same stream. Uniform-int helper `0x00598030(min,max)` =
`ftol(rand × (max−min+1) / 2^32 + min)` with defensive rejection
(verified via disassemble_function 0x00598030).

### 3.1 `0x0059B740` — green-LAT spread (ALL map types; stage missing from the prior generator report)

1. Collect list: for every cell whose tile is green-LAT (`0x004867B0`: tile ==
   `g_GreenTile` or in `[g_ClearToGreenLat, +0x10)`), check the 4 even
   directions (0,2,4,6 = N/E/S/W steps via `MapCoord_StepByDir_GetCell`); each
   clear-tile neighbor is appended to a vector.
2. `count = min(list_len / 3, 1000)`; repeat `count` times: draw a uniform
   index into the vector (one `Random__Next` + rejection), remove that entry
   (shift-down), set that cell's tile `+0x38 := g_GreenTile`, then append its
   clear 4-neighbors to the vector.

Effect: green terrain organically grows into adjacent clear cells by up to
`edge/3` (capped 1000) cells. (verified via decompile_function 0x0059B740)

### 3.2 Hills (`RMG: Creating hills`) — `0x005A35F0` pipeline (ALL map types)

**(a) `0x005A33F0` water-adjacency seeding.** For each cell: if it is a shore
piece (`CellClass__IsShorePieceTile`), find the first in-diamond clear neighbor
over the 8 `g_DirectionOffsets` directions and set that neighbor's scratch
`+0x08 := 0.5` (bits 0x3FE00000 hi) and `+0x45 := 1`; if instead the cell is
water-ish (`0x004863D0`), same scan but only `+0x45 := 1`. First matching
neighbor only (`break`).

**(b) `0x005A2F50(MapSeed)` height random-walk.** With R = Ruggedness (+0x44):
- **Early-out: if `R × 0.0025 < 0.025` (R < 10) the whole pass is skipped —
  no hills at all.**
- Per scratch cell (row-major): smooth from already-processed W (x−1,y) and
  N (x,y−1) neighbors: `height += h[nbr]`, `vel += v[nbr]` then both `× 0.5`;
  the NW diagonal (x−1,y−1) is the reference for the tilt term
  `local_60 = Σ(h[nbr] − h[NW])`, clamped to `± (R × 0.0001 + 0.1)`.
  Out-of-diamond neighbor contributes `vel += R × 0.0025` instead.
- If flagged `+0x45`: height clamped ≥ 0, velocity forced to 0.0025
  (bits 0x3F647AE1_47AE147B).
- Draw 1 (velocity): Gaussian, rejection-sampled into
  `[−vel, R×0.005 − vel]` (window re-centered when out of `±0.025` band);
  `vel += draw`.
- Draw 2 (height): Gaussian, rejection-sampled so the resulting height stays
  in `[−2.0, 2.0]`, biased by the tilt term; `height += draw`.
- Final pass: every height double is `ftol`-truncated to a whole number.

**(c) `0x006B2A70` corner-grid build.** Allocates `DAT_00B0B6EC`; each corner's
height = `slope_corner_table[cell.slope][corner] + cell.level × 0xF`; a corner
is **locked** when an adjacent cell (checked over the cell at (x,y), (x−1,y−1),
(x,y−1) plus water tests `0x006B2520`) has an overlay (`cell+0x44 != −1`), an
occupier (`cell+0xE4 != 0`), the water flag `scratch+0x45`, a tile that is
neither morphable (`IsoTileType+0x2E0 != 0`), clear, nor 0xFFFF, or is
out-of-diamond.

**(d) per-cell height push (`0x005A35F0` main loop).** For each used scratch
cell: `n = |ftol(cell.level + scratch_height − current_level)|` where
`current_level` = `0x006B4100` (min of the 4 corner heights / 0xF, or the
cell's own level byte when the cell is ineligible). Then `n` times:
- `0x006B4240` picks a 4-bit corner mask: all-corners-equal → all unvisited
  corners; raising → unvisited corners below the max; lowering → unvisited
  corners above the min.
- `0x006B3E60(dir, …, coord, mask)` adjusts each masked, unlocked corner by
  ±0xF (one level), clamped to `[0, 0xB4]`, records undo, marks visited, and
  runs `0x006B3A80` **slope propagation**: any 8-neighbor corner differing by
  more than 0xF is pulled along one level (recursively); hitting a locked
  corner fails the whole operation and replays the undo stack.

**(e) `0x006B3850` finalize.** Per cell whose 4 corners include a modified one
and whose corner spread `max−min < 0x10`, and which is in-diamond, has no
overlay/occupier, is not water-flagged, and has a morphable/clear/0xFFFF tile:
`level (+0x11B) = min/15`; corner deltas `(corner − min)` are matched against
the 19-entry pattern table `PTR_DAT_0083FF18` → slope index `+0x11C`; tile
`+0x38 = g_ClearTile` for slope 0 else `g_RampBase + slope − 1`.

**Ramp corner pattern table** (index = slope type; corner order NW,NE,SE,SW;
units 1/15 level; entry 0 = flat {0,0,0,0} at runtime `DAT_00B0B6DC`):
1 {0,15,15,0} · 2 {0,0,15,15} · 3 {15,0,0,15} · 4 {15,15,0,0} ·
5 {0,0,15,0} · 6 {0,0,0,15} · 7 {15,0,0,0} · 8 {0,15,0,0} ·
9 {0,15,15,15} · 10 {15,0,15,15} · 11 {15,15,0,15} · 12 {15,15,15,0} ·
13 {0,15,30,15} · 14 {15,0,15,30} · 15 {30,15,0,15} · 16 {15,30,15,0} ·
17 {0,15,0,15} · 18 {15,0,15,0}
(verified via read_memory 0x0083FF18 / 0x0083FDD8)

**(f) 2×2 ramp-quad cleanup (`0x005A35F0` tail).** Over all cells with quad
offsets (0,0),(1,0),(1,1),(0,1): a quad of slopes {5,6,7,8} in that order is
flattened in place (each cell: tile := 0xFFFF, `+0x11A := 0`, `+0x11C := 0`,
level unchanged); a quad of slopes {11,12,9,10} is flattened with
**level += 1**. (verified via decompile_function 0x005A35F0)

### 3.2-impl Hills implementation contract (verified 2026-07-20, Task 12)

Live re-decode of the hills pipeline for the Rust port. Evidence: `decompile_function`
+ `disassemble_bytes` on `0x005A35F0`, `0x005A33F0`, `0x005A2F50` (2026-07-20).

**Pipeline order (`0x005A35F0`):** `0x005A33F0` water-seed → `0x005A2F50` walk →
`0x006B2A70` corner build → per-cell height-push loop (`0x006B4100`/`0x006B4240`/
`0x006B3E60`) → `0x006B3850` finalize → 2×2 quad cleanup tail. The corner engine
(`0x006B2A70`/`0x006B4100`/`0x006B4240`/`0x006B3E60`/`0x006B3A80`/`0x006B3850`/
`0x006B2520`) is decoded in `RMG_HILLS_CORNER_ENGINE_GHIDRA_REPORT.md`.

**Water-seed (`0x005A33F0`) — predicate identities (corrected):** iterate all real
cells. A **shore-piece** cell (`CellClass__IsShorePieceTile` ≈ the port's
`TileIds::is_shore_piece`, shore range) → first in-diamond **clear** neighbor over the 8
`g_DirectionOffsets` gets scratch `+0x08 (height) = 0.5` and `+0x45 = 1`, then `break`.
Otherwise a **cliff/obstacle** cell (`0x004863D0` — NOT "water-ish"; it tests CliffSet
`0x00AA1020`+0x28, cliff-ramps `0x00ABBEBC`+0x14, water-caves `0x00ABAD24`+4, bridge/
wood-bridge sets +0x10, destroyable-cliffs `0x00ABC2C8`+2, and four sub-tile-gated
waterfall ranges — the port's `TheaterData::is_cliff_or_impassable_tile`) → first
in-diamond clear neighbor gets `+0x45 = 1` only, then `break`. On a generated map the
obstacle branch fires on cliff tiles from the cliff-drop pass; the waterfall ranges are
unreachable.

**Walk (`0x005A2F50`) — FP constants (all `read_memory`/disasm 2026-07-20):**
`0x007EDAA8=1e-4`, `0x007E3860=0.1`, `0x007EDAA0=0.0025`, `0x007E44E8=0.005`,
`0x007ED7C8=0.025`, `0x007E1738=0.5`, `0x007E2800=0.0`, `0xEDA98=2.0`. `R` =
Ruggedness (`MapSeed+0x44`). Precompute `tilt_clamp = R*1e-4 + 0.1`,
`out_vel = R*0.0025`, `half_step = R*0.005`. **Early-out: if `out_vel < 0.025` (R<10)
the whole walk is skipped.** Per scratch cell in linear (row-major) order, coord≠(0,0):
1. `vel = 0`; `nw_ref = in_diamond(x-1,y-1) ? h[NW] : 0`; `tilt = 0`.
2. W neighbor (x-1,y): in-diamond → `h += h[W]`; `vel = v[W]` (copy); `tilt = h[W]-nw_ref`.
   out-of-diamond → `vel = out_vel`; h unchanged.
3. N neighbor (x,y-1): in-diamond → `h += h[N]`; `vel += v[N]`; `tilt += h[N]-nw_ref`.
   out-of-diamond → `vel += out_vel`; h unchanged.
4. `h *= 0.5`; `vel *= 0.5`.
5. tilt clamp: `tilt>0 → +tilt_clamp`, `tilt<0 → -tilt_clamp`, `tilt==0 → 0`.
6. if `+0x45` flag: `h = max(h,0)`; `vel = 0.0025` (exact bits 0x3F647AE147AE147B).
7. **Draw 1 (velocity):** `lo=-vel`, `hi=half_step-vel`, `center=0`, `scale=0.025`;
   if `hi < -0.025 || 0.025 < lo` then `scale=(hi-lo)*0.5`, `center=scale+lo`; then
   `do { g = Gaussian()*scale + center } while (g<lo || g>hi)`; `vel += g`.
8. **Draw 2 (height):** `lo=-2-h`, `hi=2-h`, `center=clamp(tilt,lo,hi)`, `scale=vel`
   (the just-updated velocity); if `hi < center-scale || scale+center < lo` then
   `scale=(hi-lo)*0.5`, `center=scale+lo`; then
   `do { g = Gaussian()*scale + center } while (g<lo || g>hi)`; `h += g`.
9. Final pass: every height `= trunc(h)` (ftol, RMG truncating CW). Gaussian =
   `FUN_005980C0(ECX=0xABDFB8)` — the shared RMG Box-Muller (port's `Gaussian`).

**Main height-push loop (`0x005A35F0`):** per used scratch cell, `target = cell.level +
scratch_height`; `n = |trunc(target - current_level)|` where `current_level` comes from
`0x006B4100`; then `n×` { `mask = 0x006B4240(...)`; `0x006B3E60(coord, mask)` }.

**2×2 quad cleanup tail (verified `0x005A35F0`):** quad offsets NW(0,0) NE(1,0) SE(1,1)
SW(0,1). A quad anchored on a **slope-5** cell whose four cells have slopes {5,6,7,8}
(in NW,NE,SE,SW order) → flatten each (tile 0xFFFF, sub_tile 0, slope 0, level
unchanged). A quad anchored on a **slope-11** cell with slopes {11,12,9,10} → flatten +
`level += 1`. Consumes no RNG.

### 3.3 LAT / patches — TEMPERATE (`MapSeed+0x38 == 0`): `0x005A38C0` then `0x005A3AE0`

**`0x005A38C0(MapSeed)` probability setup.** `V = Vegetation(+0x5C) × 0.01`.
Non-shore cells with `+0x28 == 0.0`: `+0x28 := 0.005` (sand), `+0x18 :=
V × 0.02` (rough), `+0x20 := 0.005` (green). Shore-piece cells (when any cell
of their 5×5 neighborhood is in-diamond): `+0x28 := 0.005`, `+0x20 := 0.0`,
`+0x18 := V × 0.02 × 10`.

**`0x005A3AE0(MapSeed)` painter.** (asm-verified constants)
1. Clears all `+0x3C` patch ids.
2. Draws three patch-size means, each uniform `ftol(rand × 21/2^32 + 20.0)` =
   **[20,40]** (rejection >40 defensive): mean_rough, mean_sand, mean_green
   (drawn in that order; asm `0x005A3B58..0x005A3BE6`, constants
   `0x007EDAD0` = 21/2^32, `0x007E44C8` = 20.0).
3. Per clear cell (slope 0, no overlay `+0x44==−1`, no occupier `+0xE4==0`,
   not water-flagged): sequential probability test with fresh uniform [0,1)
   draws: r1 < `+0x18` → **rough**; else r2 < `+0x28` → **sand**; else
   r3 < `+0x20` → **green**; else nothing. For a hit: patch size =
   Gaussian × 20 + mean, rejection-clamped to [4,80] (fallback window
   38/42 unreachable for means ≤ 40); place patch via `0x005A4B60(cell,
   g_RoughTile|g_SandTile|g_GreenTile, ftol(size), y*0x200+x, 0)`.
4. Full-map `CellClass__ApplyLAT_and_SlopeFixup` pass.
5. **Trees:** `count = ftol((Width(+0x64) × 0.1 + 0.7) × (Vegetation(+0x5C)
   × 0.01) × MaxTrees(+0x2FC))` (asm `0x005A3F41..0x005A3F61`; constants
   `0x007E3860` = 0.1, `0x007EDAC0` = 0.7, `0x007E3808` = 0.01). While
   `count > 0` and < 100 iterations: pick uniform scratch slot (reject empty
   slots and non-clear cells, 200-try fallback keeps last pick), density =
   Gaussian × 0.1 + 0.2 clamped [0.05, 0.4], size = Gaussian × 10 + 25 clamped
   [10,35]; `count −= 0x005A45E0(cell, ftol(size), density)`.
6. **Rocks:** `area2 = (DAT_0087F8E0 + 4) × DAT_0087F8DC × 2`
   (`0x0042B1F0`); target = uniform `[0, area2/200]` (inclusive; one draw);
   attempts capped at `target × 5`. Per attempt: uniform random scratch slot
   (reject empty); cell must have `OverlayTypeIndex(+0x44) == −1`; if tile in
   sand-LAT set (`0x00486790`) → overlay = uniform(0..4) + 0xA8 = overlay
   indices 168–172 = **SROCK01–SROCK05**; else if clear or green-LAT →
   uniform(0..4) + 0xAD = 173–177 = **TROCK01–TROCK05**; sets `+0x11E := 0`
   and calls `CellClass__RecalcAttributes(−1)`. Overlay index↔name mapping
   verified by 0-based position count of `[OverlayTypes]` in rules.ini and
   rulesmd.ini (identical; ore anchor idx 102 = TIB01 cross-checks the
   convention against RMG_TIBERIUM's verified 0x66 base).

### 3.4 LAT / patches — non-TEMPERATE theaters (`MapSeed+0x38 != 0`)

`FUN_00598960` itself fills every cell's scratch: `+0x28 := 0.005`,
`+0x20 := 0.001` (bits 0x3F50624D_D2F1A9FC), then calls `0x005A4280`:
- Per clear cell (slope 0, **not** water-flagged; NO overlay/occupier check):
  one uniform draw; r < `+0x28` (0.005) → rough patch, size = Gaussian × 15 +
  20 clamped [4,60], placed via `0x005A4B60(cell, g_RoughTile, …)`.
  `+0x20` (0.001) is written but never read on this path.
- ApplyLAT fixup pass, then the same tree loop as §3.3.5 (same formula,
  same constants — asm `0x005A441D..0x005A443D`).
- **No rock overlays** and no sand/green patches on non-TEMPERATE theaters.

### 3.5 `0x005A4B60` LAT patch placer / `0x005A45E0` tree scatterer

**`0x005A4B60(cell, tile, size, patch_id, allow_extra)`**: min-heap
(binary heap keyed by float priority) blob growth from the origin cell.
Pops nearest, sets tile `+0x38 := tile`, admits 8-neighbors that are
in-diamond, clear (with `allow_extra != 0` also `0x00486670`/`0x00486650`
classes — all RMG callers pass 0), not already in this patch
(`+0x3C != patch_id`), slope 0, no overlay, no occupier. Priority =
`sqrt(dx²+dy²) + rand × 5/2^32` (one RNG draw per admitted neighbor,
jitter [0,5)). Places exactly `size` cells or until the frontier empties.

**`0x005A45E0(cell, size, density)`**: grows a region of at most `size × 25`
cells the same heap way (visited flag `scratch+0x47`, blocked by water flag
`+0x45`; priority = distance from origin + jitter [0,5)). Per popped cell that
is clear, unoccupied, overlay-free, and `cell+0xEC != 3` (land type
exclusion): one uniform draw; if `< density` → draw uniform 0..25 →
`sprintf("TREE%d%d", v/10, v%10)` → `TerrainTypeClass__Find_By_Name_Index`
→ `TerrainClass` constructed on the cell. Stops after `size` trees placed or
region exhausted; returns trees placed. **Edge:** v=0 yields "TREE00" which
does not exist in `[TerrainTypes]` (rulesmd has TREE01–TREE25 at keys 16..40)
— native behavior for the failed lookup is deferred (§8, YELLOW).

### 3.5-impl Tree scatterer `0x005A45E0` — implementation confirmations (2026-07-20)

Re-decoded live (`decompile_function 0x005A45E0`) for the Rust port. Confirms §3.5 and
adds:
- **Heap capacity = `size * 25`** (`iVar2 = param_2 * 0x19`); node cursor `local_58`
  caps admissions at `size*25`.
- **Seed:** origin node priority 0; scratch `+0x47` (visited) set on the origin; pop.
- **Per popped cell:** the density draw is INSIDE the eligibility gate — only a cell that
  is `IsClearTile && occupier(+0xE4)==0 && overlay(+0x44)==-1 && land(+0xEC)!=3` draws
  `rand·2.3283064370807974e-10` (= `next_unit()`) and compares `< density`. **The
  `land != 3` check is redundant after `IsClearTile`** (a clear tile is land 0), so the
  port omits it as a proven no-op.
- **On a density hit (RESOLVED 2026-07-20):** `v = ftol(rand · scale + 1.0)` rejected
  `> 0x19` → **`v ∈ [1,25]`**, then `sprintf("TREE%d%d", v/10, v%10)` → `TREE01..TREE25`.
  Pinned via `disassemble_bytes 0x005a482e..0x005a4848`:
  `FILD [ESP+0x44]; FMUL [0x007edad8]; FADD [0x007e1718]; call Math__ftol; CMP EAX,0x19; JA`.
  `read_memory` gives `[0x007edad8] = 0x3E39000000190000` (≈ 25·2⁻³², the embedded scale)
  and **`[0x007e1718] = 0x3FF0000000000000 = 1.0`** (the FADD offset). The `+1.0` offset
  makes the index **1-based**, so `v=0` (the nonexistent `TREE00`) is *unreachable* — this
  supersedes the earlier "`v ∈ [0,25]`, TREE00 is a dead lookup" note (§3.5/§5/§8): the
  native never emits `TREE00`, and the §8-YELLOW "failed lookup" behavior is moot. Port:
  `phases/trees.rs` `TREE_INDEX_K_BITS = 0x3E39_0000_0019_0000`, `TREE_INDEX_OFFSET = 1.0`.
- **Neighbour admission:** in-diamond, scratch `+0x47`==0 (unvisited), `+0x45`==0 (not
  water), cursor `< size*25`; priority = `Sqrt_Approx(d² from ORIGIN) + rand·5·2⁻³²`;
  visited stamped at push. Water flag blocks the whole region; there is no tile/slope gate
  on admission (only the per-pop eligibility decides tree placement).
- **Stops** when `trees_placed >= size` OR the heap empties OR the cursor hits `size*25`;
  returns `trees_placed` (the driver's `count -= placed`).

### 3.5-impl(b) Tree driver + rock pass — implementation confirmations (2026-07-20)

Re-decoded the temperate driver `0x005A3AE0` (LAT painter + trees + rocks) live for the
port. Confirms and adds:
- **Tree count** `= ftol((Width(+0x64)·0.1 + 0.7)·(Veg(+0x5C)·0.01)·MaxTrees(+0x2FC))`
  (asm `0x005a3f41..0x005a3f61`: `FILD +0x64; FMUL 0.1; FADD 0.7; FILD +0x5c; FMUL 0.01;
  FMULP; FIMUL +0x2fc; ftol`). Loop runs while `count > 0 && iter < 100`, `count -= placed`.
- **Tree slot pick** = `FUN_00598030(0, stride²−1)` = `uniform(0, stride²−1)` (asm
  `0x005a3f83`: `[0x0089c2dc]²−1`); then reject the `(0,0)` border coord (`FUN_0050e470`
  vs `MapCoord_Set(0,0)`); 200 non-empty tries, fallback anchor `(0,0)`. Identical to the
  tech-building whole-map anchor pick. `FUN_00598030` disasm (`0x00598030`) proves the
  span·K form: `EAX = max−min+1; FILD span; ... FMUL span; FMUL 0x007ed898(=RANGE_K); ftol`
  → exactly `RmgRng::uniform`.
- **Per-iteration draws (order):** density `= Gaussian·0.1 + 0.2` clamp `[0.05,0.4]`, then
  size `= ftol(Gaussian·10 + 25)` clamp `[10,35]`; then `place_region(cell, size, density)`.
- **Rock pass (temperate only):** `area2 = (H+4)·W·2` (`0x0042b1f0`); `target =
  uniform(0, area2/200)`; `attempts_cap = target·5`. Per attempt: `uniform(0, stride²−1)`
  slot (reject `(0,0)`); if `overlay(+0x44) == −1`: **sand test `FUN_00486790` is
  LAT-range-only** `[ClearToSandLat, +0x10)` (NO sand base) → SROCK `uniform(0,4)+0xA8`
  (168..172); else if `IsClearTile || FUN_004867b0` (green base OR `[ClearToGreenLat, +0x10)`)
  → TROCK `uniform(0,4)+0xAD` (173..177); else neither branch sets an overlay **but the cell
  still counts** (`placed++`). The sand-vs-green predicate asymmetry (range-only vs
  base+range) is verified from both function bodies and reproduced in `phases/rocks.rs`.

Rust status: trees + rocks implemented (`phases/trees.rs`, `phases/rocks.rs`, 2026-07-20),
completing Task 13. Determinism/range tests green; still UNVERIFIED-pending-instrument for
gamemd bit-parity (needs the live-capture oracle).

### 3.6 Map-type-3/4 extra passes (run between region partition and `0x0059B740`)

**`0x0058EBC0` region rebuild + terracing driver.** Pass 1: resets every
region object's cell vector, count, and bbox (minx/miny=9999, w/h=0). Pass 2
(reverse scan of all scratch cells): re-appends each cell to its region's
vector, recounts, and recomputes bboxes. Then `threshold = ftol(2 × genW ×
genH × (RegionSize(+0x70) × 0.005 + 0.05))` (asm `0x0058ED89..0x0058EDA9`;
genW/genH = `MapSeed+0x180/+0x184` read as globals `0x00ABE158/0x00ABE15C`;
constants `0x007E44E8` = 0.005, `0x007E8AE8` = 0.05). For each region not yet
processed: count > threshold → run `0x0058D620` (below); if it returns
success the region object is removed/freed and the scan restarts; count ≤
threshold → region marked done (`+0x1A := 1`).

**`0x0058D620(region)` blob carve + height terracing.** (a) marks all region
cells' region id −2; picks a uniform random region cell as seed; blob size
target = uniform ≈ `[0, count/3]` with span `count/3 − count/8 + 1`; grows a
**directional** blob: initial angle = `rand × 2π/2^32` (constant
1.4629180796e-9 = 2π/2^32), per-step `angle += Gaussian × π/8` (0.392699…),
neighbor priority from `0x0058C6F0(seed, cell, angle)`; carved cells get id −3.
(b) Rebuilds sub-regions from the remaining −2 cells via flood fill
(`0x0058BF70` region constructor). (c) For each new sub-region: collects the
distinct heights of neighboring regions; candidate new heights: all equal `h`
→ {h−4 if h>3, h+4 if h<8}; two heights differing by 4 → both; differing by
8 → midpoint; **map types 3/4 remove candidate 0** (`DAT_00ABE014` =
`MapSeed+0x3C`). Region with < 0x65 (101) cells → merged into the
largest-count unfinished neighbor (adopts its height, transfers cells);
otherwise a uniform draw picks one candidate and every region cell's level
`+0x11B` shifts by `new − old`. Heights are in 4-level (cliff) steps.

**`0x0058EF10` bridge + region rebuild.** `0x004A8BF0(0)` (touched); then
`MapClass__PlaceBridgeRamp_Low(cell, −1)` for every cell **stopping early the
first time it returns 0**; clears all scratch region ids; frees all region
objects; re-creates regions by flood fill (`0x0058C800`) from unassigned
cells accepted by `0x005AC370`; if the bridge loop never failed, runs
per-region `0x0058F0C0` and `0x005905D0` (bridge endpoint/connection passes —
touched, not decoded) and frees region sub-vectors.

**`0x005A19E0` water-edge cliff drops.** Per cell: 8-neighbor water adjacency
mask (`MapClass__ComputeBridgeAdjacencyMask_Low`); for straight-edge patterns
0x83 / 0x38 (also required on the dir-3 neighbor): one uniform 0..1 draw;
on 0 → the dir-2 (resp. dir-6) neighbor's level `+0x11B -= 4` and its
scratch region id is copied from the current cell. Pattern 0xE0 (required on
dir-5 and dir-1 neighbors too): no draw — current cell's level `+= 4`, region
id copied from the dir-6 neighbor. Creates 4-level (cliff-height) steps along
straight shorelines.

**`0x005A17F0` tile re-anchor.** ⚠ CORRECTED 2026-07-20: `0x00AA1020` is
**CliffSet**, not WaterSet (see §2.4) — this pass therefore operates on
cliff-set tiles; re-derive the family offsets against the cliff set before
implementing (mode-3/4 scope). Per cell with tile in
`[CliffSet_base@0x00AA1020, +0x28)`: for dir-4 and dir-2 neighbors having the
same tile but a smaller subtile `+0x11A`: `0x005A1350(tile)` picks a random
variant within the tile's family — families at WaterSet offsets {4,5,6},
{8,9,10}, {0xB,0xC,0xD}, {0xE,0xF,0x10}, {0x16,0x17,0x18} (first member +
rand(0..2); second + rand(0..1)×2; third + rand(0..1)), swap pair
{0x1C,0x1D}, family {0x22,0x23,0x24} via `0x00598030`; if the variant differs
from the current tile, the multi-cell tile block is re-placed with
`0x005A6C10` anchored at `neighbor − (subtile / blockW, subtile % blockW)`
where blockW = `IsoTileType+0x2E4`.

### 3.7 `0x0059C580(MapSeed)` — water driver for map types 3/4 (touched)

Increments `+0x308`; if maptype ∈ {3,4} and `WaterAmount(+0x4C) > 0x14` (20):
up to 10 attempts of `0x0059D510(buf,0,0,0)` until nonzero (then `+0x308`++).
Always: up to 10 attempts of `0x0059C920(buf)` until nonzero (then `+0x308`++).
Internals of `0x0059C920`/`0x0059D510` are NOT decoded here (§8 deferred;
also flagged open in RMG_WATER_SEED §6).

### 3.8 `0x005A95B0(MapSeed)` — tech buildings (`RMG: Adding tech buildings`; runs when `MapType != 0`)

- **maptype == 2:** n = uniform 0..2; n passes over all regions; each region
  with `+0x20 > 0` gets `0x00595400()` (per-region placement — touched, not
  decoded).
- **other map types:** house = neutral (via `0x005117D0` →
  `HouseClass__Find_By_Country_Index`); n = uniform 0..4; per building:
  uniform pick from the RulesClass vector at `+0xAE0` (count `+0xAEC`) —
  identity `[General] NeutralTechBuildings` (rulesmd.ini:3082 =
  `CAAIRP,CATHOSP,CAOILD,CAOUTP,CAMACH,CAPOWR`; offset↔key binding is
  MEDIUM, inferred from context); construct `BuildingClass`; up to 100
  attempts: uniform random clear scratch cell (200-try inner reject of empty
  slots); every foundation cell (offset list via type vtbl+0x90, terminated
  0x7FFF,0x7FFF) must be unoccupied, clear, at the anchor cell's exact level,
  land type != 3, pass an on-map check, and not water-flagged; then
  vtbl+0xD8 places at leptons (cell×256+0x80). Failure after 100 attempts →
  building destructed (vtbl+0x20).

### 3.9 `0x005981F0` — RMG settings loader (label-drifted to "CCINIClass__Constructor" — that label is WRONG; record label drift)

Opens **`RMGMD.INI`** (string `0x0082BDCC`, PUSH at `0x005981FC`) and reads
`[General]`:
`RMGMinimumTiberium` → +0x2BC, `RMGMaximumTiberium` → +0x2C0 (defaults =
current field values — carry semantics over the outer-ctor defaults, see 0x00595740),
`RMGLevelLightSettings` → +0x198..1A0, `RMGVegetationMinimums` → +0x294..29C,
`RMGVegetationMaximums` → +0x2B0..2B8, `TemperateAmbientLight` → +0x1B4..1BC,
`SnowAmbientLight` → +0x1D0..1D8, `TemperateAmbientRed/Green/Blue` →
+0x1EC../+0x208../+0x224.., `SnowAmbientRed/Green/Blue` → +0x240../+0x25C../
+0x278.. (3-int difficulty-style vectors), `MaxTrees` → +0x2FC,
`TemperateOrePatchLamps`/`SnowOrePatchLamps` → building vectors +0x2C4/+0x2E0.
`RMGMD.INI` is not a loose file in the retail directory — expected inside a
MIX archive; presence UNVERIFIED locally (§8).

## 4. INI Keys

| Key | File/Section | Used by | Notes |
|---|---|---|---|
| `MaxTrees` | RMGMD.INI [General] | tree count `0x005A3F5B`/`0x005A4437` | programmatic default 500 (outer ctor `0x00595740`); retail RMGMD.INI sets 600 (extracted from ra2md.mix 2026-07-20) |
| `RMGMinimumTiberium` / `RMGMaximumTiberium` | RMGMD.INI [General] | tiberium phase (outside this report's scope) | read into +0x2BC/+0x2C0 |
| `RMGLevelLightSettings`, `RMGVegetationMinimums/Maximums`, `TemperateAmbient*`, `SnowAmbient*` | RMGMD.INI [General] | lighting/vegetation vectors (consumers partly in `0x00599650` tail) | 3-int vectors |
| `TemperateOrePatchLamps`, `SnowOrePatchLamps` | RMGMD.INI [General] | ore-patch lamp buildings (consumer deferred) | building lists |
| `ClearTile`, `RampBase`, `RoughTile`, `SandTile`, `GreenTile`, `ClearToSandLat`, `ClearToGreenLat`, `WaterSet` | theater INI [General] | tile identities §2.4 | e.g. temperatmd.ini:46-63 |
| `NeutralTechBuildings` | rulesmd.ini:3082 [General] | tech building pool (MEDIUM binding) | 6 entries |
| `RequiredForRMG` | theater tileset INIs | tileset availability flag | not read by the phases above |
| — | rules(md).ini | **no RMG tuning keys exist** | verified by full grep |

## 5. Integration Points (corrected stage order inside `FUN_00598960`)

water branch (maptype 3/4 && Water≠0 → `0x0059C580`; else `0x0059A6C0`) →
`0x0059C630` → region partition (`0x0058CF90`, per-region `0x0058E740(
(cnt>8000)+4+!is34)` + `0x0058E9B0`, `0x0058D010`) → **maptype 3/4 only:**
`0x0058EBC0`, `0x0058EF10`, `0x005A19E0`, `MapClass__MarkBridgesForRepair_Low
(0,−1)`, `0x005A17F0` → **`0x0059B740` (green spread — MISSING from the prior
generator report's stage list)** → cell recalc → start loop (`0x00594B50` /
`0x005A1FB0`) → tech buildings `0x005A95B0` (maptype ≠ 0) → tiberium
`0x005A23A0` → scratch clear + region free → recalc → hills `0x005A35F0` →
LAT stage (theater 0: `0x005A38C0` + `0x005A3AE0`; else scratch fill
0.005/0.001 + `0x005A4280`) → `g_MapEditorMode--` → recalc → tiberium queues →
cleanup → `MapClass__InitCellAttributes(1)` → radar. (verified via
decompile_function 0x00598960 this session)

`g_MapEditorMode` is incremented for the whole generation window; its
consumer set is deferred (§8).

## 6. Current Rust Implementation Status

No generation code exists. Rust has UI/scaffolding only:
`src/skirmish_scenarios.rs` (sentinel record; NOTE `RANDOM_MAP_MAX_PLAYERS=4`
drifts from native NumPlayers clamp 2..8), `src/ui/skirmish_shell/*`
(Create Random Map button → log-only stub in `src/app.rs:1333`),
`src/app_skirmish_shell_render/preview.rs` (reads pre-existing `RandMap.img`),
`src/map/waypoints.rs:41` (`[RandomMap] NumPlayers` capacity fallback for
loaded maps). No `.SED` parser, no seed/options model, no terrain generator,
no `RMGMD.INI` reader. (Rust scan this session)

## 7. Coverage Ledger

| Area / function | Status | Evidence | What remains |
|---|---|---|---|
| Stage order + call args in `0x00598960` (in-scope window) | verified | decompile this session | none |
| `0x0059B740` green spread | verified | decompile | vector helpers are stock DynamicVector plumbing |
| `0x005A33F0` / `0x005A2F50` hills seed + walk | verified | decompile + constants | exact FP-op order for bit-identical f64 replay should be taken from asm when implementing |
| `0x006B2A70`/`0x006B3E60`/`0x006B3A80`/`0x006B3850`/`0x006B4100`/`0x006B4240` corner engine | verified | decompile; table reads | `0x006B2520` water test not decompiled (parallel of `0x004863D0`) |
| Ramp corner table (19 entries) | verified | read_memory 0x0083FF18/0x0083FDD8 | none |
| `0x005A38C0` / `0x005A3AE0` TEMPERATE painter | verified | decompile + full asm | none material |
| `0x005A4280` non-TEMPERATE painter | verified | decompile + FILD scan | asm-level constant sweep matched TEMPERATE |
| `0x005A4B60` patch placer / `0x005A45E0` trees | verified | decompile + RNG-instance spot checks | TREE00 lookup-miss behavior (YELLOW) |
| Rock overlays (SROCK/TROCK) | verified | asm 0x005A41DE..0x005A424C + INI position count | none |
| `0x0058EBC0` + threshold | verified | decompile + asm 0x0058ED89 | none |
| `0x0058D620` terracing | verified (mechanism + constants) | decompile | `0x0058C6F0` angle-scoring formula; `0x0058E5D0`, `0x0058D0A0` helpers |
| `0x0058EF10` bridge/regions | touched-not-exhausted | decompile | `0x004A8BF0`, `0x0058F0C0`, `0x005905D0`, `PlaceBridgeRamp_Low` internals |
| `0x005A19E0` cliff drops | verified | decompile | adjacency-mask bit semantics rely on the named helper; not re-derived |
| `0x005A17F0` + `0x005A1350` re-anchor | verified | decompile | none material |
| `0x0059C580` water-3/4 driver | verified (driver only) | decompile | `0x0059C920`, `0x0059D510` internals |
| `0x005A95B0` tech buildings | verified (non-type-2 path) | decompile | `0x00595400` (maptype-2 per-region path); `0x005117D0` country identity |
| `0x005981F0` settings loader | verified | decompile + asm + string table | consumer map for lamp lists / tiberium min-max |
| `MapSeedClass` defaults | verified | decompile 0x00595680 (options) + 0x00595740 (outer: +0x2BC=2500, +0x2C0=5500, +0x2FC=500, +0x30C=4, +0x310=0; 2026-07-20) | none |
| RMGMD.INI presence in retail MIX | not-touched | — | verify via mix extraction |

## 8. Open Questions — Final State of the Investigation Log

- `[RESOLVED]` OQ-03/10/15/23/25/26/27/28 — hills pipeline, stage list, LAT constants, green spread, scratch fields (see §2/§3; evidence: decompile/read_memory calls cited inline)
- `[RESOLVED]` OQ-09 — RMG settings source is `RMGMD.INI`; keys + field map §3.9 (evidence: decompile 0x005981F0; string table read 0x0082BC60/0x0082BDA4; asm 0x005982B8..0x0059877F)
- `[RESOLVED]` OQ-05..08 — mode-3/4 pass roles §3.6 (evidence: decompiles cited)
- `[RESOLVED]` OQ-11 — all sampled draw sites load `g_MapGenRng` ECX=0x00ABE890 (evidence: asm 0x005A3B58 etc.; search_instructions in 0x005A4B60/0x005A45E0)
- `[RESOLVED]` OQ-21/22 — Rust has no RMG; no rules-INI keys (agent scans this session)
- `[RESOLVED]` OQ-29 — `DAT_00B0B650..664` is a stock DynamicVector (undo stack), not a permission gate (evidence: growth-guard pattern identical to region vectors)
- `[RESOLVED]` OQ-30 — tile globals + theater keys §2.4 (evidence: asm operands + temperatmd.ini:46-63)
- `[DEFERRED]` OQ-01 — `0x0059C920`/`0x0059D510` water-3/4 shape internals (category: bounded-cost-too-high; next: dedicated slice, extend RMG_WATER_SEED)
- `[DEFERRED]` OQ-02 — `0x0059C630` re-verify beyond RMG_WATER_SEED §5 (category: out-of-scope; already VF there)
- `[DEFERRED]` OQ-31 — waterfall/shore tileset-base global identities in `0x004863D0` (category: bounded-cost-too-high; next: xref each DAT to the theater-INI reader and its key string) — YELLOW
- `[DEFERRED]` OQ-24 — `g_MapEditorMode` consumer list during generation (category: requires-different-system-context)
- `[DEFERRED]` OQ-18/19 partial — exhaustive edge matrix (0/100 sliders, minimum map) beyond the early-outs documented (category: bounded-cost-too-high; the Ruggedness<10 and WaterAmount≤20 gates ARE documented)
- `[DEFERRED]` OQ-20 — TS-legacy filter: nothing in-scope was found gated on TS flags; veins appear only as overlay-list neighbors (category: out-of-scope for deeper proof)
- `[DEFERRED]` TREE00 lookup-miss native behavior (category: needs-runtime-debugger) — YELLOW
- `[DEFERRED]` `0x0058C6F0` angle-priority formula; `0x00595400` maptype-2 tech path; `0x0058F0C0`/`0x005905D0`/`0x004A8BF0` bridge helpers; `0x006B2520` water test (category: bounded-cost-too-high; named follow-ups)
- `[DEFERRED]` RMGMD.INI presence/content in retail MIX (category: requires-different-system-context; next: mix extraction + dump of actual key values) — YELLOW
- `[DEFERRED]` RulesClass+0xAE0 ↔ `NeutralTechBuildings` binding proof (category: bounded-cost-too-high; next: decompile RulesClass general reader) — YELLOW

## 9. Visual/UI Composition Ledger

Omitted: these phases write sim/map state (tiles, levels, overlays, terrain
objects); they have no paint path of their own. Preview rendering is covered
by [GENERATETERRAINPREVIEW_RANDMAP_DIMENSIONS_COLORS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/skirmish-ui/GENERATETERRAINPREVIEW_RANDMAP_DIMENSIONS_COLORS_GHIDRA_REPORT.md).

## 10. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Whole pipeline draws exclusively from `g_MapGenRng`; draw counts are part of the spec (rejection loops, per-neighbor jitter draws) | §3 note; asm cites | missing | future map-layer RMG module (NOT `sim/`) | Implement one seeded RNG stream; preserve exact draw order/counts incl. rejection resampling | Same seed+options → byte-identical map twice; test `rmg_pipeline_deterministic_from_seed` | Do not reorder probability tests (rough→sand→green) or skip defensive rejections — they consume draws |
| Hills: Ruggedness<10 → no hills; corner engine with ±1-level slope constraint, 19 ramp patterns, level=min/15, cap 12 levels | §3.2 | missing | RMG module + tile/ramp mapping | Reproduce corner grid, locking rules, undo-on-locked-failure, quad cleanup | Fixture seed reproduces gamemd hill layout on a small map (needs runtime capture); unit: ramp-pattern table equality | Do not approximate with per-cell noise; the corner engine's propagation IS the shape |
| TEMPERATE vs other theaters differ structurally: 3 patch classes + rocks vs rough-only, no rocks | §3.3/§3.4 | missing | RMG module | Branch on theater exactly; non-TEMPERATE probabilities are the hardcoded 0.005/0.001 fill | Snow-theater generation never emits SROCK/TROCK or sand/green patches | Do not unify the two paths "for cleanliness" |
| Trees: count = (Width×0.1+0.7)×(Veg×0.01)×MaxTrees; MaxTrees comes ONLY from RMGMD.INI (default 0) | §3.3.5/§3.9 | missing | RMG module + new RMGMD.INI loader | Load RMGMD.INI from MIX; zero-default semantics preserved | With no RMGMD.INI data, MaxTrees falls back to the outer-ctor default 500 (verified via decompile_function 0x00595740 2026-07-20); retail RMGMD.INI (in ra2md.mix) sets 600 | Do not use zero as the missing-file default — the 2026-07-19 "zero trees" claim was refuted by the outer constructor |
| Rocks: quota uniform [0, (H+4)×W×2/200], attempts ×5, SROCK on sand-LAT / TROCK on clear+green, uniform 0..4 variant | §3.3.6 | missing | RMG module + overlay index mapping | Use 0-based [OverlayTypes] positions 168..177 | Generated rocks are only ever SROCK01-05/TROCK01-05 | Do not use key numbers as indices (comment-gap at key 183 shifts positions) |
| Mode-3/4: region terracing in 4-level steps, <101-cell merge, threshold 2WH(RegionSize×0.005+0.05); shoreline ±4 cliff drops at 1/2 chance | §3.6 | missing | RMG module | Implement after base pipeline; needs region-partition port first | Type-3/4 seeds produce plateau heights ≡ 0 (mod 4) | Do not run terracing for map types 0-2 |
| Sentinel capacity: native clamps NumPlayers 2..8 | prior report + constructor | mismatch (Rust max 4) | `src/skirmish_scenarios.rs:14-17` | Align sentinel min/max to 2..8 | Sentinel record shows max 8 | — |

### Stale Docs / Follow-up Docs

- `SKIRMISH_RANDOM_MAP_GENERATOR_00598960_GHIDRA_REPORT.md` §4 item 7: stage
  list omits `FUN_0059B740` (green spread) between the region passes and the
  first cell recalc — add it (evidence: decompile 0x00598960 this session).
  Also replace "create LAT/rocks or alternate cell-boundary data based on
  Theater" with the concrete branch: theater 0 → `0x005A38C0`+`0x005A3AE0`;
  else scratch prob fill (0.005/0.005-unused-0.001) + `0x005A4280`.
- `0x005981F0` carries the wrong Ghidra label `CCINIClass__Constructor` —
  it is the RMG settings loader reading RMGMD.INI (label drift recorded; no
  Ghidra mutation performed in this read-only session).

## Sources

- Ghidra read-only: decompile_function `0x00598960`, `0x005A35F0`,
  `0x005A2F50`, `0x005A33F0`, `0x006B2A70`, `0x006B3E60`, `0x006B3A80`,
  `0x006B3850`, `0x006B4100`, `0x006B4240`, `0x004863D0`, `0x0058C2A0`,
  `0x005AC230`, `0x005A38C0`, `0x005A3AE0`, `0x005A4280`, `0x0059B740`,
  `0x005A4B60`, `0x005A45E0`, `0x0042B1F0`, `0x00486790`, `0x004867B0`,
  `0x00598030`, `0x0050E470`, `0x0058EBC0`, `0x0058EF10`, `0x005A19E0`,
  `0x005A17F0`, `0x0058D620`, `0x005A1350`, `0x00595680`, `0x005A95B0`,
  `0x0059C580`, `0x005981F0`; disassemble_function `0x005A3AE0`,
  `0x00598030`; get_assembly_context `0x005A441D`, `0x0058ED89`,
  `0x005982C8/E7`, `0x00598772`; search_instructions (FILD/PUSH/RNG-instance
  scans); read_memory `0x0083FF18`, `0x0083FDD8`, `0x00B0B6DC`, FP constants
  `0x007EDAD0`, `0x007E44C8`, `0x007EDAC0`, `0x007E3808`, `0x007EDAB0`,
  `0x007E44E8`, string tables `0x0082BC60`, `0x0082BDA4`.
- INI: `ini/temperatmd.ini` [General] tile keys; `ini/rules.ini` +
  `ini/rulesmd.ini` [OverlayTypes] position count, [TerrainTypes] TREE01-25,
  `NeutralTechBuildings` (rulesmd.ini:3082); full-corpus RMG-key grep (agent).
- Prior docs: SKIRMISH_RANDOM_MAP_GENERATOR_00598960, RMG_WATER_SEED,
  RMG_REGION_PARTITION, RMG_TIBERIUM_CREATION, RMG_START_POINT_SCORING,
  RMG_START_GENERATION, RMG_RNG_SEED_MAPGENRNG (coverage-diff agent).
- Rust scan agent (src/ sentinel scaffolding).

---

## Appended 2026-07-20: TREE00 branch & CellClass LandType (+0xEC)

### 5. `0x005A45E0` tree scatterer — the "TREE00 miss" branch is UNREACHABLE

The tree-name draw is NOT uniform 0..25. Disassembly of the draw chain
(@0x005a481c-0x005a4848, verified via disassemble_function 0x005A45E0 +
read_memory 0x007EDAD8/0x007E1718, 2026-07-20):

`ECX=0xabe890` (g_MapGenRng) -> `CALL 0x0065C780` (Random__Next raw u32) ->
FILD u64 -> `FMUL double [0x007EDAD8]` (= 0x3E39000000190000 = 25 * 2^-32 *
(1+2^-32)) -> **`FADD double [0x007E1718]` (= 1.0)** -> Math__ftol ->
reject-redraw while result > 0x19 (25, unsigned JA).

Because of the `+1.0` bias, the truncated result is always in **1..25**
(raw*scale < 25, +1.0 -> [1.0, 26.0), ftol truncates; the JA>25 rejection is
effectively dead paranoia). `sprintf(buf, "TREE%d%d", idx/10, idx%10)` (format
string "TREE%d%d" read via read_memory 0x0082C09C) therefore produces exactly
TREE01..TREE25 — **"TREE00" is never generated**; the premise of the miss
question is refuted from the binary. Prior decompiler readings dropped the FADD
and reported a 0..25 range.

Hypothetical miss behavior (relevant only if [TerrainTypes] were modded): there
is NO -1 guard. `CALL 0x0071DD80` (TerrainTypeClass__Find_By_Name_Index:
linear scan comparing name vs type+0x24, returns -1 on miss — verified via
decompile_function 0x0071DD80, 2026-07-20) feeds straight into
`MOV ECX,[0x00a8e31c]; LEA EAX,[ECX+EAX*4]; MOV EAX,[EAX]` — i.e. an
out-of-bounds `g_TerrainTypeClass_Array[-1]` read — and that garbage pointer is
passed to `TerrainClass__Constructor` 0x0071BB90 (verified via
disassemble_function 0x005A45E0 @0x005a4892-0x005a48aa: no CMP/-1 check).
The per-call placed counter [ESP+0x1c] increments at 0x005a48af regardless
(it is also the JZ target when `operator_new(0xE0)` fails, so the counter
counts accepted density draws, not successful constructions), and the loop
continues.

Placement gate recap at the draw site (@0x005a47be-0x005a47ec): cell must pass
`CALL 0x00486380` (IsClearTile), cell+0xE4 == 0, cell+0x44 == -1, and
**cell+0xEC != 3** (LandType != Rock, see item 6); density accept =
raw * 2^-32*(1+2^-32) (via [0x007ED898]) < param_3.

### 6. CellClass+0xEC = LandType; writers, tile-to-land table, and land 3 = Rock

**Writers** (verified via search_instructions program-wide scan for
`MOV [..+0xEC],`, 2026-07-20): `CellClass__Constructor` @0x0047bc93 (init 0)
and `CellClass__RecalcAttributes` 0x0047D2B0 (all gameplay writes:
0x0047d318/53e/5ef/7c1/843/86e/8aa/b40/b48/d2a), plus `MapClass__Resize`
@0x00565e79/0x0056668b (cell copy/init during resize). No other CellClass
writer exists.

**Value sources in RecalcAttributes** (verified via decompile_function
0x0047D2B0, 2026-07-20):
- Overlay present: LandType = OverlayTypeClass+0x298 (the overlay's parsed
  Land= value); tiberium overlay with slope < 5 forces LandType = 5.
- No tile (index 0xFFFF): LandType = 0 (Clear) unless a non-wall overlay
  supplies its land.
- Tile present, no overlay: **LandType = FUN_00544be0(subtile)** — the
  tile-to-land mapping.
- CliffBackImpassability: rules byte +0x664 (INI key string "CliffBackImpassability"
  at 0x0083C8CC read into RulesClass+0x664 @0x0066f1e6 — verified via
  get_assembly_context 0066f1e6 + read_memory 0x0083C8CC; stock value 2 in both
  rules.ini:319 and rulesmd.ini:409). When == 2, cells adjacent to a cliff face
  (specific neighbor >= Level+4 probes) get LandType forced to 3; in the final
  pass the force applies only if current land is in {0 Clear, 2 Water, 6 Beach,
  8 Ice}.

**The mapping** `FUN_00544be0` (verified via decompile_function 0x00544be0,
2026-07-20): fetches the tile's loaded TMP image via vtable call +0x9C, takes
the subtile record pointer, reads its terrain byte at subtile+0x29, and maps it
through the 16-entry dword table at **0x008288E4** (sole xref = FUN_00544be0;
bytes read via read_memory 0x008288E4, 2026-07-20):

| TMP byte | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| LandType | 0 | 8 | 8 | 8 | 8 | 10 | 9 | 3 | 3 | 2 | 6 | 1 | 1 | 0 | 7 | 3 |

**Land enum binding** (verified via read_memory 0x0081DA24 pointer table +
0x0081DBC0 string block, 2026-07-20): the land-name pointer table at 0x0081DA28
runs Clear(0), Road(1), Water(2), **Rock(3)**, Wall(4), Tiberium(5), Beach(6),
Rough(7), Ice(8), Railroad(9), Tunnel(10), Weeds(11). Cross-checks inside
RecalcAttributes: LandType==10 triggers the tunnel-tube construction against the
four *Tunnels tilesets; tiberium overlay writes 5; wall/track overlay branch
tests land 4 or 9. **So the RMG tree/tech exclusion `land == 3` excludes Rock
(impassable) cells** — TMP terrain bytes 7, 8, 15 (rock/cliff subtiles) and
CliffBackImpassability=2 cliff-shadow cells.

### Unverified (this appendix)

- Identities of cell+0xE4 and cell+0x44 in the scatterer gate (shapes suggest
  occupancy/overlay-index; not verified from struct use this session).
- `TerrainClass__Constructor` 0x0071BB90 behavior when handed the
  out-of-bounds type pointer (unreachable in stock; not traced).
- Vtable slot +0x9C of IsometricTileTypeClass (assumed "get TMP image data"
  from the indexing shape `piVar1[0]*piVar1[1]` and subtile array at +0x10).
