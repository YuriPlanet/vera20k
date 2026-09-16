# CellClass+0x122 Dynamic Blocker Lifecycle Rust Mapping - Ghidra Research Report

**Address(es):** `0x00429A90`, `0x005FC570`, `0x00480630`, `0x00480CB0`, `0x004FCE80`, `0x00440580`, `0x00445880`, `0x004D7170`, `0x004DB260`, `0x004D85D0`, `0x0071C930`, `0x0071D000`, `0x004CE840`, `0x0047E8A0`, `0x0047EA90`
**Investigation Mode:** exhaustive-slice
**Claimed Scope:** Dynamic writer lifecycle of `CellClass+0x122` only as needed to build production Rust `BlockerNeighborCounts`: wall overlay placement/destruction, building limbo/unlimbo, foot limbo/unlimbo/per-cell movement, terrain object limbo/unlimbo, and aircraft descent writer caveats.
**Non-Scope:** Full A* hierarchy producer, retry edge lifetime, full `Can_Enter_Cell`, full bridge pathing, all aircraft landing behavior, all overlay classes, and Rust implementation patches.
**Confidence:** High for source categories, writer shape, global-vs-layer result, and Rust count handoff. Medium for exact future aircraft Rust integration because current flat movement does not use aircraft descent as a normal pathing caller.
**Active in YR:** Yes for wall/building/foot/terrain/object reader paths; Conditional for aircraft descent, active when aircraft are in the descent/landing path.

## 0. Required Investigation Setup

**Target question:** Which dynamic `CellClass+0x122` writers should feed Rust production `BlockerNeighborCounts`, and how should those sources be counted without over-pruning or missing lifecycle updates?

**Non-goals:** Do not re-investigate the entire hierarchy marker gate, retry producer, zone graph, `Can_Enter_Cell`, or all bridge occupancy semantics. Do not write Rust. Do not modify Ghidra.

**Evidence needed to mark COMPLETE:**

- Binary proof for the active A* read address and zero/nonzero polarity.
- Binary proof for each scoped writer class and its INC/DEC timing.
- Binary proof whether the count is global `CellClass` state or layer-specific.
- Rust surface scan showing where counts can be sourced today and what is missing.
- Explicit caveat for bridge/deck occupants and aircraft descent.

**Stop conditions:** Stop after count-source rules and dynamic update caveats are proven for the scoped lifecycle. Defer aircraft details beyond descent writer participation and defer Rust implementation.

## 1. Overview

`CellClass+0x122` is a byte count read by `AStar_main_loop` only as a hierarchy marker exception. A candidate cell outside the level-0 marker path is still expanded when its `+0x122` byte is nonzero; if the byte is zero and hierarchy mode is active, that candidate is skipped.

The count is not layer-specific. Writers from walls, buildings, foot objects, aircraft descent, and terrain objects all mutate the single `CellClass+0x122` byte. For Rust this means `BlockerNeighborCounts` must be a global per-cell source grid, not a selected movement-layer grid.

**Authored-load correction (2026-09-01):** a successful `ScenarioClass::Full_Init` keeps `ScenarioInit @ 0x00A8E7AC` nonzero while authored overlays run `OverlayClass::Mark`. That counter forces the wall build predicate true, so an authored `Wall=yes` object takes the successful wall path, increments the eight neighbor bytes immediately, and then takes the common queued-object tail. Later low procedural Overlay bodies may overwrite the wall identity without decrementing those bytes. Therefore final overlay identities cannot reconstruct the native authored contribution: Rust must retain the real-cell, wrapping-`u8` count plane produced during authored finalization and compose later runtime writers with it. Signed fixed-map neighbor lookups that alias a real slot remain observable; writes to the shared dummy have no fresh-game output and must not become a synthetic count cell.

## 2. Class Layout / Key Offsets

| Offset | Type | Meaning for this report | Evidence | Active in YR |
|---:|---|---|---|---|
| `CellClass+0x122` | `u8` | Global blocker-neighbor/expanded-blocker count used by hierarchical A* marker exception | reader `0x00429EB1`; writer sites below | Yes |
| `CellClass+0xE4` | object ptr | Ground object list, separate from `+0x122` | `CellClass::AddContent @ 0x0047E8A0` prior doc and spot check | Yes |
| `CellClass+0xE8` | object ptr | Bridge/deck object list, separate from `+0x122` | `CellClass::AddContent @ 0x0047E8A0` prior doc and spot check | Yes |
| `CellClass+0x124` | bitfield | Ground occupation bits, not the `+0x122` count | `BRIDGE_OCCUPANCY_OBJECT_LISTS_GHIDRA_REPORT.md` | Yes |
| `CellClass+0x128` | bitfield | Bridge occupation bits, not the `+0x122` count | `BRIDGE_OCCUPANCY_OBJECT_LISTS_GHIDRA_REPORT.md` | Yes |
| `ObjectClass+0x8C` | byte | `OnBridge` list selector for AddContent/RemoveContent; not used by `+0x122` writer selection | `CellClass::AddContent/RemoveContent` docs | Yes; conditional on bridge cells |

## 3. Core Logic

### 3.1 Reader

Active in YR: **Yes.** Decompile of `AStar_main_loop @ 0x00429A90` shows:

```text
if candidate zone is not marked:
    if normal/near-height branch:
        if byte[candidate_cell + 0x122] == 0 and hierarchy_flag != 0:
            skip candidate
```

Assembly evidence: `0x00429EB1..0x00429EC1` is `MOV CL, byte ptr [EBX+0x122]`, `TEST CL,CL`, `JNZ 0x00429EC7`, then `MOV CL,[ESP+0x74]`, `TEST CL,CL`, `JNZ 0x0042A1A1`. The byte is used as boolean zero/nonzero, not as a magnitude.

### 3.2 Wall overlays

Active in YR: **Yes.** `OverlayClass::Mark @ 0x005FC570` reaches the wall branch only when `OverlayTypeClass+0x2A8` is true. On successful wall placement it writes the cell overlay, calls `CellClass::PostDestructionWallCleanup(1)`, then increments `+0x122` for the 8 neighboring cells of the wall source cell.

Assembly evidence: `0x005FC762..0x005FC775` loads `[EAX+0x122]`, `INC DL`, stores it, and loops while the counter is less than `8`.

Destruction/removal decrements the same neighbor set:

- `CellClass::DestroyOverlay @ 0x00480CB0` is wall-gated by `OverlayTypeClass+0x2A8`; after clearing the wall overlay and updating four cardinal neighbors, it decrements all 8 neighbors of the destroyed wall cell. Evidence: decompile plus assembly `0x00481070..0x00481082` (`DEC DL`, loop `< 8`). Active in YR: **Yes**.
- `CellClass::PostDestructionWallCleanup @ 0x00480630` can auto-destroy isolated/damaged wall cells after recomputing connectivity. It clears the wall, runs Recalc at `0x00480969`, compares old/new zone at `0x0048096E..0x00480977`, and decrements all 8 neighbors at `0x004809DD..0x004809EF` **only when the zone changed**. The hardcoded GASAND/GAWALL/NAWALL rows are active retail; CYCL/BARB/FENC rows are dormant/mod-conditional because retail never sets their `Wall` flag.
- `HouseClass::Sell_Building_At_Cell @ 0x004FCE80` is a native exception. It clears the sold wall, Recalcs, and invokes one cleanup call, but a full 132-instruction scan contains no `+0x122` access. The sold anchor contribution remains stale; only a neighboring wall independently auto-removed by cleanup may decrement, subject to the changed-zone condition.

Rust rule: a runtime wall contributes while its live wall lifecycle owns the counter write, but removal is path-specific: direct destruction decrements unconditionally after its cardinal cleanup; cleanup auto-removal decrements only after a changed-zone Recalc; sale does not decrement the sold anchor. The authored-load baseline is the retained counter plane produced by ordered Mark effects. It is not equivalent to scanning final `Wall=yes` identities, because a later procedural body can replace the identity without reversing the earlier authored increment. Ore and non-wall overlays do not independently create wall contributions.

### 3.3 Buildings

Active in YR: **Yes.** `BuildingClass::Unlimbo @ 0x00440580` increments after successful `TechnoClass::Unlimbo`, not during placement probing. The writer is not "8 neighbors per foundation cell." It computes `BuildingTypeClass::GetFoundationWidth()` and `GetFoundationHeight(0)`, converts the building coordinate to a cell anchor, then loops a rectangle from anchor - 1 through anchor + width and anchor - 1 through anchor + height. That is `(width + 2) * (height + 2)` cells, incremented once per cell.

Decompile evidence: `0x00440580` block after `TechnoClass__Unlimbo` calls width/height, sets `sStack_38 = x_cell`, `sStack_36 = y_cell`, loops `iVar7 < height+2` and `iVar15 < width+2`, calls `MapClass__Get_CellClass`, and increments `*(char *)(cell+0x122)`.

Assembly evidence: `0x00440C9F..0x00440D01`; load at `0x00440CD9`, `INC DL` at `0x00440CDF`, store at `0x00440CE4`, inner compare against width in `EBX`, outer compare at `0x00440CF9`.

`BuildingClass::Limbo @ 0x00445880` decrements the same expanded rectangle on removal for ordinary buildings. Evidence: decompile block gated by `*(int *)(this->Type + 0xE58) == 0`, then same `(width+2)*(height+2)` loop; assembly `0x00445CCF..0x00445D33` with `DEC DL` at `0x00445D17`. Active in YR: **Yes**, conditional on the ordinary-building branch that executes the decrement; this is the normal placed-building removal path.

Rust rule: structure contribution should be an expanded rectangle around the binary foundation once, not a sum of 8-neighbor contributions for every movement-blocking foundation cell. Do not include bib-only movement exceptions unless a future proof shows the binary building `+0x122` loop includes them; this pass found width/height only.

### 3.4 Foot objects: unlimbo, limbo, and per-cell movement

Active in YR: **Yes.** `FootClass::Unlimbo @ 0x004D7170` increments all 8 neighbors of the foot object's cell after successful `TechnoClass::Unlimbo`. Evidence: decompile at `0x004D7170`; assembly `0x004D729A..0x004D72AC` (`INC DL`, loop `< 8`).

Active in YR: **Yes.** `FootClass::Limbo @ 0x004DB260` first checks the object is not already limboed (`object+0x81 == 0`) and has a type, then decrements all 8 neighbors of the last/source cell stored at `FootClass+0x55C`. Evidence: decompile plus assembly `0x004DB2D7..0x004DB2E9`.

Active in YR: **Yes.** `FootClass::PerCellProcess @ 0x004D85D0` performs the moving lifecycle update only when the stored last cell differs from the current cell. It decrements the old stored-cell 8-neighbor contribution, writes the current cell into `FootClass+0x55C`, then increments the new 8-neighbor contribution. Evidence: decompile plus assembly `0x004D86D8..0x004D86EA` for DEC and `0x004D8745..0x004D8757` for INC.

Rust rule: counts must update at cell-crossing granularity, not only spawn/despawn. A production builder can recompute each search from current entity cells, but an incremental cache must hook unlimbo, limbo, and cell-change events.

### 3.5 Terrain objects

Active in YR: **Yes.** `TerrainClass::Unlimbo @ 0x0071D000` increments all 8 neighbors of the terrain object's cell after successful `ObjectClass::Reveal`. Evidence: decompile plus assembly `0x0071D085..0x0071D097`.

Active in YR: **Yes.** `TerrainClass::Limbo @ 0x0071C930` decrements all 8 neighbors when the terrain object is not already limboed, then clears source-cell occupation bit `0x40` from `CellClass+0x124`. Evidence: decompile plus assembly `0x0071C9A6..0x0071C9B8`.

Rust rule: include live blocking terrain objects as sources. `ResolvedTerrainCell.terrain_object_blocks` covers map-load/static source cells, but runtime terrain object removal needs to keep this count source current or force a fresh count rebuild from live terrain-object state.

### 3.6 Aircraft descent

Active in YR: **Conditional.** `FlyLocomotionClass::Descent_Step @ 0x004CE840` writes `+0x122` only in descent/landing state. At touchdown, if the stored last cell is zero/null it only increments the current cell's 8-neighbor contribution; otherwise it decrements the old stored cell, writes the current cell, and increments the new one. Evidence: decompile at `0x004CE840`; assembly `0x004CEDA4..0x004CEDB6` DEC, `0x004CEE18..0x004CEE2A` first INC path, `0x004CEE8B..0x004CEE9D` second INC path.

Rust rule: for current production flat ground hierarchy, aircraft can be a deferred count source only if aircraft landing/descent pathfinding is not enabled through the same flat A* gate. If implemented, include landing/descent aircraft as live foot-like source cells.

### 3.7 Global vs layer-specific, including bridge occupants

Active in YR: **Yes / Conditional on bridge occupancy for the bridge example.** Every writer above writes `byte [CellClass+0x122]`. None chooses `CellClass+0xE4` vs `+0xE8`, `+0x124` vs `+0x128`, or reads `ObjectClass+0x8C` before choosing a counter. Bridge object lists are separate for occupancy/cell-entry, but this counter is not.

Evidence: writer assembly addresses all target `[EAX+0x122]` or equivalent `CellClass+0x122`; `CellClass::AddContent @ 0x0047E8A0` and `RemoveContent @ 0x0047EA90` layer-select object lists, but they are not the counter writers. Foot/deck units still execute FootClass unlimbo/movement lifecycle and mutate the global counter using their 2D current/last cells.

Rust rule: bridge/deck occupants affect the flat hierarchy marker exception. A Rust builder must not use only `MovementLayer::Ground` or only flat hard blocks. It should include live foot/entity source cells from both ground and bridge object-list layers when flat hierarchy is active.

## 4. INI Keys

No INI key directly controls `CellClass+0x122`. INI data matters only by creating source object classes:

| INI/key surface | Use for this report | Active in YR |
|---|---|---|
| OverlayType `Wall=yes` equivalent loaded into `OverlayTypeClass+0x2A8` | Gates wall overlay placement/destruction counter writes | Yes |
| Building foundation dimensions | Building counter rectangle uses binary type foundation width/height | Yes |
| TerrainTypes map/scenario entries | Create TerrainClass objects that run terrain unlimbo/limbo writers | Yes |
| Aircraft type/descent state | Conditional source only during landing/descent | Conditional |

Do not source counts from `MovementZone`, `TooBigToFitUnderBridge`, ore density, water, cliff, or base passability INI values unless they produce one of the live object/overlay writers above.

## 5. Integration Points

| Integration | Verified behavior | Evidence | Active in YR |
|---|---|---|---|
| A* read | Off-marker candidate accepted if `+0x122 != 0`; zero is skipped only in hierarchy mode | `AStar_main_loop 0x00429EB1..0x00429EC1` | Yes |
| Wall placement | Wall overlay source increments 8 neighbor cells after successful wall mark | `OverlayClass::Mark 0x005FC570`; assembly `0x005FC762..0x005FC775` | Yes |
| Wall destruction | Destroyed/auto-destroyed wall source decrements 8 neighbor cells | `0x00480CB0`, `0x00480630`; assembly above | Yes |
| Building placement | Placed building increments expanded foundation rectangle once per cell | `BuildingClass::Unlimbo 0x00440580`; assembly `0x00440C9F..0x00440D01` | Yes |
| Building removal | Limbo decrements expanded foundation rectangle on ordinary removal | `BuildingClass::Limbo 0x00445880`; assembly `0x00445CCF..0x00445D33` | Yes |
| Foot movement | Cell-change decrements old 8-neighbor contribution and increments new one | `FootClass::PerCellProcess 0x004D85D0`; assembly `0x004D86D8`, `0x004D8745` | Yes |
| Terrain object lifecycle | Terrain unlimbo/limbo increment/decrement 8 neighbors | `0x0071D000`, `0x0071C930` | Yes |
| Aircraft descent | Landing/descent updates old/new 8-neighbor source | `FlyLocomotionClass::Descent_Step 0x004CE840` | Conditional |

## 6. Current Rust Implementation Status

Current Rust has the read-side shape, a production count builder, and the retained authored-wall
authority infrastructure:

- `src/sim/pathfinding/core.rs:170-219`: `BlockerNeighborCounts` stores per-cell `u8` and `HierarchyGate::allows` accepts marked zones or `count_at(x,y) != 0`.
- `src/sim/pathfinding/core.rs:1950-1988`: `find_path_with_costs_hierarchy_marker` can receive counts.
- `src/sim/pathfinding/zone_search.rs:276-303`: hierarchy-marker path only activates when `blocker_neighbor_counts` is `Some`.
- `src/sim/movement/movement_path.rs:458-462,533-545`: production movement passes `ctx.blocker_neighbor_counts` into zoned marker search.
- `src/sim/movement/bump_crush.rs`: `build_blocker_neighbor_counts_with_overlays` seeds from
  `OverlayGrid::retained_wall_neighbor_counts()` when it is `Some`, then composes terrain objects,
  entities, and expanded buildings. Only temporary legacy `None` constructors fall back to a final
  live-wall scan.
- `src/map/authored_overlay.rs` and `src/sim/overlay_grid.rs`: ordered authored Mark effects now carry
  a wrapping real-cell wall plane through the non-Clone finalized payload. Runtime placement, direct
  destruction, changed-zone cleanup removal, and sale update or deliberately retain that same plane
  with their path-specific native rules, including represented fixed-stride aliases.
- `src/sim/snapshot.rs` and `src/sim/world/world_hash.rs`: retained storage is shape-validated,
  serialized, and hashed with `None` distinct from `Some(all-zero)`.
- `src/map/resolved_terrain.rs:74-117`, `409-486`, `543-545`: `ResolvedTerrainCell` exposes `terrain_object_blocks` and `overlay_blocks`, the closest static source for terrain-object and wall/blocking-overlay contributions.

Current delta: the payload and runtime consumer are implemented, but the production/headless authored
reader still constructs legacy `None` grids and therefore never supplies the retained plane. G7/G13
remain open until every gameplay-equivalent builder consumes the finalized payload and current-version
save/restore rejects `None` (or advances the version if a `None` v114 save escapes).

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| A* reader polarity | verified | decompile `0x00429A90`; assembly `0x00429EB1..0x00429EC1` | none |
| Wall overlay placement | verified | decompile `0x005FC570`; assembly `0x005FC762..0x005FC775` | none |
| Wall overlay destruction/cleanup | verified | decompile `0x00480CB0`, `0x00480630`; assembly `0x00481070`, `0x004809DD` | none |
| Building unlimbo placement | verified | decompile `0x00440580`; assembly `0x00440CD9` with rectangle loop | none |
| Building limbo removal | verified | decompile `0x00445880`; assembly `0x00445D11` with rectangle loop | exact semantic name of `BuildingType+0xE58` is not needed for Rust source rule |
| Foot unlimbo/limbo | verified | decompile `0x004D7170`, `0x004DB260`; assembly `0x004D729A`, `0x004DB2D7` | none |
| Foot per-cell movement | verified | decompile `0x004D85D0`; assembly `0x004D86D8`, `0x004D8745` | none |
| Terrain object unlimbo/limbo | verified | decompile `0x0071D000`, `0x0071C930`; assembly `0x0071D085`, `0x0071C9A6` | none |
| Aircraft descent writers | verified for count participation | decompile `0x004CE840`; assembly `0x004CEDA4`, `0x004CEE18`, `0x004CEE8B` | exact Rust aircraft search integration can be deferred |
| Global vs layer-specific | verified | all writer assembly writes `CellClass+0x122`; object-list layer docs for `0x0047E8A0/0x0047EA90` | none |
| Current Rust count producer | infrastructure complete / production disconnected | finalized payload, `OverlayGrid` retained plane, runtime deltas, and `build_blocker_neighbor_counts_with_overlays` | replace production/headless legacy constructors with the consumed payload; then close the no-`None` persistence gate |

## 8. Open Questions - Final State of the Investigation Log

- `[RESOLVED] OQ-001 - Is the read-side gate active in standard YR? -> Yes; it is in AStar_main_loop and guarded only by the hierarchy flag.` (evidence: `0x00429EB1..0x00429EC1`)
- `[RESOLVED] OQ-002 - Are wall overlays count sources? -> Yes, only wall-gated overlay lifecycle writes the counter.` (evidence: `0x005FC570`, `0x00480CB0`, `OverlayTypeClass+0x2A8`)
- `[RESOLVED] OQ-003 - Are ore overlays count sources? -> No; the observed overlay count writer is wall-gated, while ore reports show ore placement does not touch `+0x122`.` (evidence: `0x005FC570`; [CELLCLASS_REDUCE_TIBERIUM_FUN_00480A80_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_REDUCE_TIBERIUM_FUN_00480A80_GHIDRA_REPORT.md))
- `[RESOLVED] OQ-004 - Are building counts 8-neighbor-per-footprint-cell? -> No; building placement/removal writes one expanded `(width+2)*(height+2)` rectangle.` (evidence: `0x00440580`, `0x00445880`, assembly `0x00440CD9`, `0x00445D11`)
- `[RESOLVED] OQ-005 - Do foot units update counts on movement? -> Yes; per-cell process decrements old cell neighbors, stores current cell, then increments new cell neighbors.` (evidence: `0x004D85D0`, assembly `0x004D86D8`, `0x004D8745`)
- `[RESOLVED] OQ-006 - Do limbo/unlimbo update foot counts? -> Yes; successful unlimbo increments and non-limboed limbo decrements.` (evidence: `0x004D7170`, `0x004DB260`)
- `[RESOLVED] OQ-007 - Do terrain objects update counts? -> Yes; TerrainClass unlimbo/limbo increments/decrements 8 neighbors.` (evidence: `0x0071D000`, `0x0071C930`)
- `[RESOLVED] OQ-008 - Do plain water/cliffs/base unwalkable terrain write counts? -> No writer in this slice; verified writers are object/overlay/terrain-object lifecycles.` (evidence: writer xref set in prior report plus current spot checks)
- `[RESOLVED] OQ-009 - Is the counter layer-specific? -> No; all writers touch the single `CellClass+0x122` byte.` (evidence: writer assembly; object-list docs)
- `[RESOLVED] OQ-010 - Do bridge occupants affect the flat marker exception? -> Yes when they are live foot/object sources; bridge list selection is separate from `+0x122` writes.` (evidence: FootClass writers plus `BRIDGE_OCCUPANCY_OBJECT_LISTS_GHIDRA_REPORT.md`)
- `[RESOLVED] OQ-011 - Does numeric magnitude matter to A*? -> No; A* tests zero/nonzero only.` (evidence: `0x00429EB1..0x00429EC1`)
- `[RESOLVED] OQ-012 - Does the binary saturate the byte? -> No clamp was observed; writer assembly uses raw `INC DL` / `DEC DL`.` (evidence: all writer assembly contexts)
- `[RESOLVED] OQ-013 - Is an absent optional Rust count surface equivalent to all-zero? -> No for production parity; hierarchy reads the supplied global plane, and production now constructs/passes one.` (evidence: `src/sim/pathfinding/zone_search.rs`; production callers of `build_blocker_neighbor_counts_with_overlays`)
- `[RESOLVED] OQ-014 - Can current Rust derive counts from `LayeredEntityBlockMap` alone? -> No; the production builder correctly composes separate terrain, expanded-building, foot, and wall inputs. Its remaining authored defect is the final-wall reconstruction source, not the separate-source architecture.` (evidence: Rust scan)
- `[DEFERRED] OQ-015 - Exact future aircraft count lifecycle in Rust aircraft pathing` (category: `out-of-scope`; reason: this report only needs aircraft participation caveat for production ground `BlockerNeighborCounts`; next-step-if-pursued: trace aircraft landing path callers against Rust aircraft movement once hierarchy is enabled there)

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| `CellClass+0x122` is global and read as zero/nonzero by hierarchical A*, not as a selected-layer value. Active in YR: Yes. | `0x00429EB1..0x00429EC1`; writer assembly to `[CellClass+0x122]` | The consumed plane and `Some` baseline exist, but production/headless still instantiate legacy `None` | `FinalizedOverlayPayload`, `OverlayGrid`, `src/sim/pathfinding/core.rs`, `src/sim/movement/bump_crush.rs` and callers | Move the one finalized payload into every gameplay builder; reject `None` at the current snapshot boundary after migration and never reconstruct from a snapshot | Off-marker candidate adjacent to an authored fixed-stride alias is allowed by count even after the source wall identity is overwritten; identical candidate with no adjacent source is pruned | Do not build from only `MovementLayer::Ground`, only hard blocks, or only final overlay identities |
| Buildings write one expanded foundation rectangle on unlimbo/limbo. Active in YR: Yes. | `BuildingClass::Unlimbo 0x00440580`; `BuildingClass::Limbo 0x00445880`; assembly `0x00440CD9`, `0x00445D11` | Current builder already calls `add_building_expanded_foundation`; preserve it while changing the authored baseline | `src/sim/movement/bump_crush.rs`, building/rules foundation helpers | Keep each live structure's one count source over anchor-expanded rectangle `(x-1..x+width, y-1..y+height)` in bounds | A 2x2 building increments the 4x4 surrounding rectangle once per cell, not double-counted at overlapping neighbor cells | Do not regress to summing eight neighbors per occupied foundation cell while composing the new authored baseline |
| Walls, terrain objects, and foot objects use 8-neighbor lifecycle writes; authored wall writes survive a later identity overwrite; foot movement updates old and new cells. Active in YR: Yes. | Wall `0x005FC762`, terrain `0x0071D085/0x0071C9A6`, foot `0x004D729A/0x004DB2D7/0x004D86D8/0x004D8745`; authored chronology report | Wall retained/runtime infrastructure now preserves ordered writes and real aliases; only production authored delivery is disconnected | `FinalizedOverlayPayload`, `src/sim/overlay_grid.rs`, `src/sim/movement/bump_crush.rs`, `terrain_object_blocks`, occupancy/entity iteration | Preserve the implemented plane and runtime path-specific deltas while wiring the consumed payload; never reconstruct the authored baseline from final identities | A later authored low-body overwrite removes the final wall identity but retains its eight real-cell neighbor increments; direct/cleanup/sale paths retain their distinct native count tails | Do not include ore, water, cliff-only, generic `!walkable` cells, or a second scan of final authored walls |

## 10. Negative Facts / Do Not Do

- Do not build counts from every unwalkable terrain cell. Active in YR: Yes for the negative; no terrain/cliff/water writer was found, while verified writers are lifecycle object sources.
- Do not derive building contribution by taking every current building-blocked cell and adding all 8 neighbors; binary writes an expanded rectangle once per cell. Active in YR: Yes; evidence `0x00440580`, `0x00445880`.
- Do not keep `BlockerNeighborCounts` selected-layer-only. Active in YR: Yes; evidence all writers target global `CellClass+0x122`.
- Do not ignore bridge/deck occupants when building flat hierarchy counts. Active in YR: Conditional on bridge occupants; foot/object lifecycle still writes the global byte.
- Do not document `+0x122` as fog, shroud, ore-neighbor, bridge state, or `Can_Enter_Cell` passability. Active in YR: Yes; reader is A* and writers are object lifecycles.
- Do not rebuild the authored wall contribution from final `Wall=yes` identities. Active in YR: Yes for the negative; ordered authored Mark writes can survive a later low-body identity overwrite, and signed fixed-stride lookups can update an aliased real cell.

## 11. Remaining Uncertainty

- Aircraft descent should be included only when Rust aircraft landing/descent uses the same hierarchy search surface. Binary participation is verified, but current Rust ground movement does not need aircraft-specific activation to produce correct normal ground `BlockerNeighborCounts`.
- `BuildingType+0xE58` gates the ordinary `BuildingClass::Limbo` decrement block in the decompile. The exact field name is not needed for the normal production-building source rule, but future special building-to-overlay cases should avoid assuming every limbo path decrements the structure rectangle.
- Production/headless still must consume the implemented load-produced plane once. Until that cutover
  and the no-`None` snapshot gate close, focused infrastructure tests cannot certify live-map parity.

## 12. Proposed Rust Test Names

- `blocker_neighbor_counts_include_bridge_layer_occupants_globally`
- `blocker_neighbor_counts_building_uses_expanded_foundation_rectangle_once`
- `blocker_neighbor_counts_wall_destroy_removes_off_marker_exception`
- `blocker_neighbor_counts_include_static_wall_and_terrain_objects_not_plain_unwalkable`
- `blocker_neighbor_counts_unit_cell_move_transfers_neighbor_exception`
- `authored_wall_overwrite_retains_blocker_neighbor_counts`
- `authored_wall_fixed_stride_alias_updates_real_count_not_dummy_output`

## 13. Stale Docs / Follow-up Docs

Replace the over-broad statement in [CELL_0X122_WRITER_TIMING_FLAT_ASTAR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/CELL_0X122_WRITER_TIMING_FLAT_ASTAR_GHIDRA_REPORT.md) that says all semantic writer sites share the same 8-neighbor source pattern with:

> Most single-cell object sources for `CellClass+0x122` use an 8-neighbor INC/DEC lifecycle (walls, foot objects, terrain objects, and aircraft descent). Buildings are the important exception: `BuildingClass::Unlimbo/Limbo` writes a single expanded foundation rectangle `(width+2)*(height+2)`, once per cell, rather than adding 8-neighbor contributions for every foundation cell.

Replace any Rust handoff wording that says "increment all 8 in-bounds neighbors of each verified source blocker cell" with:

> Build counts from verified source shapes: 8-neighbor contributions for single-cell walls/foot objects/terrain objects/landing aircraft, and one expanded foundation rectangle for buildings. The resulting count is global per `CellClass`, not movement-layer scoped.

Older docs that call `+0x122` fog/shroud, ore-neighbor, or TS legacy should be superseded by this wording:

> `CellClass+0x122` is an active YR global blocker-neighbor/expanded-blocker count read by hierarchical A* as an off-marker exception. It is not fog/shroud, not ore, and not bridge passability state.

## Sources

- Ghidra decompiled/read-only: `0x00429A90`, `0x005FC570`, `0x00480630`, `0x00480CB0`, `0x00440580`, `0x00445880`, `0x004D7170`, `0x004DB260`, `0x004D85D0`, `0x0071C930`, `0x0071D000`, `0x004CE840`, `0x0047E8A0`, `0x0047EA90`.
- Ghidra assembly contexts: `0x00429EB1`, `0x005FC762`, `0x004809DD`, `0x00481070`, `0x00440CD9`, `0x00445D11`, `0x004D729A`, `0x004DB2D7`, `0x004D86D8`, `0x004D8745`, `0x004CEDA4`, `0x004CEE18`, `0x004CEE8B`, `0x0071C9A6`, `0x0071D085`.
- Authored wall chronology, ScenarioInit reachability, compact retail IDs, active-winner census, and fixed-map counter aliases: `docs/research/bridges/01-assets-map-load-overlay/AUTHORED_OVERLAY_WALL_SCENARIOINIT_ACCEPTANCE_REINVESTIGATION_GHIDRA_REPORT.md`.
- Referenced docs: [CELL_0X122_WRITER_TIMING_FLAT_ASTAR_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/CELL_0X122_WRITER_TIMING_FLAT_ASTAR_GHIDRA_REPORT.md), [CELL_0x122_CAN_ENTER_CELL_SEMANTIC_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/CELL_0x122_CAN_ENTER_CELL_SEMANTIC_GHIDRA_REPORT.md), `BRIDGE_OCCUPANCY_OBJECT_LISTS_GHIDRA_REPORT.md`, [CELLCLASS_REDUCE_TIBERIUM_FUN_00480A80_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_REDUCE_TIBERIUM_FUN_00480A80_GHIDRA_REPORT.md), [TIBTRE_TERRAIN_OBJECT_LIFECYCLE_AND_SEEDING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TIBTRE_TERRAIN_OBJECT_LIFECYCLE_AND_SEEDING_GHIDRA_REPORT.md).
- Rust scan: `src/sim/pathfinding/core.rs`, `src/sim/pathfinding/zone_search.rs`, `src/sim/movement/movement_path.rs`, `src/sim/movement/bump_crush.rs`, `src/map/resolved_terrain.rs`.

## Status

**COMPLETE** for `CELL_0X122_DYNAMIC_BLOCKER_LIFECYCLE_RUST_MAPPING`.
