# MapClass — Ghidra Research Revisit (2026-04-24)

Third pass over `MapClass` at `0x565090` / vtable `0x7ED404` / global
`0x0087F7E8`. Extends and **corrects** `MAPCLASS_GHIDRA_REPORT.md`
(original, 2026-04-06) and [MAPCLASS_GHIDRA_REPORT_FOLLOWUP.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_GHIDRA_REPORT_FOLLOWUP.md)
(2026-04-22).

**Confidence:** HIGH (every finding below is backed by a freshly-read
decompilation or raw-memory dump listed in Sources).

**Active in YR:** Yes — core infrastructure, always active.

---

## 1. What changed vs the prior two reports

Four substantive corrections plus four new findings.

**Corrections** (prior reports overstated or mislabeled):

1. **Vtable size: 30 slots, not 64.** The follow-up's "full 64-slot
   enumeration" dumped 256 bytes starting at `0x7ED404` and treated
   every dword as a vtable entry. But `0x7ED480` is the **next**
   vtable (generic `VectorClass<T>` shared with WaveClass and others);
   the embedded `VectorClass<CellClass*>` at MapClass+0x138 points at
   it. So the MapClass vtable actually spans `0x7ED404 – 0x7ED47B`
   (30 slots). Slot 30 is **not** a function pointer — `0x7ED47C`
   holds `0x008055E0`, which is the RTTI Complete Object Locator for
   the adjacent vtable. Addresses that the follow-up reported as
   slots 30–63 belong to two unrelated vtables that happen to sit
   next to MapClass in `.rdata`.

2. **Slot 3 purpose: IsCellExplored, not "CellHasBuilding-style".**
   `FUN_005656D0` returns `(cell.ShroudFlags >> 3) & 1`. Per
   `CELLCLASS_STRUCT_GHIDRA_REPORT.md`, cell+0x12C bit 3 is the
   *explored* bit. The correct name is
   `MapClass::IsCellExplored(CellStruct)`. This slot is inherited by
   DisplayClass, RadarClass, PowerClass, SidebarClass (all reference
   the same `0x5656D0` at their slot 3).

3. **+0x1158 is not an "init flag with no readers" — it's a
   (near-dormant) bridge-overlay draw-cache stamp.** Readers exist:
   `CellClass::DrawOverlay_Body` at `0x47F77B` and `0x47F83D` both do
   `MOV CL,byte ptr [0x00880940]` (= `g_MapClass + 0x1158`). The cell
   caches its last-drawn value at `cell+0x118` and skips redraw when
   stamp/frame/viewport all match. The only writer found (register-
   relative, via `*(this + 0x1158) = 0`) is `Init_Clear`. Since
   nobody increments it during normal play, the check degenerates to
   "cell has ever been drawn once" — a TS-era invalidation mechanism
   still present in the binary but effectively dead in YR.

4. **UpdateRamp_\* is 16 functions, not 12.** The follow-up said the
   family has 12 entries (`{EW|NS} × {High|Low} × {CollapseA|CollapseB
   |DamageA|DamageB}` = 2×2×4 = 16). Ghidra lists all 16 labeled —
   see §4 for the full address table.

**New findings:**

5. **+0x115C DynVec is the "cells with attached object" registry.**
   Finally traced: each entry is a 4-byte packed `CellStruct(short x,
   short y)`. Entries are pushed by `FUN_00485250`
   (attach-object-to-cell) and removed by `FUN_00485130`
   (detach-object-from-cell). Producers include `[CellTags]` map
   loading, bridge repair hut placement, and `MapClass::
   UnregisterBridgeRepairHut`. See §5 for the full model.

6. **+0x74–0x7F (12 bytes) and +0x11C–0x123 (8 bytes) are genuinely
   dead.** Confirmed by: no writes in the constructor (0x565090), no
   writes in `Init_Clear` (0x5659F0), no writes in vtable slot 5
   Init/Alloc (0x565800), no writes in `UpdateBridgeZonesHelper`
   (0x56C510). No direct xrefs to the corresponding globals
   (`0x87F85C`, `0x87F860`, `0x87F864`, `0x87F904`, `0x87F908`) from
   any function. Treat as 20 bytes of reserved padding / TS-era
   residue that the compiler kept in the class layout.

7. **"FUN_0056xxxx stragglers" flagged by the follow-up are not
   MapClass methods.** `FUN_00560BF0` is the video-mode / surface
   re-creation routine (creates primary/sidebar/hidden surfaces,
   calls `SidebarSurface_Create`, `Set_View_Dimensions`). It
   neighbors MapClass in address space but has no `this`-pointer
   relationship. No further action needed on these.

8. **Rust parity notes updated.** The prior "Rust status" column is
   stale in two places:
   - `src/sim/pathfinding/zone_incremental.rs` already implements the
     fast path for single-cell zone changes (with a 200-cell
     threshold fallback to full rebuild). The *algorithm* differs
     from gamemd — Rust does bbox-based clear-and-reflood per
     category; gamemd does 8-neighbor adoption with a ≤3-conflict
     limit. Both are valid incremental strategies; parity audit below.
   - `src/sim/bridge_state.rs::BridgeRuntimeCell` has a `deck_level:
     u8` field and models multi-level damage, not binary
     intact/destroyed.

   See §6 for the refreshed parity matrix.

---

## 2. Vtable (0x7ED404, 30 slots) — corrected

Raw dump of `[0x7ED404, 0x7ED47C)` = 30 dwords:

| Slot | Addr | Name / Inferred purpose | Source |
|------|------|------|------|
| 0 | `0x4F4240` | scalar deleting destructor (GScreenClass base) | inherited |
| 1 | `0x40D230` | ctor helper (GScreenClass base) | inherited |
| 2 | `0x40D240` | ctor helper (GScreenClass base) | inherited |
| 3 | `0x5656D0` | **MapClass::IsCellExplored(coord)** — returns `(cell.ShroudFlags>>3)&1` | **CORRECTED** |
| 4 | `0x588BF0` | MapClass scalar deleting destructor override | verified |
| 5 | `0x565800` | **MapClass::Init_Alloc** — allocates cell array, zone hash, zone_graph[3], zeros zone_ids[13] | verified |
| 6 | `0x4F42B0` | inherited | GScreenClass |
| 7 | `0x5659F0` | **MapClass::Init_Clear** — pause crate timers, `+0x148 = 0xD`, `+0x1158 = 0` | verified |
| 8 | `0x4F42E0` | inherited | GScreenClass |
| 9 | `0x4F4320` | inherited | GScreenClass |
| 10 | `0x4F4BB0` | inherited | GScreenClass |
| 11 | `0x4F43F0` | inherited | GScreenClass |
| 12 | `0x4F4410` | inherited | GScreenClass |
| 13 | `0x4F4450` | inherited | GScreenClass |
| 14 | `0x4F42F0` | `MarkNeedsRedraw(2)` | GScreenClass, override-able |
| 15 | `0x4F4480` | inherited | GScreenClass |
| 16 | `0x4AEBD0` | inherited | GScreenClass |
| 17 | `0x4F45B0` | inherited | GScreenClass |
| 18 | `0x4C9150` | abstract placeholder (same fn for 18–21) | purecall-style |
| 19 | `0x4C9150` | abstract placeholder | — |
| 20 | `0x4C9150` | abstract placeholder | — |
| 21 | `0x4C9150` | abstract placeholder | — |
| 22 | `0x565AA0` | MapClass cell-array reset (null all 262144 slots) | verified |
| 23 | `0x565B00` | MapClass cell-array resize helper | verified |
| 24 | `0x565BC0` | MapClass cell-array destructor walk (zeros `+0x134` at entry) | verified |
| 25 | `0x577920` | **MapClass::UnregisterBridgeRepairHut(Techno\*)** | verified |
| 26 | `0x4AEBE0` | inherited | GScreenClass |
| 27 | `0x56BBE0` | **MapClass::UpdateCrateRegenTimers** (per-tick) | verified |
| 28 | `0x565C10` | **MapClass::InitMapCells** (resize + `[Map]` Size/LocalSize parsing) | verified |
| 29 | `0x567230` | **MapClass::Viewport_Resized** — updates `in_playfield` byte on every Techno, plays idle voice on entry | verified |

The layout beyond slot 29 is not MapClass vtable — subsequent `.rdata`
addresses resolve to adjacent vtables (VectorClass at `0x7ED480`, etc).

---

## 3. Corrected struct layout — changed rows only

Full layout is still valid in the original report. Revisions:

| Offset | Prior label | Corrected label | Evidence |
|--------|-------------|------------------|----------|
| +0x74–0x7F | "Zone metadata (12 bytes, unknown)" | **Reserved / dead** (12 bytes). No writers in ctor / Init_Clear / Init_Alloc / UpdateBridgeZonesHelper. No global xrefs. | absence |
| +0x11C–0x123 | "Unknown (8 bytes)" | **Reserved / dead** (8 bytes). Same evidence profile as +0x74. | absence |
| +0x134 | "Cells-destroyed counter / scenario re-init flag" | Confirmed. Written in `FUN_00565BC0` (=0 at entry, vtable slot 24) and `ScenarioClass::Full_Init` (at `0x687B9C`). Scenario load/unload state, not gameplay logic. | xref trace |
| +0x1158 | "1-byte init flag (no readers found)" | **1-byte bridge-overlay draw-cache stamp**. Read at `0x47F77B`/`0x47F83D` in `CellClass::DrawOverlay_Body`. Written = 0 by Init_Clear. Effectively dormant in YR (no increment path observed). | xref trace |
| +0x115C (DynVec) | "Purpose unclear" | **Cells-with-attached-object registry**. Each entry is packed CellStruct. See §5. | xref trace |

---

## 4. UpdateRamp_\* family — complete enumeration (16 fns)

Prior report said 12; the full set is 16.

| Orientation | Height | Variant | Address |
|-------------|--------|---------|---------|
| NS | Low | DamageA | `0x56ED40` |
| NS | Low | DamageB | `0x56EE40` |
| NS | Low | CollapseA | `0x56EF50` ← documented in follow-up |
| NS | Low | CollapseB | `0x56F2F0` |
| EW | Low | DamageA | `0x56F690` |
| EW | Low | DamageB | `0x56F7A0` |
| EW | Low | CollapseA | `0x56F8B0` |
| EW | Low | CollapseB | `0x56FC80` |
| NS | High | DamageA | `0x572230` |
| NS | High | DamageB | `0x572330` |
| NS | High | CollapseA | `0x572440` |
| NS | High | CollapseB | `0x5727E0` |
| EW | High | DamageA | `0x572B80` |
| EW | High | DamageB | `0x572C90` |
| EW | High | CollapseA | `0x572DA0` |
| EW | High | CollapseB | `0x573170` |

Template: 2 orientations × 2 heights × 4 variants (DamageA/B +
CollapseA/B). All share the cell-step `+0x11E` state machine (0 → 7 →
8 → collapsed) and the tile-constant dispatch described in the
follow-up. `DamageA` and `DamageB` variants handle the two
half-bridges that meet at a ramp; `CollapseA/B` dispatch to
`CellClass::BlowUpBridge` when the ramp fully fails.

Low-bridge base tile constants: `DAT_00ABAD1C`. High-bridge base:
`DAT_00AA0E28`. Other constants vary per variant — worth tabulating if
a full bridge-damage deep-dive is ever needed, but the pattern is
uniform enough that documenting one (NS/Low/CollapseA, §4 of the
follow-up) covers the family for parity purposes.

---

## 5. The +0x115C DynVec — cells with attached objects

### What it stores

A `DynamicVectorClass<CellStruct>` (4 bytes per entry, packed as
`(int16 x, int16 y)`). Entries are the **map coords of cells whose
`cell+0x3c` field points at some object** (tag, bridge repair hut,
etc.).

Globals alias for this DynVec (MapClass singleton `0x87F7E8 + 0x115C
= 0x880944`):

| Global | Byte offset | Field |
|--------|-------------|-------|
| `DAT_00880944` | +0x115C | vtable (`&PTR_FUN_007E3890`) |
| `DAT_00880948` | +0x1160 | data_ptr |
| `DAT_0088094C` | +0x1164 | capacity |
| `DAT_00880950` | +0x1168 | owns_memory |
| `DAT_00880951` | +0x1169 | flag |
| `DAT_00880954` | +0x116C | count |
| `DAT_00880958` | +0x1170 | grow_step (=10) |

### Producers — `FUN_00485250(cell, object)` = AttachObject

```
void AttachObject(cell, object):
    if cell.attached_object_at_0x3c != 0:
        decrement cell.attached_object.refcount_at_0x2c
    cell.attached_object_at_0x3c = object
    if object != NULL:
        push cell.coord_at_0x24 into g_MapClass[+0x115C]_DynVec
        increment object.refcount_at_0x2c
```

Callers observed:
- `Read_Map_Section_And_IsoMapPacks` (`0x004ACE70`) — Ghidra had mislabeled
  this as `BSurface__Constructor`; the real function is the scenario map loader
  that calls AttachObject during `[CellTags]` parsing to register tag
  associations. (corrected 2026-05-28: was `BSurface__Constructor (0x004AD2D0)`;
  binary `get_function_callers` at `0x485250` returns
  `Read_Map_Section_And_IsoMapPacks @ 004ace70` — ROOT_CAUSE: GHIDRA_ADDRESS_SHIFT,
  wrong entry point cited)
- `TechnoClass::ProcessCellAction` (`0x006E53A0`). (corrected 2026-05-28: was
  `0x006E54DC`; `get_function_callers` returns
  `TechnoClass__ProcessCellAction @ 006e53a0` — ROOT_CAUSE: GHIDRA_ADDRESS_SHIFT,
  mid-function address cited as function start)
- `MapClass::UnregisterBridgeRepairHut` (`0x00577920`) — calls it
  with `object = 0` (detach) before removing from the list.
  (corrected 2026-05-28: was `0x00577994`; `get_function_callers` confirms
  function starts at `0x00577920` — ROOT_CAUSE: GHIDRA_ADDRESS_SHIFT,
  call site within function body cited as function start)

### Consumers — `FUN_00485130(object)` = DetachObject

Removes `this` cell from *all* tracking lists:
1. If `this.some_field_at_0x3c == object`, dec its `+0x2c` refcount
   and zero the field.
2. Walk the MapClass+0x115C DynVec via vtable[4] (Find-by-coord) and
   remove the matching coord.
3. Zero `this.+0x2c` and `this.+0x30` if they equal `object`.
4. If `this.+0x28` is a DynVec, remove `object` from it via vtable
   dispatch.

Called from `MapClass::UnregisterBridgeRepairHut`.

### UnregisterBridgeRepairHut (0x577920, vtable slot 25)

```
UnregisterBridgeRepairHut(building):
    if building.abstract_type != 0x2C (Building):  # vtable+0x2C query
        return
    for i in 0 .. g_MapClass[+0x116C]:
        coord = g_MapClass[+0x1160][i]   # packed (short x, short y)
        cell = GetCellClass(coord)
        if cell.attached_object_at_0x3c == building:
            FUN_00485250(cell, 0)        # detach (clears cell+0x3C)
            idx = DynVec.Find(cell.coord_at_0x24)
            if idx in range:
                DynVec.remove_at(idx)
                i -= 1
    FUN_00485130(building)               # remove building from list
    # also remove from the bridge-hut-specific registry at DAT_008B41A8:
    idx2 = DAT_008B41A8_DynVec.Find(building)
    if idx2 in range:
        DAT_008B41A8_DynVec.remove_at(idx2)
```

Called when a bridge repair hut is destroyed or its building is
unlinked. Walks every registered attachment, drops the ones that
point at this building, and also removes the building from a
*separate* hut-only registry at `0x008B41A8`.

### Why two registries?

- `MapClass[+0x115C]` — cells with *any* attachment (tags + huts +
  others). Built from `[CellTags]` at scenario load.
- `DAT_008B41A8` — buildings that *are* bridge repair huts. Separate
  list, used for the faster "find all hut buildings" query.

### Implication for Rust parity

The `[CellTags]` trigger system depends on walking this registry to
find which cells have active tags. The Rust engine currently does not
expose an equivalent "cells with attached object" index. If map
triggers ever land, a similar index is needed — otherwise map-wide
tag ticks become O(cells) rather than O(attached).

---

## 6. Rust parity matrix — refreshed

Updates to the original report's §6. `✓` = implemented and matches
behavior; `≈` = implemented but algorithm/details differ; `✗` = not
implemented.

| gamemd feature | Rust location | Status |
|----------------|---------------|--------|
| 512×512 cell grid | `src/map/resolved_terrain.rs` + `src/sim/overlay_grid.rs` + `src/sim/occupancy.rs` | ≈ — split across structs; diamond map bounds in `src/map/terrain.rs` |
| Diamond playfield test (`Is_Cell_In_Playfield`) | `src/map/terrain.rs` | ✓ |
| Zone flood-fill | `src/sim/pathfinding/zone_build.rs` | ✓ |
| **Incremental zone update** | `src/sim/pathfinding/zone_incremental.rs` | ≈ — bbox clear-and-reflood per category; threshold 200 cells. gamemd uses per-cell 8-neighbor adoption with ≤3-conflict cap. Both converge; edge-case behavior may diverge on narrow topologies. |
| Bridge records / endpoint pairs | `src/sim/bridge_state.rs::BridgeEndpointRecord` | ✓ |
| **Multi-level bridge damage** | `src/sim/bridge_state.rs::BridgeRuntimeCell::deck_level: u8` | ✓ — models progression, not binary intact/destroyed |
| `UpdateRamp_*` family (16 fns) | — | ✗ — bridge damage applies but ramp state machine unmodeled |
| `ResolvePathCoord_BridgeAware` | — | ✗ — not yet needed but will be for bridge pathing |
| Shroud / vision | `src/sim/vision/mod.rs` | ✓ |
| `RevealShroud` spiral table | `src/sim/vision/` (different algorithm) | ≈ — Rust uses bounded-radius scan, not spiral table |
| Cell iterator (diagonal zigzag) | — | ✗ — not needed; Rust iterates flat |
| Viewport→idle-voice wiring (`MapClass::Viewport_Resized`) | — | ✗ — `in_playfield` byte / voice trigger not implemented; 99%-parity item |
| Crate system | — | ✗ — placement, regen, types all unimplemented |
| Cells-with-attached-object registry (`+0x115C`) | — | ✗ — needed if `[CellTags]`/triggers land |
| World↔cell coord transforms (`0x5654A0`, `0x565520`, `0x565660`) | `src/app_sim_tick.rs::world_point_to_cell`, `src/render/minimap_helpers.rs::world_to_minimap_pixel_from_cell`, `src/sim/movement/movement_step.rs` | ≈ — present, but drift against gamemd's exact formula would misalign cursor/click hit-tests. Worth a dedicated audit. |
| `InitCellAttributes` (full-map recalc) | — | ✗ — tag-rect stamping and tiberium total not computed |
| `SetOverlayAndPropagate` (tile flood-fill) | — | ✗ — used for bridge damage tile replacement |

---

## 7. INI keys (refreshed — consolidated)

| Key | Section | Type | Default | Effect | Active in YR |
|-----|---------|------|---------|--------|--------------|
| `LocalSize` | `[Map]` (scenario) | 4 ints | — | `MapClass+0xFC..+0x108` (local playfield rect) | Yes |
| `Size` | `[Map]` (scenario) | 4 ints | — | `MapClass+0xF4..+0xF8`; drives cell allocation in `FUN_00565C10` | Yes |
| `DestroyableBridges` | `[General]` | bool | yes | Gates bridge destruction in ramp/collapse family | Yes |
| `BridgeExplosions` | `[SpecialFlags]` | tile list | `TWLT026,TWLT036,TWLT050,TWLT070` | Overlay SHP IDs for bridge destruction VFX | Yes |
| `Crates` | `[General]` | bool | yes | Gates `UpdateCrateRegenTimers` (vtable slot 27) | Yes |
| `CrateRegen` | `[CrateRules]` | minutes | 3 | Converted to frames (×1800) per-slot timer | Yes |
| `CrateMinimum` / `CrateMaximum` | `[CrateRules]` | int | 1 / 255 | Crate slot fill bounds | Yes |
| `CrateRadius` | `[CrateRules]` | float | 3.0 | Area-effect crate radius | Yes |
| `SilverCrate`/`WoodCrate`/`WaterCrate` | `[CrateRules]` | string | HealBase/Money/Money | Solo-play crate bonus kind | Yes |
| `Shroud` | `[General]` | bool | yes | Scenario-level shroud enable | Yes |
| `FogOfWar` | `[SpecialFlags]` / scenario | bool | **no** | Gates `UpdateFogBorder` and shroud fog path | **TS-legacy** — stock YR = off |
| `ShroudGrow` / `ShroudRate` | `[General]` | bool / minutes | no / 4 | Shroud creep (TS-era) | TS-legacy — stock YR = off |
| `BlendedFog` | `[General]` | bool | yes | Blend-mode fog (vs dither) | Conditional on FogOfWar |
| `AircraftFogReveal` | `[General]` | cells | 6 | Aircraft sight range for fog purposes | Conditional on FogOfWar |

MapClass itself never calls `INIClass::Read*`. `[Map]` keys arrive via
`ScenarioClass` which then calls `FUN_00565C10` (vtable slot 28) and
`FUN_006E21E0` to populate the bounds fields.

---

## 8. Still-open gaps worth filling later

**Update (2026-04-24, Task 13):** Items 3, 4, 5 below have been
resolved; 1 and 2 remain low-priority and out of MapClass scope. See
[MAPCLASS_COMPLETE_DECODE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_COMPLETE_DECODE.md) §M.5 for the final open-question list.

1. **Exact world↔cell coord-transform audit.** The three functions
   `FUN_005654A0`, `FUN_00565520`, `FUN_00565660` encode the
   isometric camera math. Any drift between their output and the
   Rust equivalents would misalign clicks and selection brackets.
   → **Reclassified:** these three are HouseClass-local-grid skews,
   not the real tactical transforms. Real tactical transforms live
   at `0x6D1EB0 / 0x6D1F10 / 0x6D1FE0 / 0x6D2140 / 0x6D6590` and
   belong to DisplayClass/TacticalClass — out of MapClass scope.
   See [COORD_TRANSFORM_AUDIT_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/COORD_TRANSFORM_AUDIT_GHIDRA_REPORT.md).

2. **Zone-incremental algorithm divergence.** gamemd adopts a
   neighbor's cluster_id if ≤3 conflicts; Rust clears bbox and
   refloods per category. → **Deferred:** covered in depth in
   [ZONE_INCREMENTAL_DIVERGENCE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ZONE_INCREMENTAL_DIVERGENCE_GHIDRA_REPORT.md). Not a MapClass
   decoding gap; needs test-driven behavior comparison, not more RE.

3. ~~**DisplayClass.** MapClass ends at `+0x1174`. Everything
   player-visible lives in DisplayClass starting at that offset.~~
   → **Resolved:** [DISPLAYCLASS_DISCOVERY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DISPLAYCLASS_DISCOVERY_GHIDRA_REPORT.md) +
   [DISPLAYCLASS_BANDBOX_AND_MI_CORRECTION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/DISPLAYCLASS_BANDBOX_AND_MI_CORRECTION_GHIDRA_REPORT.md) document
   DisplayClass struct (50-slot vtable, single-inheritance confirmed,
   fields +0x1174..+0x11E0), BandBox state machine, and the MI
   correction (the adjacent `0x7E61E0` was `BufferStraw`'s vtable,
   not a secondary-inheritance fragment).

4. ~~**The bridge-repair-hut registry at `DAT_008B41A8`.**~~
   → **Resolved:** [MAPCLASS_COMPLETE_DECODE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_COMPLETE_DECODE.md) §A shows this is
   the "tags with destroyed events (bit 0x04)" DynVec, which happens
   to get iterated by `UnregisterBridgeRepairHut` because hut
   destruction fires triggers. The DynVec is generic (any
   destroyed-event tag), not hut-specific.

5. ~~**Vtable slots 18–21** (`0x4C9150` ×4).~~
   → **Resolved:** [MAPCLASS_COMPLETE_DECODE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_COMPLETE_DECODE.md) §C — these are
   `Stub__ReturnZero` (callable no-op returning 0), not `__purecall`.
   30+ `.rdata` vtable slots across the display-chain hierarchy
   share this stub as their default. Dispatching slots 18–21 on a
   MapClass instance is safe and returns 0.

---

## Sources

### Raw memory dumps
- `0x7ED400, 8 bytes` — COL + first vtable word (`0x004F4240`)
- `0x7ED404, 256 bytes` — the disputed "vtable" region
- `0x7EA6FC, 260 bytes` — GScreenClass vtable (for comparison)
- `0x008054C8, 32 bytes` — MapClass Complete Object Locator
- `0x00805530, 256 bytes` — adjacent RTTI structures

### Newly decompiled / re-read functions
- `0x485130` — DetachObjectFromCell (FUN_00485130)
- `0x485250` — AttachObjectToCell (FUN_00485250)
- `0x565090` — MapClass constructor (re-read to confirm field init scope)
- `0x565800` — Init_Alloc (re-read to confirm non-touch of +0x74/+0x11C)
- `0x5656D0` — IsCellExplored (re-read; corrected label)
- `0x5659F0` — Init_Clear (re-read to confirm +0x1158 write)
- `0x565BC0` — cell-array destructor walk (vtable 24; +0x134 writer)
- `0x56C510` — UpdateBridgeZonesHelper (re-read to confirm field access set)
- `0x577920` — UnregisterBridgeRepairHut (newly decompiled)
- `0x588BF0` — scalar deleting destructor (re-read)
- `0x004AD2D0` — scenario map-reader (calls AttachObjectToCell for CellTags)
- `0x00560BF0` — video-mode setup (confirmed NOT MapClass)

### Field-access scans (get_field_access_context)
- `+0x74/+0x78/+0x7C` via `0x87F85C/0x87F860/0x87F864` — zero hits
- `+0x11C/+0x120` via `0x87F904/0x87F908` — zero hits
- `+0x134` via `0x87F91C` — one write from `ScenarioClass::Full_Init`
- `+0x1158` via `0x00880940` — two reads from `CellClass::DrawOverlay_Body`
- `+0x115C` via `0x00880944` — reads from `FUN_00485130`, `FUN_00485250`

### Function search
- `search_functions("UpdateRamp")` → 16 results (all 16 variants listed)

### Cross-references
- `0x7ED404` — 3 xrefs, all DATA (constructor + GScreenClass ctor calls)
- `0x7ED480` — VectorClass vtable, used by WaveClass and 9 other classes
- `0x008A55E0` (slot 30's target) — RTTI COL for the adjacent vtable

### Doc files referenced
- `MAPCLASS_GHIDRA_REPORT.md` (2026-04-06)
- [MAPCLASS_GHIDRA_REPORT_FOLLOWUP.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/MAPCLASS_GHIDRA_REPORT_FOLLOWUP.md) (2026-04-22)
- `CELLCLASS_STRUCT_GHIDRA_REPORT.md` (for cell+0x12C ShroudFlags bit meanings)
- [SHROUD_SYSTEM_COMPLETE.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/SHROUD_SYSTEM_COMPLETE.md) (for explored-bit semantics)

### INI files checked
- `ini/rulesmd.ini` — `[CrateRules]`, `[General]`, `[SpecialFlags]`
- `ini/artmd.ini` — no MapClass-relevant keys
