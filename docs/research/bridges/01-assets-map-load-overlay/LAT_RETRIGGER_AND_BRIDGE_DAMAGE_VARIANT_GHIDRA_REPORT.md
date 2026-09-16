---
title: LAT Retrigger & Bridge Damage-Variant — Ghidra Research Report
---

# LAT Retrigger on Cell Mutation & Bridge Damage-Variant Propagation — Ghidra Research Report

**Primary addresses:**
- `CellClass::RecalcAttributes` @ `0x0047D2B0`
- `CellClass::ApplyLAT_and_SlopeFixup` @ `0x0047CA80` (called from RecalcAttributes)
- `CellClass::PostDestructionWallCleanup` @ `0x00480630` (wall destroy → re-LAT 5 cells)
- `CellClass::DestroyOverlay` @ `0x00480CB0` (wall destroy entry)
- `MapClass::SetOverlayAndPropagate` @ `0x0056EB80` (tile-id 8-neighbor flood-fill)
- `MapClass::ToggleBridgePavement` @ `0x0056E990` (CellFlags & 0x2000 flood-fill)
- `IsometricTileTypeClass::HasDamagedVariantAtSubTile` @ `0x005471F0` (TMP flag 0x04 gate)

**Overall confidence:** HIGH. All critical functions decompiled and cross-referenced with prior reports. Caller lists taken from live Ghidra xrefs.

**Active in YR:** Yes — core mutation dispatch used by every gameplay-state cell change.

---

## 1. Overview

This report closes the two deferred follow-ups:

1. **LAT retrigger on wall destroy** — verified. `CellClass::PostDestructionWallCleanup` already calls `RecalcAttributes` on up to 5 cells (self + 4 cardinals). Once the Rust engine exposes a `recalc_cell_attributes(coord)` primitive, the existing wall-destroy path will re-flow ground LAT naturally. The same dispatcher is reused by ~20 other mutation sites (building place/sell, bridge destroy/repair, overlay place/remove, unit multi-cell enter/exit, terrain limbo, ore spread, tile flood-fill).

2. **Bridge damage visual via `CellFlags & 0x2000` flood-fill** — verified. Bridge damage and repair toggle the damage-variant bit across a contiguous same-tile-id region via `ToggleBridgePavement` rather than swapping tile indices. Final destruction (bridge → water) is a separate channel that uses `SetOverlayAndPropagate` to change the tile index outright. These are distinct mechanisms and both need Rust equivalents.

Two distinct flood-fills:

| Operation | Call | Effect | Propagation criterion | When used |
|-----------|------|--------|----------------------|-----------|
| Tile-id swap | `SetOverlayAndPropagate(coord, new_tid, old_tid, …)` | writes `cell.IsoTileTypeIndex`, calls `RecalcAttributes` | neighbor `cell.IsoTileTypeIndex == old_tid` | Final bridge collapse (bridge tiles → water tiles), RMG stamping |
| Damage bit toggle | `ToggleBridgePavement(coord, state, suppress_self)` | writes `cell.Flags & 0x2000`, marks radar dirty only | neighbor `cell.IsoTileTypeIndex == seed_tid` | Bridge damage progression; bridge ramp pavement repair |

---

## 2. `CellClass::RecalcAttributes` — the retrigger primitive

### 2.1 What it does (verified from decompilation at 0x0047D2B0)

Runs on a single cell. Decomposes into three phases:

1. **LandType derivation** — picks LandType from overlay (+0x298 on OverlayTypeClass when `+0x2AC` override is set) or from the IsoTileType (ground path). Clears impossible combos (e.g., ore overlay on a slope → clears overlay).
2. **`ApplyLAT_and_SlopeFixup()`** — always called in both the overlay and non-overlay paths. This is the function that re-picks the tile variant from the 4-cardinal LAT neighbor mask and fixes slope transitions. **This is the LAT retrigger.**
3. **`RecalcZoneType()` + zone-cache write-back** — refreshes movement-zone classification; writes `cell.Level` and `cell.field_0x4c` into two per-cell zone-lookup side tables at `(DAT_0087f850 + idx*4)` and `(DAT_0087f858 + idx*10)`.

Other side effects (fires under specific conditions, not always):
- **Auto-anim spawn** when the underlying IsoTileType has `+0x2C8 != -1` and `+0x2D4 == cell.Height`; guarded by `Flags & 0x20000` ("already spawned"). This is what makes burning-barrel / palm-leaf anims appear at map load.
- **Tube-endpoint construction** — if tile is one of 4 subway-entrance bridgehead ranges and LandType == 10 (Rail), calls `TubeClass::Constructor` with direction from `DAT_0081CC20`.
- **Shroud-like LandType-3 override** — gated by `g_RulesClass_Instance + 0x664`. If all 6 surrounding cells are ≥4 levels below this one, forces `LandType = 3`. (This appears to be the cliff-shadow classifier; tangential here.)

### 2.2 Complete caller list (from Ghidra xref dump)

I pulled 71 call sites in total. Categorized:

**Overlay & ore mutation (6 paths)**
- `CellClass::DestroyOverlay` (wall damage/destroy entry)
- `CellClass::Reduce_Tiberium` (refinery/harvester ore drain)
- `Apply_area_damage` (area-damage dispatch)
- `FUN_00485590`, `FUN_00485af0` — ore/gem spread (operate on OverlayTypeIndex == 0x7E, stage 0..3)
- `AnimClass::Middle` (some anims mutate cells — craters, smudges)

**Wall-specific (1 path, triggers 5-cell sweep)**
- `CellClass::PostDestructionWallCleanup` — calls RecalcAttributes on self + 4 cardinals

**Bridge mutation (18 sites across 8 functions)**
- `MapClass::DestroyBridgeWalker_NS_Low` / `_EW_Low` / `_NS_High` / `_EW_High` — 3 sites each (primary + 2 sibling cells)
- `MapClass::ApplyBridgeDestruction_NS_Low` / `_EW_Low` / `_NS_High` / `_EW_High` — 3 sites each (per-cell visual)
- `MapClass::RepairBridgeWalker_NS_Low` / `_EW_Low` / `_NS_High` / `_EW_High` — 3 sites each

**Tile-layer flood-fill (1 path)**
- `MapClass::SetOverlayAndPropagate` — called per cell as the flood-fill visits

**Map init (2 paths)**
- `MapClass::InitCellAttributes` — map-load sweep over every cell
- `ScenarioClass::Full_Init` — scenario startup

**Building / unit occupancy (5 paths)**
- `HouseClass::Sell_Building_At_Cell` (sell → recompute)
- `BuildingClass::Place_OccupyMap` (place → recompute)
- `BuildingTypeClass::SetOwnerAndOccupy` (ownership/occupy)
- `TechnoClass::EnterCell_AddToMultiCells` (unit entering multi-cell footprint)
- `TechnoClass::ExitCell_RemoveFromMultiCells` (unit leaving)

**Terrain & scripting (4 paths)**
- `TerrainClass::Limbo` (tree/terrain-object removal)
- `FUN_006E21E0` (trigger-action dispatch path)
- `FUN_00581140` (smudge / crater spawn, 2 sites)
- `FUN_0074E930` (mission-event path, 2 sites)

**Unidentified (14 call sites across 3 functions)**
- `FUN_00598960` — 4 sites, likely pathfinder update
- `FUN_005A3AE0`, `FUN_00586990`, `FUN_00684C30` — 1 site each
- `FUN_005FC981..005FD200` — 5 consecutive call sites in one function, likely OverlayTypeClass spawn/remove helpers

### 2.3 Takeaway for the Rust port

A single `world.recalc_cell_attributes(coord)` dispatcher should be called from every mutation that changes any of:
- `OverlayTypeIndex` (place / remove / damage-stage at threshold)
- `IsoTileTypeIndex` (tile flood-fill, cliff destruction)
- Multi-cell occupancy (building place/sell, multi-tile unit enter/exit)
- Terrain-object add/remove (trees)
- Bridge state transitions

The dispatcher must run LAT re-pick (`ApplyLAT_and_SlopeFixup` equivalent) and zone re-classification. Everything else the binary's RecalcAttributes does (auto-anim spawn, tube construction) is either one-shot at map init or part of separate systems we don't need to mirror in the dispatcher itself.

---

## 3. Wall destroy LAT retrigger — detailed flow

### 3.1 The call chain (verified)

```
warhead hits wall cell
  → Apply_area_damage  (0x004896AD)
    → CellClass::DestroyOverlay  (0x00480CB0)
        ├── roll probabilistic damage (damage vs Strength)
        ├── bump cell.field_0x11E += 0x10  (upper nibble = damage stage)
        ├── chain-react concrete walls (stage == max-1 && DamageLevels > 2)
        ├── [if destroyed]:
        │     cell.OverlayTypeIndex = -1
        │     cell.field_0x11E = 0
        │     ├── CellClass::RecalcAttributes(self)          ← LAT re-run #1
        │     ├── for each of 4 cardinals:
        │     │     CellClass::PostDestructionWallCleanup(neighbor)
        │     │       ├── rebuild neighbor's connectivity nibble
        │     │       ├── auto-destruct check (isolated + max-damage)
        │     │       └── CellClass::RecalcAttributes(neighbor) ← LAT re-run #2-5
        │     └── 8-neighbor OreNeighborCount decrement
```

**Five cells re-LAT per wall destruction** (self + 4 cardinals), not including cascade into concrete-wall chain reactions which multiply this.

### 3.2 `PostDestructionWallCleanup` internals (from prior report — verified)

Verified against the prior [WALL_CONNECTION_AND_DESTRUCTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WALL_CONNECTION_AND_DESTRUCTION_GHIDRA_REPORT.md). The function walks `DAT_0081CC70 = [0, 2, 4, 6, -1]` — 4 cardinals + self. For each visited cell with a wall overlay, rebuilds the connectivity nibble, applies the per-type auto-destruct safety-net, and then calls `RecalcAttributes`. If the cell auto-destructs it additionally calls `AssignOrphanedCellZone` and decrements the 8-neighbor OreNeighborCount.

### 3.3 What this means for the Rust implementation

When the Rust `recalc_cell_attributes()` lands:

- The existing `damage_wall_overlay()` in [src/map/overlay_grid.rs](src/map/overlay_grid.rs#L256-L331) destroys walls but does **not** trigger LAT recomputation on the destroyed cell OR its 4 cardinal neighbors.
- Wiring `PostDestructionWallCleanup` equivalent into the destruction path will fix **both** connectivity rebuild and LAT re-flow for free.
- Ground LAT under the destroyed wall (e.g., pavement) will then re-tile correctly because LAT is idempotent — re-running it on a pave cell whose neighbors are unchanged gives the same result; re-running on a cell whose wall just vanished exposes the pave cell as a new LAT endpoint.

There is no code path in the binary that specifically "re-flows pavement after wall destroy". The pavement re-flow is a **natural consequence** of `RecalcAttributes` running on each of the 5 affected cells.

---

## 4. Bridge damage-variant — the `CellFlags & 0x2000` flood-fill

### 4.1 The TMP flag that gates the whole mechanism (verified at 0x005471F0)

```
IsometricTileTypeClass::HasDamagedVariantAtSubTile(tile_type, sub_tile_idx) -> bool:
    tmp_header = vtable[0x9C](tile_type)        // fetch TMP data
    cell_idx   = sub_tile_idx % (width * height)
    cell_data  = tmp_header[4 + cell_idx]
    if cell_data == null: return false
    return (*(u32*)(cell_data + 0x24) >> 2) & 1  // bit 2 = 0x04 = FLAG_HAS_DAMAGED_DATA
```

So a TMP cell advertises "I have a baked damaged variant" via bit `0x04` in the per-cell flag DWORD at offset +0x24. Tiles that don't set this bit don't support the damage-variant channel at all (variant-pick falls back to the PRNG variant selector).

### 4.2 `MapClass::ToggleBridgePavement` internals (verified at 0x0056E990)

```c
void ToggleBridgePavement(short* coord, uint new_state, char suppress_self) {
    CellClass* cell = map.get_cell(coord);   // off-map sentinel = &DAT_00abdc50

    if (suppress_self == 0) {                // initial call only
        if (cell.IsoTileTypeIndex == 0xFFFF || == 0xFF) return;   // clear / empty
        if (!HasDamagedVariantAtSubTile(cell.SubTileIndex)) return; // TMP flag 0x04 gate
        // Binary projects world coord to screen first via CoordsToClient2,
        // then applies Z correction in SCREEN space, then -0x80 inset for
        // the 256×256 rect. Verified at 0x0056E9D9 (call site of
        // CoordsToClient2) — the prior version of this pseudocode skipped
        // the projection and fed world coords directly into DirtyScreenRect,
        // which only works on a non-rotated/non-iso projection.
        Coord  world_in    = coord;
        Point  screen_out;
        TacticalClass::CoordsToClient2(&world_in, &screen_out);
        screen_out.y += (char)cell.Level * -15;      // Z correction in screen space
        TacticalClass::DirtyScreenRect(
            screen_out.x - 128,
            screen_out.y - 128,
            256, 256, 0);
    }

    uint current_bit = (cell.Flags >> 13) & 1;
    if ((new_state & 1) == current_bit) return;        // already in target state

    int seed_tile_id = cell.IsoTileTypeIndex;

    // Bit flip via explicit clear-then-set (not xor — allows forcing state)
    cell.Flags = (cell.Flags & ~0x2000) | ((new_state & 1) << 13);
    RadarClass::MarkTerrainDirty(cell);

    // 8-neighbor flood-fill — only propagate to cells with the same tile_id
    for (int dir = 0; dir < 8; dir++) {
        short[2] neighbor_coord = coord + g_DirectionOffsets[dir];
        CellClass* neighbor = map.get_cell(neighbor_coord);
        if (neighbor.IsoTileTypeIndex == seed_tile_id) {
            ToggleBridgePavement(neighbor_coord, new_state, suppress_self=1);
        }
    }
}
```

**Critical details:**
- `new_state & 1` — only bit 0 of the state param is used. `1` = "show damaged", `0` = "show pristine".
- `cell.Flags & 0x2000` = bit 13. The set is `Flags = (Flags & 0xFFFFDFFF) | ((state & 1) << 13)` — an idempotent force-set, not a toggle despite the function name.
- Propagation criterion is **tile_id equality**, not overlay equality. A pavement strip under a wall segment keeps rolling the damage bit; the wall's overlay doesn't stop it. A mixed-tile neighbor (e.g., grass border) terminates the cascade.
- `HasDamagedVariantAtSubTile` is the gate — but only checked on the non-recursive call. Recursive calls **skip** the gate. This means the function trusts that whoever kicked off the flood-fill checked it, then assumes all cells sharing the same tile_id also have the damaged-variant flag (which is true because they're the same TMP).
- The dirty rect is Z-adjusted by `cell.Level * -15` (one level ≈ 15 screen px up) — so the dirty rect covers both the cell's current Z and one cell-height worth of elevation.

### 4.3 What consumes the `0x2000` bit at render time

From `ISOMETRIC_TILE_TYPE_CLASS_GHIDRA_REPORT.md` §11.3 + §17 (verified):

```python
def pick_tile_variant(cell, tile_type):
    sub_tile = cell.SubTileIndex
    if tile_type.VariantCount < 2:
        return 0
    elif HasDamagedVariantAtSubTile(tile_type, sub_tile):  # TMP +0x24 bit 2
        return (cell.Flags >> 13) & 1                      # damaged-data path
    else:
        return GetTileVariantIndex(cell, tile_id, tile_type.VariantCount)  # PRNG from coord
```

So the damage variant **replaces** the normal PRNG variant selector. A bridge tile with PRNG variants 0/1 (jitter) and a damaged baked variant becomes binary: `Flags & 0x2000 == 0` → pristine, `!= 0` → damaged. The PRNG jitter is lost while damaged.

### 4.4 Exact caller pattern — when each direction is used

Full xref map of `ToggleBridgePavement`:

| Caller | state arg | Semantic |
|--------|-----------|----------|
| `UpdateRamp_{NS,EW}_{DamageA,DamageB}_{High,Low}` (8 functions) | `1` | Damage event — turn ON damage bit |
| `UpdateRamp_{NS,EW}_{CollapseA,CollapseB}_{High,Low}` (8 functions) | `1` | Collapse event — keep damage bit on (tile already damaged) |
| `ProcessBridgeDestruction_{Low,High}` (2 functions, at the "destroyed" path) | `1` | Force damage bit on during destruction (2 sites each) |
| `UpdateBridgeEdgeTiles_Low` @ `0x00570AE0` (the REAL one) | implicit via its helpers | ramp-edge re-evaluation during rebuild |
| `FUN_00569760` (variants of edge-tile walker pair, probably `RestorePavementUnderBridge_Low`) | `0` | Repair/restore — turn OFF damage bit; 4 sites (pav-class-E, pav-class-S × WoodBridge and BridgeSet) |
| `FUN_00568E40` (byte-identical structure to FUN_00569760, paired for High bridges) | `0` | Same pattern, high-bridge set |
| `ToggleBridgePavement` itself (recursion) | propagated | 8-neighbor walk |

**Key finding:** damage AND collapse paths both pass `state=1` (turn bit ON — damage is set; collapse inherits the already-damaged state). Only the **repair walkers** (FUN_00569760 / FUN_00568E40 family + RepairBridgeWalker_*) pass `state=0` to clear it. This means once a pavement-under-bridge is flipped to damaged, it stays damaged until explicitly repaired — even if the bridge above collapses to water, the pavement tiles surrounding the collapse keep the damage bit set.

### 4.5 The two-channel model in concrete terms

For a bridge body cell over time:

| Event | `IsoTileTypeIndex` change | `Flags & 0x2000` change | `OverlayTypeIndex` change |
|-------|---------------------------|-------------------------|--------------------------|
| Healthy | (initial) | 0 | bridge body overlay (0xCD..0xDE) |
| First damage hit | unchanged | → 1 (via ToggleBridgePavement on adjacent pavement under the ramp) | damage-stub overlay (0xDF/0xE1/0xE3/0xE5) |
| Second damage hit (collapse) | **changed** (via SetOverlayAndPropagate to destroyed tile) | 1 | destroyed-body overlay (0xE7/0xE8) |
| Full collapse, connected span | via DestroyBridgeWalker → ApplyBridgeDestruction → `RecalcAttributes` on each cell | 1 | spreads across span |
| Engineer repairs bridge hut | via RepairBridgeWalker → restore tiles | → 0 (via FUN_00569760/FUN_00568E40 family) | overlay restored to healthy |

So the `0x2000` bit tracks a **separate, sticky damage visual** from the tile-id channel. They interact but don't collapse into one mechanism.

---

## 5. `MapClass::SetOverlayAndPropagate` — the tile-id flood-fill (already documented)

Covered in prior [PAVEMENT_AND_TILE_PROPAGATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PAVEMENT_AND_TILE_PROPAGATION_GHIDRA_REPORT.md) §2. Re-verified via xrefs for this pass:
- Signature `(coord, new_tid, old_tid, z_fudge, suppress_dirty)`
- 8-neighbor recursion
- Calls `RecalcAttributes` on every visited cell (confirmed in the xref list for 0x47D2B0 — `00 56 EC 80 in MapClass__SetOverlayAndPropagate`)
- Only touched when tile_id actually changes (idempotent on already-changed cells)
- Used by bridge bridgehead damage (per-ramp tile swap) and final collapse (tile → water)

No new findings beyond the prior report. The misleading function name (the ghidra label says "Overlay") is just a label error — it is a tile-index flood-fill.

---

## 6. Data globals used (all verified runtime-populated)

| Global | Purpose | Populated by |
|--------|---------|--------------|
| `DAT_00AA0E28` | `BridgeSet` — base IsoTileTypeIndex of concrete (high) bridges | theater/tileset load |
| `DAT_00ABAD1C` | `WoodBridgeSet` — base IsoTileTypeIndex of wooden (low) bridges | theater/tileset load |
| `DAT_00ABAD30` | NS bridgehead overlay class base (BridgeSet-relative) | theater/tileset load |
| `DAT_00AA1028` | EW bridgehead overlay class base (BridgeSet-relative) | theater/tileset load |
| `DAT_00ABC1E8` | Pavement-under-bridge class (East walk, sub_tile==4) | theater/tileset load |
| `DAT_00AA0E38` | Pavement-under-bridge class paired with DAT_00ABC1E8 | theater/tileset load |
| `DAT_00ABC1D0` | Pavement-under-bridge class (South walk, sub_tile==2) | theater/tileset load |
| `DAT_00AA1540` | Pavement-under-bridge class paired with DAT_00ABC1D0 | theater/tileset load |
| `g_DirectionOffsets` @ `0x0089F688` | 8 × (dx:i16, dy:i16) for direction walks | `FUN_0049F300` startup |
| `DAT_0081CC70` | `[0, 2, 4, 6, -1]` — wall-cleanup direction table (self + 4 cardinals) | static |
| `DAT_0087F850` / `DAT_0087F858` | Zone-lookup side tables written by RecalcAttributes | `ZoneMap::*` |

---

## 7. Current Rust implementation status

Rust scan of [src/](src/):

### 7.1 LAT & RecalcAttributes

- [src/map/lat.rs](src/map/lat.rs) — `apply_lat()` at line 217 runs once at map load over all cells. Has hardcoded exemption pairs (lines 164–186). **No per-cell re-invocation.**
- **No `recalc_cell_attributes()` function exists.** Line 215 has an explicit TODO for this: "extract per-cell variant when a runtime terrain-tile rewriter lands (bridge destruction, crater tiles, destructible cliffs)".

### 7.2 Wall destroy

- [src/map/overlay_grid.rs](src/map/overlay_grid.rs) lines 256-331 — `damage_wall_overlay()` mirrors `CellClass::DestroyOverlay` (probabilistic damage gate, upper-nibble damage stage, concrete chain-damage of 4 cardinals).
- Push to `dirty_cells` on destroy (line 97); app drains these and calls `recalc_overlay_passability()` on the **destroyed cell only** (overlay_grid.rs:179).
- **No `PostDestructionWallCleanup` equivalent.** No re-LAT on the 4 cardinal neighbors. No re-LAT on the destroyed cell beyond passability.
- No wall-connectivity rebuild for neighbors after destroy.

### 7.3 Bridge damage (updated 2026-05-20 — damage-variant channel is now wired)

- [src/sim/bridge_state.rs](src/sim/bridge_state.rs) — `apply_damage()` does whole-group binary destruction (all cells flip to destroyed when group HP hits 0).
- [src/sim/bridge_specs.rs](src/sim/bridge_specs.rs) — RE-backed helpers `low_bridge_overlay_damage_step_ra2()` and `low_bridge_connected_section_selector_yr()` exist but aren't wired into the live path.
- **Damage-variant bit IS implemented** (was missing as of 2026-05-12). [src/sim/bridge_state/mod.rs:1129](src/sim/bridge_state/mod.rs#L1129) `apply_damaged_variant_flood_fill` mirrors the binary's `ToggleBridgePavement` 8-neighbor flood-fill on tile-id equality. Render-time pick at [src/map/terrain.rs:586-593](src/map/terrain.rs#L586-L593) reads `BridgeRuntimeState.damaged_variant` and routes through it — exact mirror of the binary's `(cell.Flags >> 13) & 1` read.
- **8-neighbor tile_id flood-fill primitive IS implemented** (was missing as of 2026-05-12) — co-located with the damage-variant flood-fill above.
- State-machine transitions (healthy → damaged → destroyed progression) — still partial; the binary-side state machine in [HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md](../05-damage-collapse-repair-cabhut/HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md) describes the full ProcessBridgeDamageStateMachine_High/Low cascade; our Rust impl handles the direct-overlay walker case but not all intermediate progressions.

### 7.4 CellFlags model

- Rust uses distributed bool fields ([src/map/resolved_terrain.rs](src/map/resolved_terrain.rs)), not a packed `u32 Flags`. There is no equivalent of `CellClass + 0x140`.
- Wall overlay damage is encoded in `overlay_data` upper nibble ([src/map/overlay_grid.rs:14-26](src/map/overlay_grid.rs#L14-L26)) — this is fine; it matches the `field_0x11E` byte.
- For the bridge damage-variant channel, we need either a new bool (`bridge_damaged_variant`) or a real `flags: u32` field. Given ~8 bits used and more likely incoming (cliff redraw, shroud levels), a packed `u32 flags` is probably correct — but that's a design choice for a later brainstorm.

---

## 8. Port plan implications (not a plan — just the shape)

Three distinct primitives are needed in Rust. They compose as follows:

1. **`world.recalc_cell_attributes(coord)`** — the dispatcher.
   - Re-run ground LAT for this cell (picks variant from 4-cardinal neighbor mask).
   - Re-classify movement zone.
   - No auto-anim / tube / shroud-LandType logic — those are map-init concerns or separate systems.
   - Called from every mutation path listed in §2.2, starting with the easy wins (wall destroy, building place/sell, tile flood-fill).

2. **`world.flood_fill_tile_id(coord, new_tid, old_tid)`** — 8-neighbor tile-index flood-fill.
   - Exact mirror of `SetOverlayAndPropagate`.
   - Calls `recalc_cell_attributes` on each visited cell.
   - Marks radar/terrain dirty on each visit; only dirties screen rect on the initial call.
   - Needed for bridge final-collapse (tile → water).

3. **`world.flood_fill_damage_variant(coord, state: bool)`** — 8-neighbor damage-variant flood-fill.
   - Exact mirror of `ToggleBridgePavement`.
   - Only propagates across cells with identical tile_id.
   - Only operates on cells whose TMP cell advertises "has damaged variant" (the `0x04` flag on the TMP per-cell struct at +0x24).
   - Does NOT call `recalc_cell_attributes` — damage-variant is a pure render-time concern and doesn't affect LandType/zone.
   - Flips a `damaged_variant: bool` on the cell (or bit 13 of a `flags` field).

**Ordering priority** (cheapest-first): #1, then #3 (for bridge damage state machine), then #2 (for bridge destruction).

Wall destroy LAT re-flow is a free ride on #1 — no new primitive needed for walls once #1 lands and `PostDestructionWallCleanup` is wired.

---

## 9. Open questions / deferred

1. **`ToggleBridgePavement` mystery callsites at `0x0056AB5A` / `0x0056ACC9`.** These are inside a function that starts somewhere in the `0x0056A8XX` range (not auto-defined in Ghidra). Context shows they pass direction=2 and direction=4 and immediately call `FUN_00569760` afterward. Probably part of `ProcessBridgeDestruction_Low`'s per-direction dispatch. Doesn't affect the semantic model — `ToggleBridgePavement` behavior is already fully understood. Leaving for a dedicated bridge-destruction decomp session.

2. **Unidentified RecalcAttributes callers** (§2.2, "Unidentified" section — 14 sites across 3 functions). `FUN_00598960` with 4 sites is the highest-value candidate — likely the pathfinder's cell-update hook. Would be worth tracing when porting pathfinding invalidation, but not now.

3. **Does `recalc_cell_attributes` need to run during `TechnoClass::EnterCell_AddToMultiCells`?** That xref surprised me — why would a unit entering a cell need a LAT re-pick? Hypothesis: it's only for multi-cell building placement (the building footprint straddles cells and each one needs re-attr). But the function is called for techno movement too. Leaving as a verification task before wiring unit-motion paths.

4. **Auto-destruct safety-net in `PostDestructionWallCleanup` (§5 of the wall report)** — the per-type data-byte destruction thresholds. The prior report has these but flagged them as "cleanup safety-net" rather than primary destruction gate. Worth verifying in one more Ghidra pass before porting, because the thresholds may differ from what we'd naively derive from DamageLevels.

---

## 10. Summary of new findings beyond prior reports

1. **`RecalcAttributes` has 71 call sites, not the "30+" mentioned in the prior pavement report.** Full enumeration categorized above in §2.2.
2. **TMP per-cell flag 0x04 gate lives at TMP-cell-data + 0x24, bit 2** — verified via decompilation of `FUN_005471F0`. Previous references were consistent but derivation wasn't shown.
3. **`ToggleBridgePavement` is called with state=1 from BOTH damage AND collapse paths** (not just damage) and state=0 only from the pavement-repair walkers. So the damage visual persists through collapse and only clears on explicit repair.
4. **Wall destroy already does re-LAT on 5 cells via existing `PostDestructionWallCleanup` → `RecalcAttributes` chain.** Once Rust's `recalc_cell_attributes()` is added, wiring wall cleanup gives us both connectivity rebuild AND pave-LAT re-flow "for free".
5. **The two flood-fills (`SetOverlayAndPropagate` vs `ToggleBridgePavement`) use different propagation criteria** — tile_id equality for both, but `SetOverlayAndPropagate` swaps the tile_id (so the criterion dynamically shifts to `== old_tid`), while `ToggleBridgePavement` keeps `== seed_tid` throughout (stable).
6. **`TechnoClass::EnterCell_AddToMultiCells` and `ExitCell_RemoveFromMultiCells` call RecalcAttributes** — unexpected. Flagged as an open verification task (§9.3).

---

## Sources

**Ghidra addresses decompiled this pass:**
- `0x0047D2B0` — `CellClass::RecalcAttributes`
- `0x0056E990` — `MapClass::ToggleBridgePavement`
- `0x00572230` — `MapClass::UpdateRamp_NS_DamageA_High` (spot-check of damage path)
- `0x00572440` — `MapClass::UpdateRamp_NS_CollapseA_High` (spot-check of collapse path)
- `0x00570AE0` — `MapClass::UpdateBridgeEdgeTiles_Low` (real vs FUN_00569760 paired walker)
- `0x00569760` / `0x00568E40` — pavement-restore walker pair (low/high, state=0 repair)
- `0x005471F0` — `IsometricTileTypeClass::HasDamagedVariantAtSubTile`
- `0x00485590` — `FUN_00485590` (ore-spread RecalcAttributes caller identification)

**Xrefs pulled:**
- `ToggleBridgePavement` @ `0x0056E990` — 31 xrefs (8 UpdateRamp Low, 8 UpdateRamp High, ProcessBridgeDestruction_{Low,High}, UpdateBridgeEdgeTiles pavement pair, recursion, 2 mystery sites in 0x56A8XX range)
- `RecalcAttributes` @ `0x0047D2B0` — 71 xrefs (full categorized list in §2.2)

**Prior reports cross-verified:**
- [WALL_CONNECTION_AND_DESTRUCTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/WALL_CONNECTION_AND_DESTRUCTION_GHIDRA_REPORT.md) (wall destroy chain)
- [PAVEMENT_AND_TILE_PROPAGATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/PAVEMENT_AND_TILE_PROPAGATION_GHIDRA_REPORT.md) (SetOverlayAndPropagate + ToggleBridgePavement overview)
- `HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md` (bridge damage state machine)
- `ISOMETRIC_TILE_TYPE_CLASS_GHIDRA_REPORT.md` (TMP flag 0x04, LAT algorithm, Flags bit 13)
- [BRIDGE_RENDERING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/06-render-presentation-audio/BRIDGE_RENDERING_GHIDRA_REPORT.md), `BRIDGE_SYSTEM.md`, `CELLCLASS_STRUCT_GHIDRA_REPORT.md`

**Rust source audited:**
- [src/map/lat.rs](src/map/lat.rs) (load-only LAT)
- [src/map/overlay_grid.rs](src/map/overlay_grid.rs) (wall damage)
- [src/map/resolved_terrain.rs](src/map/resolved_terrain.rs) (no CellFlags)
- [src/sim/bridge_state.rs](src/sim/bridge_state.rs), [src/sim/bridge_specs.rs](src/sim/bridge_specs.rs) (binary bridge destruction only)
