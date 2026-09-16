# Cell Flags 0x500 TIBTRE Placement Semantics - Ghidra Research Report

**Address(es):** `0x004838E0`, `0x0047E040`, `0x0047E470`, `0x005FC570`, `0x0047C620`, `0x00429830`, `0x0042ACF0`  
**Investigation Mode:** exhaustive-slice  
**Claimed Scope:** exact runtime meaning, writers, and Rust mapping for `CellClass+0x140 & 0x500` as used by `CellClass::CanPlaceTiberium` on the TIBTRE placement path.  
**Non-Scope:** full bridge damage visuals, full bridge movement legality, low-bridge TubeClass behavior, `AllowTiberium`, building exception bytes, and TIBTRE timing/type/density side effects.  
**Confidence:** High for the mask, bit meanings, writer family, active YR reachability, and current Rust mapping.  
**Active in YR:** Yes. TIBTRE reaches `CellClass::CanPlaceTiberium` through `TerrainClass::AI -> SpreadTiberium(force=1)`, and bridge overlay stamping/damage paths are live YR map/runtime paths.

## 0. Investigation Contract

Target question: What exact runtime semantics and writers correspond to `CellClass+0x140` bits masked by `0x500` in `CanPlaceTiberium`, especially bridge/rail/structural cells, and how should that map to current Rust bridge facts for TIBTRE placement validation?

Non-goals: Do not reinvestigate TIBTRE animation timing, ore type selection, density placement effects, `AllowTiberium`, building exception bytes, full bridge traversal, or full bridge damage presentation.

Evidence needed to mark COMPLETE: decompile/assembly proof of the `CanPlaceTiberium` mask; writer proof for both bits in the mask; caller/xref proof that the writers are live in YR; negative proof that `0x400` is not the A* `0x40000` cost flag; current Rust source refs for equivalent stored facts; testable handoff.

Stop conditions: Stop after `0x100` and `0x400` are mapped to verified bridge-state semantics and Rust facts, with no Rust/INI/in-repo-doc/Ghidra mutation.

## 1. Overview

`CanPlaceTiberium` rejects a candidate cell when `(CellClass+0x140 & 0x500) != 0`. The two bits are `0x100` and `0x400`; this report found no evidence that the mask is a railroad-land-type gate. It is a bridge-state gate: `0x100` marks structural bridge cells, while `0x400` marks destroyed/inactive bridge cells written by `SetBridgeDirection_*` when the state argument is zero.

For TIBTRE, the player-visible effect is simple: ore from trees cannot appear on intact structural bridge cells or on destroyed/inactive bridge marker cells, even if Rust's `PathGrid::is_walkable` would allow walking on the underlying ground after bridge destruction.

## 2. Class Layout / Key Offsets

| Owner | Offset / bit | Meaning in this slice | Evidence | Active in YR |
|---|---:|---|---|---|
| `CellClass` | `+0x140 & 0x100` | Structural bridge/on-bridge cell flag | `CanPlaceTiberium @ 0x004838E0`; `SetBridgeDirection_* @ 0x0047E040/0x0047E470`; `TechnoClass__IsOnBridge_ForFiring @ 0x00703B10`; `PathfinderClass__UpdateBridgePassability @ 0x0042ACF0` | Yes |
| `CellClass` | `+0x140 & 0x400` | Destroyed/inactive bridge marker / placement blocker | `CanPlaceTiberium @ 0x004838E0`; `SetBridgeDirection_* @ 0x0047E040/0x0047E470`; placement reader `0x0047C620` | Yes |
| `CellClass` | `+0x140 & 0x500` | Combined TIBTRE placement rejection mask for `0x100 | 0x400` | assembly `0x004838FC..0x00483905` loads flags and tests `AH,0x5` | Yes |
| `CellClass` | `+0x140 & 0x40000` | Separate temporary A* bridge-approach cost marker, not part of this mask | `AStar_compute_edge_cost @ 0x00429830`; `PathfinderClass__UpdateBridgePassability @ 0x0042ACF0` | Yes, but unrelated |
| `CellClass` | `+0x11E` | Bridge state/data byte written alongside bridge flags | `SetBridgeDirection_* @ 0x0047E040/0x0047E470`; prior bridge reports | Yes |
| `CellClass` | `+0x2C` | Bridge anchor pointer for related stamped cells | `SetBridgeDirection_* @ 0x0047E040/0x0047E470` | Yes |

## 3. Core Logic

`CellClass::CanPlaceTiberium @ 0x004838E0` first checks playfield bounds, then immediately rejects bridge-flagged candidates. The assembly at `0x004838FC..0x00483905` loads `CellClass+0x140`, executes `TEST AH,0x5`, and jumps to failure when nonzero. Because `AH` is bits 8..15 of the dword, `0x5` in `AH` is the dword mask `0x100 | 0x400 = 0x500`.

The same function continues to later gates only if that mask is clear: active-game building scan, `SpawnsTiberium` terrain-object scan, land-type Buildable table, no existing overlay, flat slope, and tile `AllowTiberium`. Therefore the bridge mask is an early hard reject for TIBTRE candidate cells, not a fallback after pathing or overlay checks.

`CellClass::SetBridgeDirection_NESW @ 0x0047E040` and `CellClass::SetBridgeDirection_NWSE @ 0x0047E470` are byte-identical writer families. Both derive:

| Derived value | Formula in decompile | Meaning for this slice |
|---|---|---|
| `0x100` | `(param_3 & 1) << 8` | Structural bridge flag when state is intact/nonzero |
| `0x400` | `(param_3 == 0) << 10` | Destroyed/inactive marker when state argument is zero |
| `0x200`, `0x1000`, `0x10000` | other state-derived shifts | Not part of `CanPlaceTiberium` mask |
| `0x800` | `(param_2 == 0) << 11` | Direction/orientation bit, not part of `CanPlaceTiberium` mask |

Normal overlay marking calls these helpers with `state=1`. `OverlayClass::Mark @ 0x005FC570` dispatches bridge overlay IDs `0x18`, `0x19`, `0xED`, and `0xEE` into the two SetBridgeDirection helpers with `state=1`; xrefs also show runtime bridge update/damage/resize callers. With `state=1`, stamped anchor, forward, and opposite slots receive `0x100` where appropriate and clear `0x400`. With `state=0`, the anchor/forward/opposite slots clear intact bridge bits and set `0x400`.

The scope-relevant negative boundary is A*: `AStar_compute_edge_cost @ 0x00429830` multiplies cost when destination `CellClass+0x140 & 0x40000` is set; it does not use `0x400`. `PathfinderClass::UpdateBridgePassability @ 0x0042ACF0` reads `0x100` for bridge-layer object-list choice and toggles `0x40000` around searches. That is separate from the TIBTRE `0x500` reject.

## 4. INI Keys

No direct INI key writes `CellClass+0x140 & 0x100` or `0x400`. The active data path is map overlay content plus runtime bridge state:

| Data source | Binary effect | Evidence | Active in YR |
|---|---|---|---|
| Map overlay IDs `0x18`, `0x19`, `0xED`, `0xEE` | `OverlayClass::Mark` calls `SetBridgeDirection_*` with `state=1` | `0x005FC5FE`, `0x005FC60A`, `0x005FC62C`; xrefs to `0x0047E040/0x0047E470` | Yes |
| Overlay data / bridge runtime state | Supplies bridge state byte and later runtime state updates | `SetBridgeDirection_*`; prior [BRIDGE_SETBRIDGEDIRECTION_STAMPING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/BRIDGE_SETBRIDGEDIRECTION_STAMPING_GHIDRA_REPORT.md) | Yes |
| Railroad TMP/land data | Not the source of this mask | no writer/reader evidence tying railroad land type to `0x500`; passability Rust maps railroad separately | No for this mask |

## 5. Integration Points

| Point | Evidence | Result |
|---|---|---|
| TIBTRE target validation | prior TIBTRE reports; `CellClass::CanPlaceTiberium @ 0x004838E0` | Every candidate cell rejects immediately when `0x100` or `0x400` is set |
| Writer family | `SetBridgeDirection_NESW @ 0x0047E040`, `SetBridgeDirection_NWSE @ 0x0047E470` | Both bits are produced by bridge state stamping, not by ore code |
| Map-load writer caller | `OverlayClass::Mark @ 0x005FC570`; calls at `0x005FC5FE`, `0x005FC60A`, `0x005FC62C` | Bridge overlays stamp intact structural bridge cells with `0x100` and clear `0x400` |
| Runtime caller inventory | xrefs to `0x0047E040`: `0x005724CD`, `0x0057286D`, `0x00572E31`, `0x00573201`, `0x0057671C`, `0x00567078`, `0x00577790`, `0x005778AC`, `0x005FC5FE`, `0x005FC60A`; xrefs to `0x0047E470`: `0x0056EFDD`, `0x0056F37D`, `0x0056F941`, `0x0056FD11`, `0x00570FFC`, `0x0056706C`, `0x005721B3`, `0x005FC62C` | Writers are used by map load, bridge update/damage/repair/resize paths |
| Placement sibling reader | `Cell_passability_building_placement @ 0x0047C620`, assembly `0x0047C984`, `0x0047C9EA` | Building placement also rejects `0x100`/`0x400`, supporting placement-blocker semantics |
| A* negative boundary | `AStar_compute_edge_cost @ 0x00429830`; `PathfinderClass__UpdateBridgePassability @ 0x0042ACF0` | `0x400` is not the A* bridge-approach marker; `0x40000` is |

## 6. Current Rust Implementation Status

Current Rust already stores a close map of these bridge bits:

| Rust surface | Current status | Source refs |
|---|---|---|
| `BridgeCellFacts.raw_flags` | Stores raw bridge flag bits | `src/map/bridge_facts.rs:52..60` |
| `BRIDGE_FLAG_STRUCTURAL` | Defined as `0x100`, matching binary `0x100` | `src/map/bridge_facts.rs:3..9` |
| `BRIDGE_FLAG_DESTROYED_OR_RAMP` | Defined as `0x400`, matching binary `0x400` behavior-derived marker | `src/map/bridge_facts.rs:3..9` |
| `stamp_intact` | Clears `0x400` and sets `0x100` on anchor/forward/opposite structural cells | `src/map/bridge_facts.rs:128..177` |
| `stamp_destroy` | Clears structural bridge flags and sets `0x400` on anchor/forward/opposite slots | `src/map/bridge_facts.rs:179..210` |
| `ResolvedTerrainCell::bridge_flags()` | Exposes raw bridge flags from resolved terrain | `src/map/resolved_terrain.rs:196..198` |
| Resolved terrain bridge stamping | Stamps facts from map overlays and copies overlay data byte | `src/map/resolved_terrain.rs:573..623` |
| `PathCell` | Carries `bridge_structural` but not the full raw `0x400` marker | `src/sim/pathfinding/core.rs:1110..1152`, `1470..1532` |
| `terrain_spawn.rs::can_accept_tiberium` | Does not receive `ResolvedTerrainCell`/bridge facts and only checks `PathGrid::is_walkable`, spawner map, and resource type | `src/sim/terrain_spawn.rs:156..187` |

Rust-facing delta: `terrain_spawn.rs` cannot currently reproduce the `0x500` gate because `PathGrid::is_walkable` is the wrong abstraction. It may reject some intact bridge cells indirectly, but it cannot reliably reject all cells with `BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DESTROYED_OR_RAMP`, especially destroyed bridge marker cells that may fall back to underlying ground walkability.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `CanPlaceTiberium` `0x500` mask | verified | decompile `0x004838E0`; assembly `0x004838FC..0x00483905` | none |
| `0x100` structural bridge semantics | verified | `SetBridgeDirection_*`; `TechnoClass__IsOnBridge_ForFiring @ 0x00703B10`; `PathfinderClass__UpdateBridgePassability @ 0x0042ACF0`; prior bridge reports | exact original symbol name remains unknown |
| `0x400` destroyed/inactive marker semantics | verified | `SetBridgeDirection_*`; placement reader `0x0047C620`; prior global xref census | exact original symbol name remains unknown |
| `SetBridgeDirection_NESW` writer | verified | decompile `0x0047E040`; assembly store sites `0x0047E0F0`, `0x0047E1CB`, `0x0047E295`, `0x0047E3CC`, `0x0047E452` | none for this mask |
| `SetBridgeDirection_NWSE` writer | verified | decompile `0x0047E470`; byte-identical body | none for this mask |
| Map-load bridge overlay caller | verified | `OverlayClass::Mark @ 0x005FC570`; callsites `0x005FC5FE`, `0x005FC60A`, `0x005FC62C`; xrefs | none for this mask |
| Runtime bridge caller inventory | touched-not-exhausted | bulk xrefs to `0x0047E040/0x0047E470`; prior bridge reports | full bridge damage/repair path semantics out of scope |
| Railroad/rail claim | verified negative for this mask | no writer evidence tying railroad land/TMP data to `0x500`; mask bits are produced by bridge writer family | whole-binary symbol-name audit out of scope |
| A* `0x40000` boundary | verified negative | decompile `0x00429830`, `0x0042ACF0` | none for preventing conflation |
| Current Rust bridge facts | verified | `src/map/bridge_facts.rs`, `src/map/resolved_terrain.rs` refs above | dynamic runtime bridge-state preservation remains broader implementation audit |
| Current Rust TIBTRE validation | verified | `src/sim/terrain_spawn.rs:156..187` | implementation follow-up |

## 8. Open Questions - Final State of the Investigation Log

- `[RESOLVED] OQ1 - What is the investigation mode? -> exhaustive-slice for `CellClass+0x140 & 0x500` TIBTRE placement semantics only` (evidence: user scope and report header)
- `[RESOLVED] OQ2 - Is `CanPlaceTiberium` on the live TIBTRE path? -> yes, prior TIBTRE reports verify `TerrainClass::AI -> SpreadTiberium(force=1) -> CanPlaceTiberium` before placement` (evidence: [TIBTRE_CANACCEPTTIBERIUM_REJECTION_GATES_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TIBTRE_CANACCEPTTIBERIUM_REJECTION_GATES_GHIDRA_REPORT.md); `0x004838E0`)
- `[RESOLVED] OQ3 - What exact bits are in the mask? -> `TEST AH,0x5` after loading `CellClass+0x140`, i.e. dword mask `0x100 | 0x400` == `0x500`` (evidence: `0x004838FC..0x00483905`)
- `[RESOLVED] OQ4 - What does `0x100` mean here? -> structural bridge/on-bridge cell flag, written by intact `SetBridgeDirection_*` stamping and read by bridge/firing/pathfinder layer code` (evidence: `0x0047E040`, `0x0047E470`, `0x00703B10`, `0x0042ACF0`)
- `[RESOLVED] OQ5 - What does `0x400` mean here? -> destroyed/inactive bridge marker and placement blocker, written by `SetBridgeDirection_*` when state argument is zero` (evidence: `0x0047E040`, `0x0047E470`, `0x0047C620`)
- `[RESOLVED] OQ6 - Is `0x400` a rail/railroad bit? -> no evidence in this slice; writer and readers tie it to bridge state, not railroad land type` (evidence: `0x0047E040`, `0x0047E470`, `0x0047C620`; Rust railroad passability is separate in `src/sim/pathfinding/passability.rs`)
- `[RESOLVED] OQ7 - Who writes both bits? -> `CellClass::SetBridgeDirection_NESW/NWSE` derive `0x100` from `(state&1)<<8` and `0x400` from `(state==0)<<10`` (evidence: `0x0047E040`, `0x0047E470`)
- `[RESOLVED] OQ8 - Are the writers live in YR? -> yes, `OverlayClass::Mark` calls them for bridge overlay IDs at map load, and runtime xrefs include bridge update/damage/repair/resize sites` (evidence: `0x005FC570`; bulk xrefs to `0x0047E040/0x0047E470`)
- `[RESOLVED] OQ9 - Does `state=1` set or clear `0x400`? -> state=1 clears `0x400` and sets structural bits; map-load bridge overlay calls pass state=1` (evidence: `0x0047E040`, `0x005FC570`)
- `[RESOLVED] OQ10 - Does `state=0` set `0x400`? -> yes, anchor/forward/opposite destroy-state paths clear intact flags and set `0x400`` (evidence: `0x0047E040`, `0x0047E470`; `src/map/bridge_facts.rs:179..210` mirrors this)
- `[RESOLVED] OQ11 - Is `0x400` the A* bridge-approach cost marker? -> no, A* uses `0x40000` for the cost multiplier` (evidence: `0x00429830`, `0x0042ACF0`)
- `[RESOLVED] OQ12 - Does current Rust already retain the bits? -> yes in `BridgeCellFacts.raw_flags`, with constants for `0x100` and `0x400`` (evidence: `src/map/bridge_facts.rs:3..9`, `52..79`)
- `[RESOLVED] OQ13 - Does current Rust TIBTRE validation read those bits? -> no, `can_accept_tiberium` only takes resource nodes, optional `PathGrid`, and spawners` (evidence: `src/sim/terrain_spawn.rs:156..187`)
- `[RESOLVED] OQ14 - Can `PathGrid::is_walkable` stand in for the mask? -> no, `PathCell` carries structural bridge but not full raw `0x400`; walkability can diverge from placement-blocker flags` (evidence: `src/sim/pathfinding/core.rs:1110..1152`, `1470..1532`; `0x004838E0`)
- `[DEFERRED] OQ15 - Exact original Westwood/YRPP names for `0x100` and `0x400`` (category: requires-different-system-context; reason: symbolic labels are not ground truth and no reliable binary symbol name was found; next-step-if-pursued: dedicated cell-flag serialization/name audit)
- `[DEFERRED] OQ16 - Full dynamic bridge runtime state audit in Rust` (category: out-of-scope; reason: this report only maps the TIBTRE placement mask to existing facts; next-step-if-pursued: audit `src/sim/bridge_state` damage/repair transitions against `SetBridgeDirection(state=0/1)`)

Adversarial corner-case answers:

- Intact structural bridge cell: rejected by TIBTRE because `0x100` is set.
- Destroyed bridge marker over otherwise ground-walkable terrain: rejected by TIBTRE because `0x400` is set.
- A cell with only `0x200` transition: not rejected by the `0x500` mask, though later gates may reject it.
- A cell with `0x40000` only: not rejected by the `0x500` mask; `0x40000` is a temporary A* marker.
- Railroad/rail land type without bridge `0x100`/`0x400`: not rejected by this mask; any rejection must come from other gates such as land Buildable, slope, overlay, or `AllowTiberium`.

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| TIBTRE rejects candidates with `CellClass+0x140 & 0x100` structural bridge flag | `0x004838E0`; `0x0047E040`; `0x0047E470` | missing in `terrain_spawn.rs` unless `PathGrid::is_walkable` happens to reject | `src/sim/terrain_spawn.rs`; resolved terrain bridge facts passed into terrain-spawn validation | Check `ResolvedTerrainCell.bridge_facts.raw_flags & 0x100` or a dedicated `can_place_tiberium_bridge_blocked` predicate before placement | `tibtre_spread_rejects_structural_bridge_cell_even_if_other_gates_pass` | Do not rely on bridge walkability or ground walkability as equivalent to the placement mask |
| TIBTRE rejects candidates with `CellClass+0x140 & 0x400` destroyed/inactive bridge marker | `0x004838E0`; `0x0047E040`; `0x0047C620`; [CELLCLASS_0X140_BIT_0X400_PATHGRID_SEMANTIC_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/CELLCLASS_0X140_BIT_0X400_PATHGRID_SEMANTIC_GHIDRA_REPORT.md) | missing; `PathCell` does not carry full raw `0x400` marker | `src/map/bridge_facts.rs`; `src/map/resolved_terrain.rs`; `src/sim/terrain_spawn.rs` | Preserve and expose `BRIDGE_FLAG_DESTROYED_OR_RAMP` to TIBTRE validation, independent of pathing | `tibtre_spread_rejects_destroyed_bridge_marker_cell_even_when_ground_walkable` | Do not drop `0x400` because the bridge is no longer walkable; the placement blocker survives as its own bit |
| `0x500` is not a railroad/rail land-type test | writer/reader evidence from `0x0047E040`, `0x0047E470`, `0x004838E0`; no railroad writer evidence in scope | current Rust name `BRIDGE_FLAG_DESTROYED_OR_RAMP` is acceptable behavior-derived naming; no TIBTRE gate exists | docs/comments around future validation | Describe the gate as bridge structural or destroyed/inactive marker, not rail | `tibtre_spread_allows_plain_railroad_tile_when_other_can_place_tiberium_gates_pass` | Do not reject all railroad terrain unless another verified gate does so |
| `0x400` is distinct from A* `0x40000` | `AStar_compute_edge_cost @ 0x00429830`; `PathfinderClass__UpdateBridgePassability @ 0x0042ACF0` | Rust already separates raw bridge facts from A* pathing cost surfaces; terrain spawn lacks raw facts | `src/sim/pathfinding/core.rs`; `src/sim/terrain_spawn.rs` | Keep TIBTRE placement mask on raw bridge facts/resolved terrain, not A* temporary cost overlays | `tibtre_spread_does_not_reject_cell_with_only_bridge_approach_cost_marker` if a test seam exists | Do not implement the `0x500` check from `PathGrid` cost/marker state |
| Current Rust contains enough static map facts for map-load bridge cells but not in `terrain_spawn.rs` | `src/map/bridge_facts.rs:3..9`, `128..210`; `src/map/resolved_terrain.rs:573..623`; `src/sim/terrain_spawn.rs:156..187` | missing API/data threading | terrain spawner seeding/tick context | Add validation input that can query resolved terrain cell bridge raw flags by cell | `tibtre_can_place_uses_resolved_bridge_flags_not_pathgrid_walkability` | Do not duplicate bridge inference in terrain spawning; consume the existing authoritative facts |

## 10. Negative Facts / Do Not Do

- Do not call `CellClass+0x140 & 0x400` a railroad bit in the TIBTRE placement gate. The verified writer family is bridge state stamping, and sibling readers are bridge/placement/render-related.
- Do not implement `0x500` as `PathGrid::is_walkable == false`. Destroyed bridge marker cells can differ from movement walkability, and intact bridge structural cells are a placement-specific reject here.
- Do not conflate `0x400` with `0x40000`. The latter is a temporary A* bridge-approach cost marker.
- Do not reject `0x200` bridge-transition cells from this mask alone. `0x200` is not included in `0x500`; other `CanPlaceTiberium` gates may still reject the cell.
- Do not rederive bridge facts in `terrain_spawn.rs`. Rust already has `BridgeCellFacts.raw_flags`; future implementation should thread/read that data.

## 11. Remaining Uncertainty

- Exact original symbolic names for `0x100` and `0x400` remain unknown; this report uses behavior-derived labels.
- Full Rust dynamic bridge runtime transitions were not audited here. Static map-load facts match the relevant bit layout, but future damage/repair integration should verify that runtime `0x400` marker state remains available to TIBTRE validation.
- This report did not perform a new whole-binary scalar xref census for every `0x100` reader/writer. It relied on targeted Ghidra verification plus existing bridge/global-xref reports for out-of-scope consumers.

## Sources

- Ghidra decompile: `CellClass::CanPlaceTiberium @ 0x004838E0`
- Ghidra assembly context: `0x004838FC..0x00483905`
- Ghidra decompile: `CellClass::SetBridgeDirection_NESW @ 0x0047E040`
- Ghidra decompile: `CellClass::SetBridgeDirection_NWSE @ 0x0047E470`
- Ghidra decompile: `OverlayClass::Mark @ 0x005FC570`
- Ghidra bulk xrefs: `0x0047E040`, `0x0047E470`, `0x004838E0`, `0x0047C620`
- Ghidra decompile: `Cell_passability_building_placement @ 0x0047C620`
- Ghidra decompile: `AStar_compute_edge_cost @ 0x00429830`
- Ghidra decompile: `PathfinderClass::UpdateBridgePassability @ 0x0042ACF0`
- Ghidra decompile: `TechnoClass__IsOnBridge_ForFiring @ 0x00703B10`
- Prior reports: [TIBTRE_CANACCEPTTIBERIUM_REJECTION_GATES_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/TIBTRE_CANACCEPTTIBERIUM_REJECTION_GATES_GHIDRA_REPORT.md), [BRIDGE_SETBRIDGEDIRECTION_STAMPING_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/BRIDGE_SETBRIDGEDIRECTION_STAMPING_GHIDRA_REPORT.md), [BRIDGE_MAP_LOAD_AND_BRIDGEHEAD_TRANSITIONS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/01-assets-map-load-overlay/BRIDGE_MAP_LOAD_AND_BRIDGEHEAD_TRANSITIONS_GHIDRA_REPORT.md), [CELLCLASS_0X140_BIT_0X400_PATHGRID_SEMANTIC_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/CELLCLASS_0X140_BIT_0X400_PATHGRID_SEMANTIC_GHIDRA_REPORT.md), [CELLCLASS_0X140_0X400_GLOBAL_XREF_CENSUS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/CELLCLASS_0X140_0X400_GLOBAL_XREF_CENSUS_GHIDRA_REPORT.md), [PATHFINDER_UPDATE_BRIDGE_PASSABILITY_0042ACF0_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/03-traversal-pathfinding-entry/PATHFINDER_UPDATE_BRIDGE_PASSABILITY_0042ACF0_GHIDRA_REPORT.md)
- Rust refs: `src/map/bridge_facts.rs`, `src/map/resolved_terrain.rs`, `src/sim/pathfinding/core.rs`, `src/sim/terrain_spawn.rs`
