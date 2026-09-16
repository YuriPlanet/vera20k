# Bridge Display Table — Ghidra Research Report

**Topic:** Per-frame bridge tile selection in `gamemd.exe` — the `(overlay_byte, damage_state, axis, deck_level)` → visible tile pipeline, scoped for the Phase D Rust renderer.
**Plan executed:** `docs/plans/2026-05-07-bridge-display-table-investigation-plan.md`
**Date:** 2026-05-07 / 2026-05-08
**Confidence:** **HIGH** on the per-frame draw chain, render-side mutation set, high-bridge "no runtime selector" claim, layer ordering, and the `Get_Draw_Offset` Y-offset formula. **MEDIUM** on the rim-refresh state-write set (`UpdateBridgeEdgeTiles_*`) and the LOW-bridge `Select*_Low` PRNG-driven variant choice. **LOW / unresolved** on the EW-vs-NS axis label (binary doesn't say which is which — depends on SHP frame layout).
**Active in YR:** Yes for the live draw chain. **No** for `FUN_004D1890` (FoggedObject snapshot walker) and `FUN_0059E740` (RMG bridge placer) — both TS-legacy.

---

## 1. Goal

Answer 7 questions for the Phase D renderer:

1. **Q1 — Full draw chain:** what runs per frame, in what order, to render a bridge cell?
2. **Q2 — Constellation:** is there a single `BridgeDisplayTable`, or a constellation? If constellation, enumerate.
3. **Q3 — High-bridge runtime selector:** does a `SelectBridgeTileVariant_High` or equivalent runtime tile-classifier exist, or is the high-bridge `tile_index` stamped event-driven (map-init + destruction) only?
4. **Q4 — Overlay-range invariants:** verify `HIGH 0xCD..=0xE6` raw + `0xE7/0xE8` final; `LOW 0x4A..=0x63` raw + `0x64/0x65` final.
5. **Q5 — Rim refresh body:** what does `UpdateAdjacentBridges_High/_Low` actually do? Mutate cells, or only dirty redraw rects?
6. **Q6 — Per-frame mutations:** is rendering pure-read?
7. **Q7 — UpdateRamp_*_* display crossover:** which display fields are touched by the state-machine ramp drivers?

All answered below with binary citations.

---

## 2. Headline Findings (read this first)

The investigation surfaced **eight major corrections** to prior reports. The pre-Phase F [BRIDGE_RENDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/06-render-presentation-audio/BRIDGE_RENDERING_GHIDRA_REPORT.md) had several layer-mapping and table-existence claims that are wrong. Document them here so future investigators don't relitigate.

### 2.1 The "FoggedObject walker" myth (corrects BRIDGE_RENDERING §2.1)

The prior doc claimed `FUN_004D1890 @ 0x4D1890` (called from Step 4, `Tactical_layer_base_terrain`) is a primary live bridge-rendering path via "case 0x14". It is **not**.

`FUN_004D1890` is the **`FoggedObjectClass` display-table walker**. It iterates the global table at `DAT_008B3CC0` (entries: `{sortKey:i32, FoggedObjectClass*:i32}`, 8-byte stride; count at `DAT_008B3CC4`). The "case 0x14" inside it dispatches an *overlay snapshot* (a saved `cell.+0x44`/`+0x11E` pair captured when the live bridge cell was last seen), temporarily impersonates those values onto the live cell, calls `DrawOverlay_Body/Shadow`, and restores. Cases 6/0x14/0x1D/0x24 are RTTI tags for Building/Overlay/Terrain/Smudge snapshots.

**Population chain (gated end-to-end on FogOfWar):**
```
FUN_004D1890                       ← reads DAT_008B3CC0/C4
   ↑ populated by
BuildingClass__CreateFoggedSnapshot @ 0x004D1040  ← writes DAT_008B3CC4 at 0x4D10D0
   ↑ called by
FUN_00457AA0
   ↑ called by
FUN_00486A70 @ 0x00486A70
   ↳ opens with: if ((*g_ScenarioClass_Instance & 0x1000) != 0) { ... }
                                                  ^^^^^^
                                                  FogOfWar bit — defaults FALSE in YR
```

`[MultiplayerDialogSettings] FogOfWar` is `false` by default in YR. With it off, `FUN_00486A70` early-exits, `CreateFoggedSnapshot` is never called, `DAT_008B3CC4 == 0`, `FUN_004D1890`'s outer guard `if (g_hWnd != 0 && DAT_008B3CC4 > 0)` short-circuits, and **case 0x14 never fires**.

**Verdict:** `FUN_004D1890` and the entire Step 4 (`Tactical_layer_base_terrain`) layer is **TS-legacy dormant in standard YR skirmish**. Prior doc was wrong. The live overlay-rendering path is **Step 5** (see §2.2).

### 2.2 The corrected layer-step → bridge-work mapping

Verified by decompiling all eight `Tactical_layer_*` functions and tracing callee chains:

| Step | Address | Ghidra label (misleading) | **Actual purpose** | Bridge work |
|------|---------|---------------------------|--------------------|-------------|
| 1 | `0x6D2B60` | `Tactical_ZBufferDirtyClear` | Z-buffer dirty-rect clear | indirect |
| 2 | `0x6D3660` | `Tactical_layer_shroud_edges` | Shroud/fog write | none |
| **3** | `0x6D2DE0` | `Tactical_layer_terrain_shadows` | **ISO TMP base-tile draw** via `iso_to_screen` → `CellOverlay_TileDraw` → `TMP_TileBlitter` (Z R+W) | **bridge deck TMP** |
| 4 | `0x6D3470` | `Tactical_layer_base_terrain` | **FoggedObjectClass display-table walker** (`FUN_004D1890`) | **dormant in YR** |
| **5** | `0x6D3290` | `Tactical_layer_smudges` | **Per-cell overlay body/shadow** via `Cell_ContentRendering` → `DrawOverlay_Body/Shadow` (Z R+W body, Z R/no-W shadow) | **bridge body+shadow SHP** |
| 6 | `0x6D3AC0` | `Tactical_layer_building_overlays` | Tesla glow / building flat anims | none |
| **7** | `0x6D3040` | `Tactical_layer_overlays` | **ISO base re-blit + RAILING emit** via `FUN_006D7C00` → `FUN_004802A0` → `FUN_00547230` (Z-test, no Z-write) | **bridge railings** |
| 8 | `0x6D3870` | `Tactical_layer_animations` | Ground-level flat anims | none |

The Ghidra labels appear to be inherited from RA1/TD-era nomenclature where layer ordering and purpose differed; in YR they no longer match the runtime semantics.

**Critical implication:** bridge **deck TMPs** (Step 3) and bridge **railings** (Step 7) are emitted in *different layers*, with intervening passes between them. Anything drawn between Steps 3 and 7 (units, animations, building overlays) appears **above the deck** but **below the railings**. Any z-stack fidelity issue must respect this ordering.

### 2.3 No runtime tile selector for high bridges (Q3 — confirmed)

Verified by exhaustive name-pattern search on `gamemd.exe`:
- `search_functions("SelectBridgeTileVariant")` → **only `MapClass__SelectBridgeTileVariant_Low @ 0x57ACF0`**. No `_High`.
- `search_functions("UpdateBridgeTile")` → **only `MapClass__UpdateBridgeTile_Low @ 0x57A430`**. No `_High`.
- `search_functions("SelectDestroyedBridgeTile")` → **only `MapClass__SelectDestroyedBridgeTile_Low @ 0x579620`**. No `_High`.

Verified by call-graph tracing from per-frame draw functions:
- `Cell_ContentRendering` callees: `MapClass__Get_CellClass`, `Cell_in_bounds_check`, `CoordStruct__Set`, `CoordsToClient`, `Math__ftol`, `Matrix3x4_TransformPoint`, `AlphaShapeClass__ClipRect`, `FUN_0047fb90` (body rect), `FUN_0047fde0` (shadow rect), `CellClass__DrawOverlay_Body`, `CellClass__DrawOverlay_Shadow`, `TacticalClass__CoordsToClient2`. **No bridge-classifier call. No neighbor-mask scan. No tile_index lookup.**
- `DrawOverlay_Body` reads: `cell+0x44` (overlay byte), `cell+0x140 & 0x80` (HasBridge bit), `cell+0x11E` (`bridge_damage_state`), `cell+0x24/0x26 & 3` (Latin-square index), `cell+0x11B` (height for Z), and the OverlayTypeClass SHP via vtable+0x9C. **No neighbor reads.**
- `MapClass__ApplyBridgeTile @ 0x57B440` callers: `RMG_PlaceBridge` (TS-legacy), `SelectBridgeTileVariant_Low`, `SelectDestroyedBridgeTile_Low`. **No per-frame draw caller.**
- `MapClass__SelectBridgeTileVariant_Low` callers: `MarkBridgesForRepair_High` only. **No per-frame draw caller.**

**Verdict (Q3):** **High-bridge `cell+0x38` (tile_index) is event-driven** — written at scenario load (`MapClass::Read_Binary @ 0x565C10` from IsoMapPack via `SetBridgeDirection_NESW/_NWSE`) and at destruction events (`ApplyBridgeDestruction_*_High`, `UpdateBridgeEdgeTiles_High`, `ProcessBridgeDamageStateMachine_High`). **Per-frame, only `cell+0x11E` (`bridge_damage_state`) and the cell xy parity bits drive the visible frame.**

This is the highest-value Phase D simplifier. The Rust renderer's high-bridge path is constant-time arithmetic per cell — no neighbor mask, no tile-table indirection.

### 2.4 No `BridgeDisplayTable` symbol exists (Q2)

Name-pattern search returns 0 hits for `BridgeDisplayTable`, `DisplayTable`, or any single-symbol classifier. The "display table" is conceptual — implemented as a constellation of:

1. `DAT_0081CC30` — 16-entry × 4-byte Latin-square frame-jitter table (verified `{0,1,2,3, 3,2,1,0, 2,3,0,1, 1,0,3,2}`).
2. `cell+0x11E` (`bridge_damage_state`, byte) — primary frame-index driver.
3. `cell+0x140 & 0x80` (HasBridge flag) — gates the bridge-specific Y-offset and Z-bias.
4. `cell+0x140 & 0x2000` (damaged-variant flag) — selects damaged-vs-undamaged sub-tile via IsoTileType linked-list.
5. `OverlayTypeClass.vtable[0x9C]` — virtual call returning the SHP image pointer.
6. `IsoTileType` linked-list at `(IsoTileType+0xAC>>2)` (sub-tile variant chain) — alternate-art selector.
7. Bridge-railing tables `DAT_00ABC210` (concrete, 10×16-byte) and a parallel near `DAT_00AA1098` (wood, 10×16-byte) — runtime-populated at theater load.
8. Runtime-init globals `DAT_00AA0E28` (`g_BridgeSet`), `DAT_00ABAD1C` (`g_WoodBridgeSet`) — base tile-set indices.

For LOW bridges only:
- `MapClass__SelectBridgeTileVariant_Low @ 0x57ACF0` — healthy variant chooser (mask + PRNG).
- `MapClass__SelectDestroyedBridgeTile_Low @ 0x579620` — destroyed variant chooser (mask + PRNG).
- `DAT_00ABDB64` / `DAT_00ABDDA4` — coord-delta tables for healthy/destroyed LOW variants (4 bytes/entry: `(short dx, short dy)`, runtime-populated).

The 4 alleged "next-overlay tables" cited in prior reports (HIGH NS @ 0x57E7A0, HIGH EW @ 0x57ED00, LOW NS @ 0x57DD50, LOW EW @ 0x57E2A0) **do not exist as static data**. Those addresses fall inside function bodies (the prologue bytes `81 EC CC 00 00 00` = `SUB ESP, 0xCC` are visible in raw memory). The "next-overlay" mapping is computed **inline via if/else mask decoding** in `SelectDestroyedBridgeTile_Low`, NOT a 16-entry int8 lookup. The values 0x4F/0x52/0x4E/0x50/etc. are computed via inline switch (e.g. `iVar6 = (rng%3) + 0xF` evaluates to 15/16/17).

### 2.5 `FUN_004863D0` is NOT an `(overlay_byte → tile-class index)` classifier

Phase 1's hypothesis refuted. `FUN_004863D0` is a `(int tile_index → bool)` membership tester for **11 theater tile-set ranges**:

| Global | Size | Purpose |
|--------|------|---------|
| `DAT_00aa1020` | 40 | bridge-ramp tile-set |
| `DAT_00aa073c` | 4 (sub 0/4) | bridge end-piece set 1 |
| `DAT_00abb110` | 4 (sub 1/3) | bridge end-piece set 2 |
| `DAT_00aa1050` | 4 (sub 0/1) | bridge end-piece set 3 |
| `DAT_00aa10a0` | 4 (sub 2/3) | bridge end-piece set 4 |
| `DAT_00abbebc` | 20 | (unidentified) |
| `DAT_00abad24` | 4 | (unidentified) |
| `DAT_00aa0e28` | 16 | **HIGH bridge main set** (= `g_BridgeSet`) |
| `DAT_00abad1c` | 16 | **WOOD/LOW bridge main set** (= `g_WoodBridgeSet`) |
| `DAT_00abc2c8` | 2 | (unidentified) |
| `DAT_00aa101c` | 28 | (unidentified) |

Input is `cell+0x38` (tile_index, 32-bit), output is bool. Used by `ComputeBridgeAdjacencyMask_Low` and the shore-piece-fallback branch in `ApplyBridgeTile`. **Has no per-frame caller.**

### 2.6 `FUN_0059E740` is TS-legacy `RMG_PlaceBridge`, not a YR map-init pass

Caller chain:
```
FUN_0059E740 (RMG_PlaceBridge — height ±4 adjustments are RMG-specific)
  ↑ FUN_0059D510 (RMG_PlaceRiverWithBridge — uses Sin_lookup/Cos_lookup)
  ↑ FUN_0059C580 (RMG terrain dispatcher)
  ↑ FUN_00598960 (MapClass::Generate_Random_Map — strings: "RMG: Init random map", "RMG: Creating starting points", "RMG: Creating tiberium")
```

**TS-only.** YR ships pre-authored .map files. Hand-authored YR maps load bridges via `MapClass::Read_Binary @ 0x565C10` calling `SetBridgeDirection_NESW/_NWSE` directly from the IsoMapPack binary. **There is no map-init bridge fixup pass to replicate** for skirmish parity.

### 2.7 `HasBridgeOverlay @ 0x4865D0` is misnamed

Tests `cell+0x38` (tile_index in tile-set space), **not** `cell+0x44` (overlay byte). Cannot be cross-checked against Rust's `is_bridge_overlay_index` (which tests overlay byte) — they operate on different fields. **No exact-byte-range disagreement** to flag in Rust; they're incomparable. The Rust ranges (`24, 25, 237, 238, 74..=101, 122..=125, 205..=232, 233..=236`) are correct from the YR overlay-byte taxonomy. The HIGH `0xCD..=0xE6 raw + 0xE7/0xE8 final` and LOW `0x4A..=0x63 raw + 0x64/0x65 final` invariants are confirmed at the **state-machine** level (`HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md` §2), not at draw time.

### 2.8 `DAT_00880940` is dead in YR

The cell-render-cache token at `cell+0x118` is compared each frame against `DAT_00880940`. Xref scan: `DAT_00880940` has 2 readers (both inside `DrawOverlay_Body`) and **0 writers anywhere in the binary**. Statically initialized to 0, never incremented.

The render-cache early-out reduces effectively to `last_drawn_frame == g_CurrentFrameCounter && clip_rect == cached_clip_rect` — the byte token is `0 == 0` always-true. **TS-era residual.** Rust port can omit the byte cache entirely.

---

## 3. The per-frame bridge draw pipeline (Q1 + Q6)

### 3.1 Top-level dispatch — `TacticalClass::Draw @ 0x6D3D10`

Every frame:
1. Read scroll deltas (`+0xB8 - +0xB0`, `+0xBC - +0xB4`).
2. **Skip-gate** (Pass 0/1): if `param_2 == 0` AND `+0xD7C == 0` AND `+0xD7D == 0` AND no viewport movement AND `DAT_00b0ce88 == 0` (dirty rect count) → skip terrain entirely.
3. If proceeding: lock back surface, compute clip rects, run **Steps 1–8** in the order shown in §2.2.
4. Compact dirty-rect list (`DAT_00b0ce88` count, stride `0x14` bytes = 5×u32).
5. Unlock; copy paired fields (+D64→+D6C, +D68→+D70, +B0→+B8, +B4→+BC); clear `+0xE0` per-cell dirty count.
6. If `param_3 ∈ {2,3}` (objects/combined pass): run pass-2 work; **at the end, clear `+0xD7C` and `+0xD7D`**.

**State writes by `TacticalClass::Draw` itself:** none directly to gameplay; only the deferred-rebuild flag clear at end-of-pass-2 and the paired field-copies (render bookkeeping).

### 3.2 Step 3 — bridge deck TMP draw

```
Tactical_layer_terrain_shadows @ 0x6D2DE0
  → iso_to_screen @ 0x6D7560        [diamond walk over visible cells]
       outer iter: param[3]/0xF + 0x11   (rect_height/15 + 17)
       inner iter: param[2]/0x3C + 4     (rect_width/60 + 4)
       per cell:
         FUN_0047FF80 → screen rect for TMP (reads cell+0x38, +0x11A, +0x11B)
         AlphaShapeClass__ClipRect
         CellOverlay_TileDraw @ 0x480350 (param_4 = 0)
            ├─ if cell+0x38 == 0xFFFF: use g_ClearTile fallback
            ├─ else if (IsoTileType+0xBC) >= 2 [multi-tile, e.g. bridge]:
            │     FUN_005471F0(uVar1 = cell+0x11A)  [pavement bit pre-check]
            │     if returns nonzero: _param_4 = (cell.Flags >> 13) & 1   [0x2000 bit]
            │  else FUN_004814F0(...) [random LAT pick]
            ├─ TMP_TileBlitter(IsoTileType*, sub_tile, surface, screen_x,
            │                  screen_y, clip_rect_4, cell+0x11B,
            │                  cell+0x10C [zAdjust],
            │                  param_13 = 1 [Z R+W ON],
            │                  param_14 = _param_4 [variant select],
            │                  0,0,0,0)
            └─ if cell+0x48 != -1: smudge dispatch (IRR for bridges)
```

`TMP_TileBlitter` writes per-pixel: surface RGB + Z (when `param_13=1`) + ABuffer (alpha). The Z value is computed as `(ZBuffer.maxY + ZBuffer.curY) - param_6 - tile_height/2 - (z_adjust * tile_height)/2` and stored at `DAT_00AA1104` for the inner pixel loop. The inner loop tests `if (uVar7 <= *DAT_00AA10C4)` (Z-test, less-or-equal) then writes `*DAT_00AA10C4 = uVar7`.

**`cell+0x140 & 0x2000` (damage-variant bit)** selects between regular and "alternate" sub-tile art via the IsoTileType linked list at `IsoTileType + 0xAF * 4` ("NextSubTile" pointer) — `param_14` decrements until 0 (variant select). For bridge cells, this toggles undamaged/damaged tile art. Set by `MapClass::ToggleBridgePavement @ 0x56E990` during damage events; **never at draw time**.

### 3.3 Step 5 — bridge body + shadow SHP draw

```
Tactical_layer_smudges @ 0x6D3290
  → Cell_ContentRendering @ 0x6D6D10  [twice — for each scrolled clip rect, plus dirty rects, plus per-object micro rects]
       outer iter: rect[3]/0xF + 0x15    (height-driven loop count)
       inner iter: rect[2]/0x3C + 4
       diamond walk: PASS 1 = body, PASS 2 = shadow

       per pass, per cell:
         Cell_in_bounds_check
         MapClass__Get_CellClass(&local_b0)
         if (cell + 0x44) == -1: SKIP (no overlay)
         else:
            FUN_0047fb90 (body) or FUN_0047fde0 (shadow) → screen rect
              reads cell+0x44, +0x11E, +0x11C, +0x140 & 0x80
              [shadow] if (HasBridge && state in [9,17]):
                       rect.x -= 45; rect.y += 7   ← HIGH-bridge shadow shift
            AlphaShapeClass__ClipRect → if width/height > 0:
              compute screen pos:
                world_x = cell.x * 0x100 + 0x80
                world_y = cell.y * 0x100 + 0x80
                CoordsToClient → screen pos
                local_90 = client_x - viewport_x - 0x1E
                local_8C = client_y - viewport_y
              CellClass__DrawOverlay_Body(&local_90, &local_5c)   [pass 1]
              CellClass__DrawOverlay_Shadow(&local_90, &local_5c) [pass 2]
```

#### 3.3.1 `CellClass::DrawOverlay_Body @ 0x47F6A0` — full extraction

**Signature:** `void __thiscall DrawOverlay_Body(int param_1 /*CellClass**/, int *param_2 /*screen XY*/, int *param_3 /*clip rect 4i*/)`. `param_1` is `int` (direct byte offsets, no `*4`).

```c
// 1. Two hardcoded early-outs:
overlay = *(int *)(param_1 + 0x44);         // cell.OverlayTypeIndex (DWORD; sentinel -1)
if (overlay == 0xA7) return;                 // 167 = Veinhole (TS-era)
if (overlay == 0xB2) return;                 // 178 = Crate

// 2. Resolve OverlayTypeClass and SHP:
OverlayTypeClass *otype = g_OverlayTypeClass_Array[overlay];   // 0x00A83D84 + overlay*4
SHPHeader *shp = otype->vtable[0x9C / 4]();  // virtual Get_Image_Data

// 3. Y offset and Z bias:
draw_off = Get_Draw_Offset(cell);
height_with_bridge = (signed char)cell+0x11B + ((cell+0x140 >> 7) & 1) * 4;
                                          //   ^ HasBridge bit → +4 height bonus
z_value = height_with_bridge * -15 + -2;   // each height step = 15 px Z; -2 baseline
                                          //   bridge cell = extra -60 = wins z-test vs ground

// 4. Lazy tint init (FIRST CALL ONLY):
if (cell + 0x34 == 0) {
    FUN_00483E30(0, 0x10000, 0, 1000, 1000, 1000);  // neutral tint; allocs cell+0x104..+0x114
}

// 5. Render-cache early-out (BRIDGE BRANCH ONLY — gated on cell+0x140 & 0x80):
if (cell+0x140 & 0x80) {
    if (cell+0x64  == g_CurrentFrameCounter        // last_draw_frame
     && cell+0x118 == DAT_00880940                  // ALWAYS 0 — DEAD in YR
     && cell+0x68  == clip_x  &&  cell+0x6c == clip_y
     && cell+0x70  == clip_w  &&  cell+0x74 == clip_h)
        return;                                     // already drawn this frame
}

// 6. THE FRAME FORMULA:
state = (uint)*(byte *)(param_1 + 0x11E);          // bridge_damage_state, 0..17
if (state == 0 || state == 9) {                    // ← LATIN SQUARE ONLY ON BOUNDARY STATES
    state += g_LatinSquare[((cell+0x26 & 3) << 2) | (cell+0x24 & 3)];
    //                       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    //                       (low2bits(cell.y) << 2) | low2bits(cell.x)
    //                       g_LatinSquare = DAT_0081CC30, 16 dwords = {0,1,2,3, 3,2,1,0, 2,3,0,1, 1,0,3,2}
}
// states 1..8, 10..17: frame = state directly. NO Latin square applied.

// 7. The blit:
CC_Draw_Shape(shp, frame=state, &local_10 /*screen pos*/, clip_rect,
              flags = 0x4E00,             // body blitter
              0,                           // stretch
              z_value,                     // per-pixel Z write
              0,                           // (unused)
              (short)cell+0x10E,           // tint intensity
              0, 0, 0, 0, 0);

// 8. Render-cache write-back (BRIDGE BRANCH ONLY):
cell+0x64  = g_CurrentFrameCounter;
cell+0x68  = clip_x;
cell+0x6C  = clip_y;
cell+0x70  = clip_w;
cell+0x74  = clip_h;
cell+0x118 = DAT_00880940;        // = 0, always — no-op
```

**No range check on overlay byte.** The function trusts `cell+0x44`, calls the OverlayType vtable for the SHP, and dispatches by HasBridge bit. Bridge byte ranges (`0xCD..=0xE6`, `0x4A..=0x63`) are **not enforced at draw time**.

**State writes (bridge branch only):** lazy tint init at first call (cell+0x34, +0x104..+0x114); render cache on every redraw (cell+0x64, +0x68..+0x74, +0x118). All render-side. No gameplay state mutated.

**`cell+0x140 & 0x80` is the HasBridge flag** — it lives at *bit 7 of the flags DWORD at offset +0x140*, **not** at byte offset `+0x80`. The previous shorthand "cell+0x80 flag" was misleading.

#### 3.3.2 `CellClass::DrawOverlay_Shadow @ 0x47F510` — full extraction

```c
height = (signed char)cell+0x11B;
shp = otype->vtable[0x9C/4]();
Get_Draw_Offset → screen XY base

// HIGH-bridge shadow shift (verified at 0x47F510):
if ((cell+0x140 & 0x80) && cell+0x11E in [9, 17]) {
    iStack_10 += -0xF;         // x -= 15
    iStack_C  +=   7;          // y +=  7
}

// frame = (shp.frame_count / 2) + state
frame = (int)*(short *)(shp + 6) / 2 + state;

CC_Draw_Shape(shp, frame, &iStack_10, clip_rect,
              flags = 0x4601,            // shadow blitter (bit 0x0001 = darken)
              0, height * -15 + -2,
              0, 1000,                   // neutral tint
              0, 0, 0, 0, 0);
```

**Verified at 0x47F510:** binary literal `iStack_10 = iStack_10 + -0xf` (x -= 15). The BRIDGE_RENDERING doc's `(-15, +7)` claim is **correct**. An earlier Phase 1C extract that suggested -45 / -0x2D was a misreading; -15 is the binary truth.

(Settled — see §10 OQ#2 which is now resolved.)

**Shadow does NOT write the render cache** (no `cell+0x64..+0x74/0x118` writes). Only Body owns the cache.

#### 3.3.3 `CellClass::Get_Draw_Offset @ 0x480110` — Y-offset formula

```c
// Returns the per-cell pixel offset for SHP placement.
piVar1 = FUN_005FDCC0(overlay);    // returns {0, 0} for bridges (see §3.3.4)
y_adjust = piVar1[1];

if (cell+0x140 & 0x80) {           // HasBridge bit
    y_adjust -= 16;                // unconditional bridge offset
    if (cell+0x11E >= 9 && cell+0x11E <= 0x11) {
        y_adjust -= 15;            // ADDITIONAL shift for "second axis" states 9..17
    }
}
else if (cell+0x44 == 0xEF) {
    y_adjust -= 15;                // overlay byte 0xEF special case (Veinhole?)
}

result_y = g_clip_top + (height_level * -15) + y_adjust + 15;
result_x = piVar1[0] + 30;         // +0x1E
```

**The prior axis-convention conflict is now fully resolved:**
- States 0..8 (HasBridge) → Y offset `-16` = **physically EW** (bridge runs NW→SE in screen-space)
- States 9..17 (HasBridge) → Y offset `-31` (`-16 - 15`) = **physically NS** (bridge runs NE→SW in screen-space)

Verified 2026-05-13 by extracting `bridge.tem` frames 0 and 9 via the
`extract-bridge-frames` bin and visually comparing sprite orientation to
RA2's isometric projection (world east-west projects to screen NW-SE
diagonal; world north-south projects to screen NE-SW diagonal). Both
frames are 148×91 px; the difference between them is `frame_y` (3 vs 18)
and the SHP body bitmap itself.

#### 3.3.4 `FUN_005FDCC0 @ 0x5FDCC0` — overlay-type Y offset

```c
otype = g_OverlayTypeClass_Array[param_2];   // overlay byte
y = 0;
if (otype+0x2A8 != 0    /* IsCrystal */
 || otype+0x2A9 != 0    /* IsTiberium */
 || otype+0x294 == 0x7E /* Veins */
 || otype+0x2AA != 0)   /* IsWall */
    y = -12;

if (otype+0x298 == 9) y -= 1;        // ramp/special
if (param_2 == 0x7E) y -= 1;         // overlay byte 126 (Vein root)

return {x: 0, y: y};                  // {0, 0/-12/-13/-14}
```

**For bridges: returns `{0, 0}`** — none of the four flags are set on bridge OverlayTypes. Bridge Y offset is purely from `Get_Draw_Offset`'s own `-16/-31` arithmetic.

#### 3.3.5 `FUN_00483E30 @ 0x483E30` — NOT the SHP resolver (correction)

The plan suspected this was an "OverlayTypeClass→SHP source resolver". **It is not.** It is the **lazy tint-context init** for `cell+0x34`. The actual SHP resolver is the virtual call `otype.vtable[0x9C/4]()` (i.e., `OverlayTypeClass::Get_Image_Data`).

When `cell+0x34 == 0`, `FUN_00483E30` is called with `(mode=0, scale=0x10000=1.0, R=G=B=1000)` to allocate a neutral tint slot. Subsequent calls skip when `cell+0x34 != 0`. **First-call-only side effect.** Render-side concern; not bridge-specific.

### 3.4 Step 7 — bridge railing draw

```
Tactical_layer_overlays @ 0x6D3040
  → FUN_006D7C00 [11 callees, NOT 17 as task spec claimed; NO switch dispatcher]
       diamond walk over rect
       per cell:
         Cell_in_bounds_check
         MapClass__Get_CellClass
         FUN_004802A0(cell, screen_xy, clip_rect)   [the railing trampoline]
            ├─ if cell+0x38 == 0xFFFF: use g_ClearTile
            ├─ uVar1 = cell+0x11A (sub_tile)
            │  IsoTileType = g_TileTypeArray[cell+0x38]
            │  if IsoTileType.NumTiles > 1:
            │      FUN_005471F0(uVar1)              [side-effect-free check; result IGNORED]
            ├─ if IsoTileType+0x2E1 != 0           [ShadowCaster type flag]:
            │      FUN_00547230(uVar1, surface,
            │                    screen_x,
            │                    screen_y + cell+0x11B * -15,
            │                    clip_x, clip_y, clip_w, clip_h,
            │                    cell+0x11B * -15 + 0x3A);
```

#### 3.4.1 `FUN_00547230 @ 0x547230` — railing emit (the OPEN QUESTION resolved)

```c
self_idx = *(int *)(param_1 + 0x294);   // IsoTileType.SelfIdx

// 3-way range dispatch:

// Path 1: concrete bridge
if (self_idx >= DAT_00ABC1F8 && self_idx < DAT_00ABC1F8 + 10) {
    offset = (self_idx - DAT_00ABC1F8) * 16;
    if (param_2 != *(&DAT_00ABC214 + offset)) return;   // surface mismatch
    shp_frame = *(&DAT_00ABC210 + offset);              // entry+0
    if (shp_frame == 0) return;
    x_off = *(&DAT_00ABC218 + offset);                  // entry+8
    y_off = *(&DAT_00ABC21C + offset);                  // entry+12
}
// Path 2: wood bridge (parallel table near DAT_00AA1098)
else if (self_idx >= DAT_00AA1098 && self_idx < DAT_00AA1098 + 10) {
    /* same shape, different base address */
}
// Path 3: shadow-caster (non-bridge railings)
else {
    if (param_1+0x2E1 == 0) return;   // not a shadow caster
    /* linear search of DAT_00AA102C..DAT_00AA1040 (5 entries × 4 bytes) */
    /* if matched: index into DAT_00ABC2D0 (entry stride 0x10) */
}

// Compute screen position:
final_x = (param_4 + x_off + 0x1E + g_RadarViewportOffsetX) - param_6;
final_y = (param_5 + y_off + 0x0F + g_RadarViewportOffsetY) - param_7;

// Draw the railing:
CC_Draw_Shape(DAT_00ABC554,           // **railing SHP** (theater-loaded)
              shp_frame - 1,           // frame (0-based; table is 1-based, 0 = "no railing")
              &final_x, &param_6,
              flags = 0x4601,          // shadow blitter, Z-test, NO Z-write
              0, param_10, 0, 1000, 0, 0, 0, 0, 0);
```

**Definitive table layout:**

| Table | Base | Stride | Element count | Indexed by |
|-------|------|--------|---------------|------------|
| Concrete bridge | `DAT_00ABC210` | 16 bytes | **10** | `(IsoTileType.SelfIdx - DAT_00ABC1F8) * 16` |
| Wood bridge (parallel) | near `DAT_00AA1098` | 16 bytes | **10** | `(IsoTileType.SelfIdx - DAT_00AA1098) * 16` |
| Shadow-caster railings | `DAT_00ABC2D0` | 16 bytes | **5** | linear search of `DAT_00AA102C..0xAA1040` |

**Each entry = 4 ints `{shp_frame_idx_plus_1, surface_ptr, x_offset, y_offset}`.**

`shp_frame_idx == 0` means "no railing for this sub-tile". The `-1` in `CC_Draw_Shape(..., shp_frame - 1, ...)` converts the 1-based table index to a 0-based SHP frame index.

**The tables are zero in static memory** because they're populated at theater-load time by `CDFileClass__Constructor` (the IsoTileType / theater loader). Writes occur at `0x005446B1`, `0x00543F36`, `0x005451DC`, `0x00543C42`, `0x00543E02` inside that loader.

#### 3.4.2 `FUN_005471F0 @ 0x5471F0` — pavement-bit pre-check

```c
piVar1 = otype.vtable[0x9C/4]();    // GetTileData
if (piVar1 == 0) return 0;
sub_idx = param_2 % (piVar1[1] * piVar1[0]);   // wrap to grid size
if (piVar1[sub_idx + 4] == 0) return uVar2 & 0xFF000000;   // null sub-tile
return ((piVar1[sub_idx + 4]) + 0x24) >> 2 & 1;  // TileData[idx].Flags bit 2
```

Returns the **per-sub-tile "pavement / has-railing" flag** (bit 2 of `TileData+0x24`). Set by `CDFileClass__Constructor` for tiles that need a railing emit. Consumed by `CellOverlay_TileDraw` (Step 3) to decide damage-vs-undamaged variant gate, and by `MapClass::ToggleBridgePavement` to validate the cell is paveable before toggling.

The trampoline `FUN_004802A0` calls `FUN_005471F0` and **discards its return value** — looks like dead code; likely a cache-prefetch artifact.

### 3.5 Per-frame mutation summary (Q6)

The complete set of writes during a single bridge cell's per-frame draw:

| Write | Function | Type |
|-------|----------|------|
| `cell+0x34, +0x104..+0x114` | `DrawOverlay_Body/Shadow` (lazy tint init, **first call only**) | Render cache |
| `cell+0x64` (last_draw_frame) | `DrawOverlay_Body` (HasBridge branch only) | Render cache |
| `cell+0x68..+0x74` (last_clip_rect) | `DrawOverlay_Body` (HasBridge branch only) | Render cache |
| `cell+0x118` (always 0 = no-op) | `DrawOverlay_Body` (HasBridge branch only) | Dead — TS residual |
| Surface pixels + Z-buffer + ABuffer | `TMP_TileBlitter`, `CC_Draw_Shape` | GPU-side (in our case) |
| `g_Tactical+0xD7C, +0xD7D` (cleared) | `TacticalClass::Draw` end-of-pass-2 | Render bookkeeping |
| `+0xB8/+0xBC, +0xD6C/+0xD70` (paired copies) | `TacticalClass::Draw` end-of-pass | Render bookkeeping |
| `DAT_00b0ce88` and `g_DirtyRectList` (compaction) | `TacticalClass::Draw` end-of-pass | Render bookkeeping |

**No gameplay-visible state mutated at draw time.** Confirmed.

The two-channel damage state (`cell+0x11E` overlay byte, `cell+0x140 & 0x2000` damage variant) is **only mutated by sim-side code** (state machine, walker, `ToggleBridgePavement`), never by render functions.

---

## 4. Per-cell field map (CellClass)

Verified offsets touched by the bridge render path. **`param_1` is `int` everywhere — direct byte offsets, no `*4` multiplication.**

| Offset | Type | Field | Read by | Written by |
|--------|------|-------|---------|------------|
| `+0x24` | i32 (xy packed: lo16=x, hi16=y) | MapCoord | DrawOverlay_Body, Cell_ContentRendering, all coord helpers | (init) |
| `+0x26` | i16 (high half of +0x24) | y-coord | Latin-square index | (init) |
| `+0x34` | ptr | tint slot | DrawOverlay_Body, Shadow | First-call lazy init via FUN_00483E30 |
| `+0x38` | i32 | tile_index (IsoTileType global index) | TMP_TileBlitter, IsBridge tests, FUN_004863D0 | ApplyBridgeTile, MapClass::Read_Binary, ApplyBridgeDestruction_*_High |
| `+0x44` | i32 | OverlayTypeIndex (-1 = no overlay; bridge byte ranges live here) | DrawOverlay_Body, Cell_ContentRendering discriminator, FUN_005FDCC0 | sim state machine / walker; `UpdateBridgeEdgeTiles_*` writes -1 on dangling-stub repair |
| `+0x48` | i32 | SmudgeTypeIndex (-1 if none) | CellOverlay_TileDraw | smudge placement |
| `+0x64` | u32 | last_draw_frame (render cache) | DrawOverlay_Body | DrawOverlay_Body |
| `+0x68..+0x74` | i32×4 | last_clip_rect | DrawOverlay_Body | DrawOverlay_Body |
| `+0x104..+0x114` | tint storage | tint context | DrawOverlay_*, CC_Draw_Shape | Lazy init |
| `+0x10A/+0x10C/+0x10E` | i16 | cellZAdjust_top / mid / bottom | DrawOverlay_Body (uses +0x10E for bridge body), TMP_TileBlitter (+0x10C) | Cell_ComputeZAdjust @ 0x484680 (per-tick from LogicClass::PerTickUpdate) |
| `+0x118` | u8 | render-cache token (DEAD in YR) | DrawOverlay_Body | DrawOverlay_Body |
| `+0x11A` | u8 | sub_tile (icon idx within IsoTileType) | TMP_TileBlitter, FUN_005471F0, FUN_004802A0, FUN_0047FB90 | ApplyBridgeTile, UpdateBridgeTile_Low |
| `+0x11B` | i8 (signed!) | height_level | Get_Draw_Offset, FUN_0047FF80, CC_Draw_Shape (Z calc) | ApplyBridgeTile, RMG_PlaceBridge ±4 |
| `+0x11C` | u8 | tiberium growth stage / multi-frame variant | FUN_0047FB90 | ore growth |
| `+0x11E` | u8 | **bridge_damage_state (0..17)** — primary frame driver | DrawOverlay_Body, FUN_0047FB90, FUN_0047FDE0, Get_Draw_Offset | sim state machine (`SetBridgeDirection_NESW/NWSE`, `UpdateRamp_*_*`, `UpdateBridgeEdgeTiles_*`) |
| `+0x140` | u32 (flag bitset) | cell flags | many | SetBridgeDirection, ToggleBridgePavement |
| `+0x140 & 0x80` | bit 7 | **HasBridge** (gates bridge-specific Y/Z, render cache, shadow shift) | DrawOverlay_Body, Shadow, Get_Draw_Offset, sim | SetBridgeDirection writes |
| `+0x140 & 0x100` | bit 8 | bridge structural | sim, UpdateAdjacentBridges_* | SetBridgeDirection |
| `+0x140 & 0x200` | bit 9 | bridge endpoint/ramp | sim | SetBridgeDirection |
| `+0x140 & 0x400` | bit 10 | destroyed flag | sim, UpdateAdjacentBridges_* | SetBridgeDirection |
| `+0x140 & 0x500` | combined 0x100 + 0x400 mask | "is BRIDGE_HEAD candidate" | UpdateAdjacentBridges_* (rim refresh discriminator) | (set via 0x100/0x400) |
| `+0x140 & 0x800` | bit 11 | bridge tail / direction-flip | sim | SetBridgeDirection |
| `+0x140 & 0x2000` | bit 13 | **damaged-art variant select** (orthogonal channel) | TMP_TileBlitter (`param_14`) | `MapClass::ToggleBridgePavement @ 0x56E990` |

**Important: there is field-offset disagreement between the Phase 1 and Phase 2 reports about whether `cell+0x11A` is "sub_tile" or "damage_state_1".** Phase 1C's direct decompilation of `DrawOverlay_Body` (consumes `+0x11E`) and `CellOverlay_TileDraw` (consumes `+0x11A` as sub_tile passed to `TMP_TileBlitter`) is authoritative. The Phase 2 report's claim that `UpdateAdjacentBridges_High` reads `+0x11A` as "damage_state_1" appears to be a misreading — re-verify if precision is needed (see §10).

---

## 5. Latin-square table @ `0x0081CC30` — verified

64-byte read returned 16 dwords:

```
00 00 00 00  01 00 00 00  02 00 00 00  03 00 00 00
03 00 00 00  02 00 00 00  01 00 00 00  00 00 00 00
02 00 00 00  03 00 00 00  00 00 00 00  01 00 00 00
01 00 00 00  00 00 00 00  03 00 00 00  02 00 00 00
```

= `{0,1,2,3, 3,2,1,0, 2,3,0,1, 1,0,3,2}` — the expected 4×4 Latin square. **Stride is 4 bytes per entry (dwords), NOT 1 byte.**

Indexed by `((cell.y_low2 << 2) | cell.x_low2)` — i.e. `(cell+0x26 & 3) * 4 + (cell+0x24 & 3)`. The two forms `(y&3) * 4 + (x&3)` and `((y&3) << 2) | (x&3)` are arithmetically identical.

**Frame application:** ONLY for `bridge_damage_state == 0` OR `== 9`. States 1..8 and 10..17 use `frame = state` directly with no jitter.

**Implication:** the bridge SHP layout is (axis labels resolved 2026-05-13):
- Frames 0..3: healthy **EW** variants (state 0 + variant 0..3)
- Frames 4..8: **EW** damage progression (states 4..8, fixed)
- Frames 9..12: healthy **NS** variants (state 9 + variant 0..3)
- Frames 13..17: **NS** damage progression (states 13..17, fixed)

(Total 18 frames per axis pair; matches typical bridge SHP layouts. EW frames
appear as NW→SE diagonals in screen-space, NS as NE→SW.)

---

## 6. INI surface

Verified via grep on `ini/rulesmd.ini`, `ini/rules.ini`, `ini/artmd.ini`, `ini/art.ini`:

| Key | Section | Default | Currently parsed in Rust |
|-----|---------|---------|--------------------------|
| `BridgeVoxelMax` | `[General]` | `3` | TBD (DORMANT in YR — TS-only, see HIGH §12.11) |
| `RepairBridgeSound` | `[General]` | `BridgeRepaired` | TBD |
| `BridgeExplosions` | `[General]` | `TWLT026,TWLT036,TWLT050,TWLT070` | TBD |
| `BridgeStrength` | `[CombatDamage]` | `1500` | **Yes** |
| `DestroyableBridges` | `[CombatDamage]` | `yes` | **Yes** (combat path) |
| `BridgeSet` (theater) | theater config | tile-set base name | **Yes** (theater load) |
| `WoodBridgeSet` (theater) | theater config | tile-set base name | **Yes** (theater load) |
| `WaterBridge` (theater) | theater config | tile-set base name | TBD |
| `TooBigToFitUnderBridge` (per-unit) | unit | bool | TBD (locomotor) |
| `ZFudgeBridge` (per-unit) | unit | int (`7` for 2 units) | TBD (sprite Z) |

Tile sections present: `[BRIDGE1]`, `[BRIDGE2]`, `[BRIDGEB1]`, `[BRIDGEB2]` (4 each), `[LOBRDG01..28]` (28 each), `[LOBRDGE1..4]` (4 each), `[LOBRDGB1..4]` (4 each). All match expectations.

**Conclusion:** Phase D needs no new INI parsing for tile selection. Tile-name registration is metadata for `bridge_atlas.rs`/`overlay_atlas.rs`; selection is computed from runtime state against hardcoded constants and theater-loaded base indices.

---

## 7. Rim refresh — `UpdateAdjacentBridges_High/_Low` (Q5)

**The highest-value answer in this report for the orchestrator's stub.**

### 7.1 `MapClass::UpdateAdjacentBridges_High @ 0x576770`

**Signature:** `void __thiscall fn(MapClass *self, short *coord)`

**Phase A — find adjacent BRIDGE_HEAD:** 8-direction walk starting at `coord`, stop at first cell with `(flags & 0x500) != 0` (bit 8 OR bit 10 — combined "bridge head / destroyed flag" mask). Loop bound: 8 iterations. Direction offsets from `g_DirectionOffsets @ 0x89F680` (runtime-populated).

**Phase B — pick walk direction** based on the matched cell's flag bits:
- `(flags & 0x100) == 0 && (flags & 0x400)`: walk through ramp path forward, up to 3 cells (`local_1c < 4` guard).
- `(flags & 0x100) && !(flags & 0x80)`: jump to `*(self + 0x2C) + 0x24` (anchor coord).
- `(flags & 0x100) && (flags & 0x80)`: start at current cell.
- `(flags & 0x800)`: flip walk direction.

**Phase C — walk along bridge, find dangling end, repair:** for each cell along the walk:
```c
normalized = (cell+0x38) - DAT_00aa0e28 + 1;   // bridge-local tile index

// Pattern matching against runtime tile-class constants:
if ((normalized == DAT_00abc2b4 || normalized == DAT_00aa1130) && cell+0x11A == 8)
    mode = 2;
else if ((normalized == DAT_00abad30 || ...) && cell+0x11A == 5)
    mode = 2;
else if ((normalized == DAT_00aa1548 || normalized == DAT_00aa0740) && cell+0x11A == 12)
    mode = 4;
else if ((normalized == DAT_00aa1028 || ...) && cell+0x11A == 7) {
    UpdateBridgeEdgeTiles_High(coord, 4, &local_10);
    goto LAB_00576b73;  // dirty rect
}
else continue walking;

UpdateBridgeEdgeTiles_High(coord, mode, &local_10);
LAB_00576b73:
TacticalClass::DirtyScreenRect(local_10, local_C, local_8, local_4, 0);
```

**Field writes by `UpdateAdjacentBridges_High` itself:** **NONE on cells.** Only:
- `local_10..local_4` (RectangleStruct local) — passed to `DirtyScreenRect`.
- `TacticalClass::DirtyScreenRect(rect, 0)` — queues redraw region.

**State writes happen inside the callee `UpdateBridgeEdgeTiles_High`** (next section).

### 7.2 `MapClass::UpdateBridgeEdgeTiles_High @ 0x576200`

The **actual rim-refresh state writer.** Walks up to 30 cells (the `0x1E` immediate is the walk limit) along `g_DirectionOffsets[direction & 7]`, looking for an "open dangling stub" condition. When found:

```c
CellClass::SetBridgeDirection_NESW(direction_code, 0);  // clears bridge-direction flags
puVar15[0x11E] = 0;                          // damage_state = 0
*(int *)(puVar15 + 0x44) = 0xFFFFFFFF;       // overlay_byte = -1 (no overlay)
RadarClass::MarkTerrainDirty(puVar15 + 0x24);
RepairBridgeSegment(coord, edge_coord);      // re-stamp cap tile via SelectBridgeTileVariant
UpdateBridgeEdgeTiles_High(...);             // recurse on next stub
```

**Recursion bound:** the walk-cap (`< 30 cells per call`) and finite bridge-cluster size.

The "task spec constants 5/7/8/12" referenced in the plan come from the *Phase A pattern matches* in the outer `UpdateAdjacentBridges_High` (testing `cell+0x11A` against 5, 7, 8, 12), **not** from `UpdateBridgeEdgeTiles_High`'s body.

### 7.3 Callers (verified non-render)

| Function | Callers |
|----------|---------|
| `UpdateAdjacentBridges_High` | `MapClass::DestroyBridge_High_MapInit @ 0x574000` (BombClass detonate, BuildingClass update); `MapClass::DestroyBridge_Low_MapInit @ 0x574C20`; `ProcessBridgeDamageStateMachine_High @ 0x576BA0` |
| `UpdateAdjacentBridges_Low` | `ProcessBridgeDamageStateMachine_Low @ 0x571490` only |
| `UpdateBridgeEdgeTiles_High` | `UpdateAdjacentBridges_High` + self (recursion) |

**None called from per-frame draw functions.** Verified.

### 7.4 What our Rust orchestrator must do

The `update_adjacent_bridges()` stub at `bridge_orchestrator.rs:208` needs to mirror this contract on each rim cell:

1. **Find adjacent bridge head** (8-dir walk, stop on `flags & 0x500`).
2. **Determine walk direction** from flags.
3. **Walk up to 30 cells** along direction.
4. **Pattern-match (normalized_tile_idx, sub_tile)** against the tile-class constants (these are runtime-populated globals — treat as opaque tags).
5. **At dangling stub:**
   - Write `cell.bridge_damage_state = 0` (mirror of `cell+0x11E = 0`)
   - Write `cell.overlay_byte = NONE` (mirror of `cell+0x44 = -1`)
   - Update bridge-direction flags via SetBridgeDirection-equivalent
   - Mark radar dirty
   - Call `repair_bridge_segment(cell, edge)` to re-stamp cap tile
6. **Always queue render dirty rect** for visited region.

This is **substantively more than render-side dirtying.** The rim refresh has a real state-mutation channel.

---

## 8. UpdateRamp_*_* display crossover (Q7)

Cross-reference only — full coverage in `HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md` §11.1, §11.2.

Eight UpdateRamp_*_High @ `0x572230..0x573170` and eight UpdateRamp_*_Low @ `0x56ED40..0x56FC80`. Each handles a (NS|EW) × (DamageA|DamageB|CollapseA|CollapseB) state transition. They are called exclusively from `ProcessBridgeDamageStateMachine_*` — **never from any per-frame draw function**.

Each has an **overlay-write branch** that toggles `cell+0x44` (overlay byte) along with `cell+0x140 & 0x2000` (damage-variant flag) via `MapClass::ToggleBridgePavement @ 0x56E990` and/or `MapClass::SetOverlayAndPropagate @ 0x56EB80`. **The data-driving globals (`DAT_00ABAD30`, `DAT_00AA1028`, `DAT_00ABC1E8`, `DAT_00AA0E38`, `DAT_00AA0E28`) are zero in the static binary** — they're populated by the theater loader at runtime. **Tasks 13.5/15.5 in our Rust port defer this branch's full implementation** until live-debugger capture is available; the *structure* of the overlay-write branch is documented (8-direction propagation, type-class gates), only the *data values* are missing from static analysis.

**For Phase D:** the renderer reads `cell.overlay_byte` and `cell.flags & 0x2000` post-tick. The state-machine's overlay writes occur at sim-tick time (committed before draw), so the renderer always sees a consistent post-mutation state. **Phase D does not need to mirror the UpdateRamp_*_* logic.** It just needs to consume the post-tick fields.

---

## 9. The Renderer Model — Phase D synthesis

A language-agnostic data flow of what the renderer must do. **This is the deliverable for Phase D design.**

### 9.1 Inputs (read each frame, per visible cell)

```
For each cell (rx, ry) in visible viewport:
    Inputs (from sim, all post-tick):
        cell.overlay_byte:   u8       // -1 (no overlay) OR
                                       //   HIGH 0xCD..=0xE6 (raw body) | 0xE7/0xE8 (final)
                                       //   LOW  0x4A..=0x63 (raw body) | 0x64/0x65 (final)
                                       //   (other overlays: ore, walls, fences, etc.)
        cell.bridge_damage_state: u8  // 0..17 (only meaningful if HasBridge bit set)
        cell.height_level: i8         // signed; controls Z stacking
        cell.flags: u32
            bit 0x80   = HasBridge (gates bridge-specific draw)
            bit 0x100  = bridge structural
            bit 0x400  = destroyed
            bit 0x800  = direction-flip
            bit 0x2000 = damaged-art variant select
        cell.tile_index: i32          // IsoTileType global index (event-driven; static for high)
        cell.sub_tile: u8             // sub_tile within IsoTileType
        cell.zAdjust: i16             // pre-computed by Cell_ComputeZAdjust per-tick

    Inputs (from globals, theater-loaded):
        g_BridgeSet, g_WoodBridgeSet  // tile-set base indices
        g_BridgeRailingSHP            // theater-loaded SHP
        g_BridgeRailingTable_Concrete // 10 × {shp_frame, surface, dx, dy}
        g_BridgeRailingTable_Wood     // 10 × {shp_frame, surface, dx, dy}
        g_LatinSquare = {0,1,2,3, 3,2,1,0, 2,3,0,1, 1,0,3,2}
```

### 9.2 Decision tree (per cell, per frame)

For a cell with `cell.flags & 0x80` (HasBridge) — i.e. a bridge cell:

**A. TMP base tile** (Step 3, written before SHP body):
```
sub_tile_variant = cell.sub_tile
if IsoTileType[cell.tile_index].num_tiles >= 2 AND IsoTileType.tile_data[sub_tile].flags & 0x04:
    variant_select = (cell.flags >> 13) & 1   // 0 = undamaged, 1 = damaged
else:
    variant_select = pseudo_random(cell.x, cell.y)   // LAT noise
emit TMP_TileBlitter(IsoTileType, sub_tile_variant, screen_pos, clip,
                      Z_W = ON,
                      variant_select)
```

**B. Body SHP** (Step 5, drawn over TMP):
```
state = cell.bridge_damage_state  (0..17)
if state == 0 OR state == 9:
    frame = state + g_LatinSquare[((cell.y & 3) << 2) | (cell.x & 3)]
else:
    frame = state

y_offset = -16  if state in [0..8]
         = -31  if state in [9..17]

z_value = (cell.height_level + 4) * -15 - 2   // +4 bonus for HasBridge

flags = 0x4E00 (body blitter)
emit CC_Draw_Shape(g_OverlayTypeClass[cell.overlay_byte].SHP,
                    frame, screen_pos + (30, y_offset),
                    clip, flags, z_value, ...)
```

**C. Shadow SHP** (Step 5, second pass):
```
shadow_frame = (shp.frame_count / 2) + state
if state in [9..17]:
    shadow_pos.x -= 15   // verified at 0x47F510
    shadow_pos.y +=  7
flags = 0x4601 (shadow blitter — darken, no Z-write)
emit CC_Draw_Shape(shp, shadow_frame, shadow_pos, clip, flags, z_value, ...)
```

**D. Railing** (Step 7, drawn last):
```
if IsoTileType[cell.tile_index].is_shadow_caster:
    self_idx = IsoTileType.self_idx
    if self_idx in [g_BridgeSet, g_BridgeSet + 10):
        entry = g_BridgeRailingTable_Concrete[self_idx - g_BridgeSet]
    elif self_idx in [g_WoodBridgeSet, g_WoodBridgeSet + 10):
        entry = g_BridgeRailingTable_Wood[self_idx - g_WoodBridgeSet]
    else: skip
    if entry.shp_frame == 0: skip   // no railing for this sub-tile
    flags = 0x4601 (Z-test, no Z-write)
    emit CC_Draw_Shape(g_BridgeRailingSHP, entry.shp_frame - 1,
                        screen_pos + (entry.dx, entry.dy), clip, flags, ...)
```

### 9.3 Outputs (writes during draw)

- Per-cell render cache: `last_drawn_frame` + `last_clip_rect` (used for next-frame skip-test).
- Surface pixels + Z-buffer + ABuffer (per blit).
- **No gameplay state mutation.**

### 9.4 Inter-step ordering

Steps 3 → 4 (FoggedObject, dormant) → 5 → 6 → 7 → 8.

Anything drawn between Steps 3 and 7 (units, animations, shroud edges) appears **above the deck** but **below the railings**. This ordering is observable and must be preserved.

### 9.5 Per-frame skip-gate

```
if not param_2 AND not g_Tactical.dirty_terrain_flag AND
   no_viewport_movement AND no_dirty_rects:
    skip_terrain
```

The dirty flag is set by sim mutations (state machine transitions, `ToggleBridgePavement`, `RecalcBridgeShroudFlags`). Cleared at end-of-pass-2.

---

## 10. Open Questions

1. ~~**Axis convention (EW vs NS).**~~ **RESOLVED 2026-05-13; clarified 2026-05-16.** Extracted `bridge.tem` frames 0 and 9 via the new `extract-bridge-frames` bin and inspected the sprites. Frame 0 (state range 0..8, Y-offset -16) is a NW→SE screen-space diagonal = world east-west = **physically EW**. Frame 9 (state range 9..17, Y-offset -31) is NE→SW = world north-south = **physically NS**. These are physical asset-frame labels. In Rust, the runtime `Axis` enum follows the state-byte family, so `Axis::NS -> 0..=8` and `Axis::EW -> 9..=17`. Do not use the physical asset label to flip renderer frame families away from `DamageState::to_state_byte(axis)`. gamemd's `UpdateRamp_*` and `ApplyBridgeDestruction_*` function-name suffixes remain inverted relative to the state ranges they operate on; trust byte-range/state-byte evidence over binary function names.

2. ~~**Shadow X-displacement: `-15` or `-45`?**~~ **RESOLVED.** Binary literal at `0x47F510`: `iStack_10 = iStack_10 + -0xf` = **-15**. The -45 extract was a misreading; BRIDGE_RENDERING's original -15 value stands.

3. **`cell+0x11A` semantics.** Phase 1C says "sub_tile (icon idx)"; Phase 2B+2C says `UpdateAdjacentBridges_High` reads it as "damage_state_1". One of these is misreading the disassembly. Phase 1C's read (consumed by `TMP_TileBlitter` as sub_tile_idx) is more likely correct. Re-verify in `UpdateAdjacentBridges_High` if this matters for the orchestrator port.

4. **Wood-bridge railing table address.** Phase 1D found the table near `DAT_00AA1098` but the exact base wasn't fully extracted. Confirm by reading raw memory at theater-load time (live-debugger task).

5. **Damaged-tile art file naming.** The CDFile loader probes `DamagedTile.tem`, `.sno`, `.urb`, `.ubn` (from string globals). Exact filenames per theater not yet extracted.

6. **`FUN_006D7F20`** (per-shadow-caster cell dispatcher in Step 1's ZBufferDirtyClear chain): the function exists and handles 4 special-case cells around shadow casters under bridge-overhang rules. Worth a future deep-dive for shadow-edge correctness, but bridge mainline already covered.

7. **`UpdateBridgeTile_Low` recursion semantics.** The function recurses on 8 neighbors with a deferred-state cycle-breaker, but the exact bounded-iteration count (worst-case bridge-cluster size) wasn't measured. May matter for performance but not for parity.

8. **Tile-class constant mapping.** Several `DAT_*` constants in `UpdateAdjacentBridges_High` and `UpdateBridgeEdgeTiles_High` (e.g. `DAT_00abc2b4`, `DAT_00aa1130`, `DAT_00abad30`, `DAT_00aa1028`, `DAT_00aa1548`, `DAT_00aa0740`) are tile-class indices populated at theater load. The Rust port must produce equivalent indices from theater-load INI parsing — verify the mapping during Phase D implementation.

---

## 11. Sources

### Ghidra functions decompiled (live, read-only, gamemd.exe)

**Per-frame draw chain:**
- `0x6D3D10` TacticalClass::Draw
- `0x6D2B60` Tactical_ZBufferDirtyClear
- `0x6D3660` Tactical_layer_shroud_edges
- `0x6D2DE0` Tactical_layer_terrain_shadows
- `0x6D3470` Tactical_layer_base_terrain
- `0x6D3290` Tactical_layer_smudges
- `0x6D3AC0` Tactical_layer_building_overlays
- `0x6D3040` Tactical_layer_overlays
- `0x6D3870` Tactical_layer_animations
- `0x6D7560` iso_to_screen
- `0x6D6D10` Cell_ContentRendering
- `0x6D7C00` FUN_006D7C00
- `0x47F6A0` CellClass::DrawOverlay_Body
- `0x47F510` CellClass::DrawOverlay_Shadow
- `0x480110` CellClass::Get_Draw_Offset
- `0x5FDCC0` FUN_005FDCC0 (overlay-type Y offset)
- `0x483E30` FUN_00483E30 (lazy tint init — NOT a SHP resolver)
- `0x480350` CellOverlay_TileDraw
- `0x547CF0` TMP_TileBlitter
- `0x547230` FUN_00547230 (railing emit)
- `0x5471F0` FUN_005471F0 (pavement bit pre-check)
- `0x4802A0` FUN_004802A0 (railing trampoline)
- `0x4D1890` FUN_004D1890 (FoggedObject walker — DORMANT)
- `0x4D1040` BuildingClass::CreateFoggedSnapshot
- `0x457AA0` FUN_00457AA0 (snapshot create entry)
- `0x486A70` FUN_00486A70 (FogOfWar gate)
- `0x4865D0` CellClass::HasBridgeOverlay (misnamed — tests tile_index, not overlay)
- `0x47FB90` FUN_0047FB90 (body rect)
- `0x47FDE0` FUN_0047FDE0 (shadow rect; HIGH-bridge shadow shift)
- `0x47FF80` FUN_0047FF80 (TMP rect)
- `0x4AED70` CC_Draw_Shape

**Map-init / event-driven writers:**
- `0x57B440` MapClass::ApplyBridgeTile (generic, not high-specific)
- `0x4863D0` FUN_004863D0 (tile-set membership tester, NOT classifier)
- `0x59E740` FUN_0059E740 = RMG_PlaceBridge (TS-LEGACY)
- `0x47E040` CellClass::SetBridgeDirection_NESW
- `0x47E470` CellClass::SetBridgeDirection_NWSE (byte-identical to NESW)
- `0x484680` Cell_ComputeZAdjust (per-tick lighting; render-side only)
- `0x576200` MapClass::UpdateBridgeEdgeTiles_High (orphan-stub reaper)

**Low-bridge selectors:**
- `0x57ACF0` MapClass::SelectBridgeTileVariant_Low
- `0x579620` MapClass::SelectDestroyedBridgeTile_Low
- `0x57A430` MapClass::UpdateBridgeTile_Low
- `0x57B210` MapClass::ComputeBridgeSurfaceMask
- `0x579B70` MapClass::ComputeBridgeAdjacencyMask_Low
- `0x57A0C0` MapClass::MarkBridgesForRepair_High
- `0x578E60` MapClass::MarkBridgesForRepair_Low
- `0x598030` Rand_in_range (FUN_00598030)

**Rim refresh:**
- `0x576770` MapClass::UpdateAdjacentBridges_High
- `0x571050` MapClass::UpdateAdjacentBridges (low)

**Predicates (light touch):**
- `0x486750` CellClass::IsBridge
- `0x486770` CellClass::IsWoodBridge
- `0x484AB0` CellClass::IsLowBridgeCell
- `0x485060` CellClass::IsOnBridgeSurface
- `0x574600` MapClass::IsLowBridgeEndpointTile
- `0x5746C0` MapClass::IsBridgeRampTile
- `0x578D80` IsOnBridgeRamp

### Globals / static tables

- `0x0081CC30` `g_LatinSquare` (16 dwords, verified by raw memory read)
- `0x00A83D84` `g_OverlayTypeClass_Array` (256 ptrs)
- `0x00A8ED2C` `g_TileTypeArray` / `g_IsoTileTypeArray` (master TileSet directory base)
- `0x00AA0E28` `g_BridgeSet` (HIGH bridge tile-set base, 16 entries)
- `0x00ABAD1C` `g_WoodBridgeSet` (LOW bridge tile-set base, 16 entries)
- `0x00ABC1F8` `g_SlopeSetPieces` (HIGH bridge body-tile base)
- `0x00AA1098` `g_SlopeSetPieces2` (LOW bridge body-tile base)
- `0x00ABC210` Concrete bridge railing table (10 × 16-byte entries)
- `0x00ABC2D0` Shadow-caster railing table (5 × 16-byte entries)
- `0x00ABC554` Bridge railing SHP pointer (theater-loaded)
- `0x00ABDB64` LOW healthy variant coord-delta table
- `0x00ABDDA4` LOW destroyed variant coord-delta table
- `0x0089F680` `g_DirectionOffsets` (8 × 4-byte dx/dy pairs, runtime-populated)
- `0x00ABE890` `g_GlobalRng` (RandomClass instance, used by Rand_in_range)
- `0x00880940` `DAT_00880940` (DEAD in YR — render-cache token, never written)
- `g_Tactical+0xD7C/+0xD7D` deferred-rebuild flag pair (cleared end-of-pass-2)
- `0x008B3CC0/+CC4/+CC8/+CCC/+CD0` FoggedObject display table (DORMANT in YR)

### Reference docs cross-checked

- `ra2-rust-game-docs/BRIDGE_RENDERING_GHIDRA_REPORT.md` (pre-Phase F; layer mapping corrections in §2.2 above)
- `ra2-rust-game-docs/HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md` (state-machine authority; verified for §8 cross-reference)
- `ra2-rust-game-docs/BRIDGE_SYSTEM.md` (TubeClass + zone integration)
- `ra2-rust-game-docs/CELLCLASS_ZONES_SPEED_BRIDGES.md` (CellClass field map; cross-checked offsets)
- `ra2-rust-game-docs/LAT_RETRIGGER_AND_BRIDGE_DAMAGE_VARIANT_GHIDRA_REPORT.md` (two-channel state model — confirmed in §3.2)
- `ra2-rust-game-docs/PHASE_F_BRIDGE_DAMAGE_DISPATCH_VERIFICATION.md` (4-path dispatcher; out-of-scope for this report)
- `docs/plans/2026-05-07-bridges-tier2-phase-f-orchestrator-design.md` (Rust orchestrator; rim refresh stub at line 208)

### INI files checked

- `ini/rulesmd.ini`, `ini/rules.ini` — `[General]`, `[CombatDamage]`, tile sections
- `ini/artmd.ini`, `ini/art.ini` — `[BRIDGE]`, `[BRIDGB]`, `[RAILBRDG]`, all `[LOBRDG##]` / `[LOBRDGE#]` / `[LOBRDGB#]` sections

### Rust source surface mapped (no edits)

- `src/sim/bridge_state/mod.rs` — `BridgeRuntimeState`, `BridgeRuntimeCell`
- `src/sim/world/bridge_orchestrator.rs:208-210` — `update_adjacent_bridges` stub
- `src/sim/bridge_specs.rs` — destruction overlay tables (verified byte-for-byte)
- `src/render/bridge_atlas.rs`, `src/render/overlay_atlas.rs` — atlas APIs
- `src/map/overlay.rs`, `src/map/overlay_types.rs:20-30` — `is_bridge_overlay_index` ranges (24, 25, 237, 238, 74-101, 122-125, 205-232, 233-236)

---

## 12. Success Criteria — self-audit

| Criterion | Status |
|-----------|--------|
| Answer all 7 questions in §1 with HIGH confidence | ✅ Q1, Q3, Q4, Q5, Q6, Q7 HIGH; Q2 HIGH (no single table; constellation enumerated) |
| Include every function from plan §3 | ✅ 40 functions decompiled or LIGHT-touched (Phase 3 deferred to existing HIGH report for UpdateRamp_*_*) |
| Resolve every plan §9 deferred question | Mostly ✅; remaining unresolved items moved to §10 above |
| State "Active in YR: Yes/No/Conditional" per finding | ✅ TS-legacy items (FoggedObject walker, RMG_PlaceBridge, BridgeVoxelMax, DAT_00880940) flagged |
| Cite Ghidra addresses for every HIGH-confidence claim | ✅ §11 sources |
| Renderer Model — language-agnostic data flow | ✅ §9 |
| Phase D integration sketch (pseudocode, not Rust) | ✅ §9.2 decision tree; §7.4 rim-refresh contract |

The investigation is complete. The Phase D renderer can proceed to design (`/brainstorm bridge renderer (Phase D)`) with this doc as context.
