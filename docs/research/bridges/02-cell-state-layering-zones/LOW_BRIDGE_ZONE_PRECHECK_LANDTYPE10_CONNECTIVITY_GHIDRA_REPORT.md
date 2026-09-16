# Low Bridge Zone Precheck LandType10 Connectivity -- Ghidra Research Report

**Address(es):** `0x00484AB0`, `0x00483C80`, `0x0056D6E0`, `0x0056C510`, `0x00581F90`, `0x00582D70`, `0x0042C290`, `0x0056DA10`
**Investigation Mode:** exhaustive-slice
**Claimed Scope:** Low-bridge `LandType == 10` / tube-backed cells as they are turned into zone records and consumed by the pre-A* hierarchical `Zone_precheck`.
**Non-Scope:** Movement interpolation, low-bridge rendering, bridge damage visuals, tube locomotor tick behavior, and high-bridge traversal details except where needed to distinguish record filters.
**Confidence:** High for record/filter/precheck behavior; Medium for the human-readable zone-type label of `LandType == 10` cells because it depends on the speed table and absence of overriding overlays/objects.
**Active in YR:** Yes. All primary functions are on standard YR map-load/pathfinding paths; no TS-only gate was found in this slice.

## 1. Overview

The pre-A* zone layer does not reject low bridges merely because their `BridgeRecord` kind is low. In cold zone construction, intact low bridge/tube records are included in the same bridge-record loops that build cluster and hierarchical zone connectivity. The high-only filter exists in `FindBridgeRecord`, which is used by high-bridge lookup/repair/damage style consumers, not by the all-active bridge-record loops that feed `Zone_precheck`.

For a standard unobstructed low bridge cell that satisfies `CellClass::IsLowBridgeCell` (`tube_index` valid and final `LandType == 10`), `RecalcZoneType` does not classify `LandType == 10` as impassable. It falls through to the default zone type unless an overlay/object-specific branch overrides it. That default zone type is passable for infantry and normal ground vehicle movement-zone rows, so zone precheck can allow ground -> low bridge -> ground connectivity when the low bridge record/adjacency is present.

## 2. Class Layout / Key Offsets

| Structure | Offset | Meaning | Evidence | Active in YR |
|---|---:|---|---|---|
| `CellClass` | `+0x116` | signed tube index; must be `0 <= index < g_TubeArray.count` for low-bridge predicate | `CellClass__IsLowBridgeCell @ 0x00484AB0` | Yes |
| `CellClass` | `+0xEC` | final numeric `LandType`; low bridge predicate requires `10` | `CellClass__IsLowBridgeCell @ 0x00484AB0` | Yes |
| `CellClass` | `+0x4C` | zone type byte written by `RecalcZoneType` and copied into zone-map cell data | `CellClass__RecalcZoneType @ 0x00483C80`, `CellClass__RecalcAttributes @ 0x0047D2B0` | Yes |
| `BridgeRecord` | `+0x00/+0x04` | endpoint A / endpoint B cell coords | `MapClass__ComputeBridgeZones @ 0x0056D6E0` | Yes |
| `BridgeRecord` | `+0x08` | intact byte; all-active zone builders require nonzero | `MapClass__UpdateBridgeZonesHelper @ 0x0056C510`, `ZoneMap__BuildZoneLevel @ 0x00581F90` | Yes |
| `BridgeRecord` | `+0x0C` | bridge kind: `0 = high`, `1 = low/tube` | writers in `MapClass__ComputeBridgeZones @ 0x0056D6E0`; filter in `FindBridgeRecord @ 0x0056DA10` | Yes |
| `MapClass` | `+0x68` | per-cell zone data: zone type, height, cluster id | `MapClass__UpdateBridgeZonesHelper @ 0x0056C510` | Yes |
| `MapClass` | `+0x70` / `DAT_0087F858` | per-cell hierarchical zone IDs, 10 bytes per cell | `ZoneMap__BuildZoneLevel @ 0x00581F90`, `Zone_precheck @ 0x0042C290` | Yes |
| `MapClass` | `+0x90` / `DAT_0087F878` | 3-level hierarchical zone graph consumed by `Zone_precheck` | `ZoneMap__BuildZoneLevel @ 0x00581F90`, `Zone_precheck @ 0x0042C290` | Yes |

## 3. Core Logic

### 3.1 Low bridge predicate is tube plus LandType 10

`CellClass__IsLowBridgeCell @ 0x00484AB0` returns true only when the cell has a valid tube index and `CellClass+0xEC == 10`.

Active in YR: Yes. The predicate is called from `MapClass__ComputeBridgeZones @ 0x0056D6E0` and other low-bridge/tube paths used in standard YR map/pathing behavior.

### 3.2 LandType 10 cells become passable zone-type candidates

`CellClass__RecalcZoneType @ 0x00483C80` assigns zone type `6` only when the relevant speed-table entry is at or below the impassable threshold, or when overlay/object blockers force it. The verified speed table row for `Tunnel (10)` has foot/track/wheel speeds of `1.0`, so an unobstructed `LandType == 10` low bridge cell falls through to default zone type `0` rather than impassable.

Active in YR: Yes. `CellClass__RecalcAttributes @ 0x0047D2B0` calls `RecalcZoneType` and copies the result into both cell and zone-map data during normal map initialization/recalculation.

### 3.3 ComputeBridgeZones writes low records

`MapClass__ComputeBridgeZones @ 0x0056D6E0` creates high records from high/wood bridge tile predicates and low records from `IsLowBridgeCell`. The low branch checks low-bridge neighbors in E/W or S/N pairs, reads tube data through `GetTubeAtCell`, consumes `tube+0x28` as the other endpoint, and writes `BridgeRecord+0x0C = 1`.

Active in YR: Yes. This runs during standard map zone initialization after cell attributes have been recalculated.

### 3.4 Full zone rebuild includes low records

`MapClass__UpdateBridgeZonesHelper @ 0x0056C510` iterates the bridge-record vector and only tests `record+0x08 != 0` before folding endpoint cluster pairs into the zone connection graph. It does not test `record+0x0C`, so intact low records participate.

Active in YR: Yes. This is the standard zone rebuild used after map/load terrain state changes.

### 3.5 Hierarchical BuildZoneLevel includes low records before Zone_precheck

`ZoneMap__BuildZoneLevel @ 0x00581F90` builds the final three-level graph consumed by `Zone_precheck`. For every active bridge record, it calls `FUN_00582D70` without filtering `record+0x0C`. `FUN_00582D70 @ 0x00582D70` handles the non-high case by using `GetTubeAtCell`, `tube+0x2C` direction, and walked tube path endpoints, then inserts three packed connection pairs with a zero low-byte flag.

Active in YR: Yes. The map-zone init path calls `BuildZoneLevel` for levels `2`, `1`, and `0`; these are the levels later read by `Zone_precheck`.

### 3.6 Zone_precheck applies movement-zone passability, not bridge kind

`Zone_precheck @ 0x0042C290` reads start and destination zone IDs at levels `2 -> 1 -> 0`, expands graph edges, and accepts a candidate zone only if `g_PassabilityMatrix[movement_zone * 8 + next_zone.zone_type] == 1` along with the hierarchical path gates. It does not read `BridgeRecord+0x0C`; by this point bridge/tube records have already been compiled into graph edges.

For default zone type `0`, the verified matrix has `1` for Normal, Crusher, Destroyer, Infantry, InfantryDestroyer, and other land rows. Thus infantry and ordinary ground vehicles can pass the zone precheck across low bridge/tube connectivity when the graph contains an intact low record.

Active in YR: Yes. `Zone_precheck` is called by the normal A* search path and nearby-cell fallback helper in standard YR.

### 3.7 FindBridgeRecord is high-only, but it is not the cold precheck inclusion gate

`MapClass__FindBridgeRecord @ 0x0056DA10` skips any record whose `+0x0C` kind is nonzero. That makes `FindBridgeRecord` high-only. This high-only behavior affects `GetZoneID` bridge redirect and validate/invalidate lookup paths, but it does not contradict the all-active bridge-record inclusion used by `UpdateBridgeZonesHelper` and `BuildZoneLevel`.

Active in YR: Yes. High-bridge lookup/repair/damage paths call this function; the low-record skip is live.

## 4. INI Keys

| Source | Relevant data | Verified effect for this slice |
|---|---|---|
| `ini/rulesmd.ini` / `ini/rules.ini` | low bridge overlay families `LOBRDG*`, `LOBRDGE*`, `LOBRDB*`, `LOBRDGB*` | Overlay identity is not sufficient for `IsLowBridgeCell`; binary requires tube index plus final `LandType == 10`. |
| `ini/rulesmd.ini` / `ini/rules.ini` | representative low bridge overlay entries use `Land=Road` and usually `NoUseTileLandType=true` | This is stale/misleading if treated as the pathing predicate; binary low-bridge predicate is numeric LandType 10 plus tube. |
| map `[Tubes]` | explicit TubeClass records with entry/exit/path steps | May provide nonzero tube paths consumed by low bridge/tube code; auto shells are a separate creation path. |

## 5. Integration Points

Cold path:

1. `CellClass__RecalcAttributes @ 0x0047D2B0` computes final land/zone/tube state.
2. `MapClass__ComputeBridgeZones @ 0x0056D6E0` creates high and low `BridgeRecord`s.
3. `MapClass__UpdateBridgeZonesHelper @ 0x0056C510` builds per-cluster and per-MovementZone connectivity using all active records.
4. `ZoneMap__BuildZoneLevel @ 0x00581F90` builds the three hierarchical levels and calls `FUN_00582D70` for every active bridge/tube record.
5. `Zone_precheck @ 0x0042C290` consumes those three levels before A*.

Incremental/high-bridge lookup path:

- `MapClass__FindBridgeRecord @ 0x0056DA10` filters to high records only. This is real but should not be generalized to the cold all-active graph construction path.

## 6. Current Rust Implementation Status

Rust now has several matching concepts and, as of the 2026-05-22 verify-doc audit, has named regression coverage for the exact low-bridge zone-precheck slice:

| Surface | Current status |
|---|---|
| `src/map/resolved_terrain.rs:200` | `ResolvedTerrainCell::is_low_bridge_tube_cell` mirrors the binary predicate shape: tube index present and YR cell land type tunnel (`10`). |
| `src/sim/pathfinding/core.rs:382` and `:891` | A* has an explicit direction-8 tube edge, but it accepts only explicit nonzero map tubes; auto low-bridge shell tubes remain predicate-only. |
| `src/sim/pathfinding/zone_build.rs:55` | `BridgeRecordFilter` already distinguishes all-active zone insertion from high-only lookup. |
| `src/sim/pathfinding/zone_build.rs:630` | `inject_bridge_adjacency(..., AllActive)` can include low records, matching the all-active zone-build behavior. |
| `src/sim/pathfinding/zone_build.rs:672` | bridge redirect can use `HighActiveOnly`, matching `FindBridgeRecord`-style consumers. |
| `src/sim/pathfinding/zone_build.rs:37` | movement-class passability rows are present; row 7 Infantry has column 0 passable and column 1 blocked, matching the verified matrix. |
| `src/sim/pathfinding/zone_map_tests.rs:522` | `stock_low_bridge_auto_shell_zone_grid_uses_low_records_without_explicit_tubes` covers Normal and Infantry ground-to-ground reachability through an active low `BridgeRecordKind::Low`. |

Rust delta for this report is now mostly guardrail maintenance: preserve the high-only vs all-active distinction and keep the low bridge ground -> low bridge -> ground zone-precheck regression from being weakened by future bridge refactors.

## 7. Coverage Ledger

| Area / function / branch | Status | Evidence | What remains |
|---|---|---|---|
| `CellClass__IsLowBridgeCell` predicate | verified | `0x00484AB0` | none |
| `CellClass__RecalcZoneType` LandType 10 zone-type consequence | verified with condition | `0x00483C80`; [CELLCLASS_ZONES_SPEED_BRIDGES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/CELLCLASS_ZONES_SPEED_BRIDGES.md) Tunnel speed row | overlay/object override cases outside this slice |
| `MapClass__ComputeBridgeZones` low record creation | verified | `0x0056D6E0` | exact map-authored tube endpoint cases beyond `tube+0x28` not expanded |
| `UpdateBridgeZonesHelper` all-active record loop | verified | `0x0056C510` | none for high/low filter question |
| `ZoneMap__BuildZoneLevel` all-active record loop | verified | `0x00581F90` | none for high/low filter question |
| `FUN_00582D70` low/tube connection insertion | verified | `0x00582D70` | exact path endpoint geometry should remain in tube movement docs |
| `Zone_precheck` passability gate | verified | `0x0042C290`, `g_PassabilityMatrix @ 0x0082A594` | exact runtime path chosen in equal-cost ties not measured |
| `FindBridgeRecord` high-only filter | verified | `0x0056DA10` | none |
| Movement interpolation across low bridge | deferred | non-scope | use trace-action / locomotor reports |
| Rendering/OnBridge behavior | deferred | non-scope | use bridge object/render reports |

## 8. Open Questions -- Final State of the Investigation Log

- `[RESOLVED] OQ1 -- Does the low-bridge predicate require overlay identity or tube-backed LandType 10? -> It requires valid tube index and `LandType == 10`; overlay identity alone is not checked.` (evidence: `0x00484AB0`)
- `[RESOLVED] OQ2 -- Are low bridge records created for the zone system? -> Yes, `ComputeBridgeZones` writes kind `1` records from `IsLowBridgeCell` and tube data.` (evidence: `0x0056D6E0`)
- `[RESOLVED] OQ3 -- Does cold/full zone rebuild filter bridge records to high-only? -> No, `UpdateBridgeZonesHelper` only checks intact byte and includes low records.` (evidence: `0x0056C510`)
- `[RESOLVED] OQ4 -- Does hierarchical `BuildZoneLevel` filter bridge records to high-only before `Zone_precheck`? -> No, it calls `FUN_00582D70` for every intact record regardless of kind.` (evidence: `0x00581F90`)
- `[RESOLVED] OQ5 -- What does the low branch of `FUN_00582D70` use? -> It uses `GetTubeAtCell`, tube direction `+0x2C`, path-walk endpoints, and inserts three connection pairs with zero flag low byte.` (evidence: `0x00582D70`)
- `[RESOLVED] OQ6 -- Does `Zone_precheck` read bridge kind? -> No; it reads prebuilt graph edges and movement-zone passability by zone type.` (evidence: `0x0042C290`)
- `[RESOLVED] OQ7 -- Can infantry pass the zone type used by unobstructed LandType 10 low bridge cells? -> Yes for default zone type 0; matrix row 7 column 0 is passable.` (evidence: `g_PassabilityMatrix @ 0x0082A594`; [ZONE_PASSABILITY_VERIFIED.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ZONE_PASSABILITY_VERIFIED.md))
- `[RESOLVED] OQ8 -- Can ordinary ground vehicles pass the same zone type? -> Yes for Normal/Crusher/Destroyer rows on default zone type 0.` (evidence: `g_PassabilityMatrix @ 0x0082A594`; [ZONE_PASSABILITY_VERIFIED.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ZONE_PASSABILITY_VERIFIED.md))
- `[RESOLVED] OQ9 -- Is `FindBridgeRecord` high-only? -> Yes; it skips `record+0x0C != 0`, so low records are filtered there.` (evidence: `0x0056DA10`)
- `[RESOLVED] OQ10 -- Does high-only `FindBridgeRecord` prove low records are absent from precheck connectivity? -> No; precheck uses graph already built by all-active loops.` (evidence: `0x0056C510`, `0x00581F90`, `0x0042C290`)
- `[RESOLVED] OQ11 -- Are bridge-derived precheck edge flags nonzero for low records? -> No evidence of nonzero bridge flags; `FUN_00582D70` inserts zero-flag temporary entries.` (evidence: `0x00582D70`; [BRIDGE_ZONE_EDGE_FLAGS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_ZONE_EDGE_FLAGS_GHIDRA_REPORT.md))
- `[DEFERRED] OQ12 -- Which specific retail maps provide explicit nonzero `[Tubes]` for every low bridge?` (category: `requires-different-system-context`; reason: map corpus audit is outside the precheck code slice; next-step-if-pursued: run a retail map `[Tubes]` extractor and compare low bridge cells)
- `[DEFERRED] OQ13 -- Do object/building overlays ever override a standard low bridge cell to zone type 5/6 in normal gameplay?` (category: `out-of-scope`; reason: object placement and damage overlay state are not part of this precheck connectivity slice; next-step-if-pursued: investigate low bridge damage/repair zone-type mutations)
- `[DEFERRED] OQ14 -- What is the exact player-visible tie-ordering effect of zero vs nonzero hierarchy-boundary edge flags?` (category: `needs-runtime-debugger`; reason: graph cost behavior is verified but equal-cost path selection needs runtime path comparison)

## 9. Implementation Handoff

| Verified behavior | Evidence | Current Rust delta | Affected Rust surface | Required implementation effect | Acceptance scenario | Risk / do-not-do |
|---|---|---|---|---|---|---|
| Intact low bridge records are included in full/cold zone connectivity and can let infantry/vehicles pass ground -> low bridge -> ground at precheck. | `UpdateBridgeZonesHelper @ 0x0056C510`, `BuildZoneLevel @ 0x00581F90`, `Zone_precheck @ 0x0042C290` | likely mostly present, needs focused regression | `src/sim/pathfinding/zone_build.rs`, `src/sim/pathfinding/zone_map_tests.rs` | Preserve `AllActive` bridge adjacency for zone connectivity, including `BridgeRecordKind::Low`. | Build a small terrain-aware zone grid with two ground zones separated by low bridge/tube cells plus an active low record; infantry and normal vehicle reachability succeeds. Proposed test: `test_low_bridge_landtype10_zone_precheck_allows_crossing`. | Do not apply the high-only `FindBridgeRecord` filter to full zone connectivity. |
| High-only filtering is real for `FindBridgeRecord`-style consumers but not for all-active precheck graph build. | `FindBridgeRecord @ 0x0056DA10`; contrast with `0x0056C510` and `0x00581F90` | represented by `BridgeRecordFilter`, needs guard tests around call sites | `src/sim/pathfinding/zone_build.rs`, bridge redirect call sites | Keep `HighActiveOnly` for bridge redirect/lookup behavior and `AllActive` for zone adjacency insertion. | A low bridge record does not produce a bridge redirect, but does produce zone adjacency when `AllActive` is used. Proposed test: `test_low_bridge_record_included_in_zone_adjacency_but_not_redirect`. | Do not collapse filters into a single `is_high` or single `all bridge records` policy. |
| `LandType == 10` low bridge predicate is not the same as overlay `Land=Road`; unobstructed `LandType == 10` cells are zone-precheck passable through default zone type, not water. | `IsLowBridgeCell @ 0x00484AB0`, `RecalcZoneType @ 0x00483C80`, [ZONE_PASSABILITY_VERIFIED.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ZONE_PASSABILITY_VERIFIED.md) matrix | partially present via `is_low_bridge_tube_cell`; terrain/class mapping remains fragile | `src/map/resolved_terrain.rs`, `src/sim/pathfinding/zone_build.rs`, `src/sim/pathfinding/passability.rs` | Preserve final YR land type 10 and tube index on low bridge cells; zone-class derivation must not turn them into water/road overlay truth in a way that blocks Infantry/Normal rows. | A resolved low bridge tube cell with `yr_cell_land_type == 10` and `tube_index` is classed passable for Infantry and Normal precheck rows. Proposed test: `test_low_bridge_landtype10_zone_class_is_ground_passable`. | Do not infer low bridge pathing from low bridge overlay ID or `Land=Road` alone. |

### Negative Facts / Do Not Do

- Do not implement low bridge precheck connectivity by using `FindBridgeRecord`; that function is high-only. Evidence: `FindBridgeRecord @ 0x0056DA10` skips `record+0x0C != 0`.
- Do not filter low records out of cold/full zone graph construction. Evidence: `UpdateBridgeZonesHelper @ 0x0056C510` and `BuildZoneLevel @ 0x00581F90` test intact byte, not kind.
- Do not treat `edge+4` low-byte nonzero as "low bridge" or "bridge edge". Bridge/tube helpers insert zero-flag entries. Evidence: `FUN_00582D70 @ 0x00582D70`, [BRIDGE_ZONE_EDGE_FLAGS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_ZONE_EDGE_FLAGS_GHIDRA_REPORT.md).
- Do not treat low bridge overlay `Land=Road` as the movement predicate. Evidence: `IsLowBridgeCell @ 0x00484AB0` requires final `LandType == 10` plus tube index.
- Do not model auto low-bridge tube shells as visible direction-8 movement edges. Evidence: [LOW_BRIDGE_TUBECLASS_PRODUCERS_AND_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/04-locomotion-height-tubes/LOW_BRIDGE_TUBECLASS_PRODUCERS_AND_LIFECYCLE_GHIDRA_REPORT.md); Rust `core.rs:382` already rejects zero-step auto shells for explicit tube jumps.

### Remaining Uncertainty

- Exact retail-map coverage of explicit nonzero `[Tubes]` records for low bridges was not audited; this report verifies the code path, not map corpus completeness.
- Overlay/object overrides on a low bridge cell could change the zone type after the default `LandType == 10` case; those damaged/occupied states are outside this precheck slice.
- The player-visible effect of the `0.001` hierarchy-boundary tiebreak was not runtime-measured; the flag read/write behavior is verified.

### Stale Docs / Follow-up Docs

- `docs/research/traces/PATHFIND_INFANTRY_LOW_BRIDGE_RAMP_TRACE.md`: replace Stage 1 `Status: UNCHECKED` wording with:
  - `Status: PASS (binary precheck slice verified): Zone_precheck consumes hierarchical zone graph levels built after all-active bridge/tube record insertion. Intact low BridgeRecord kind=1 records are included by UpdateBridgeZonesHelper/BuildZoneLevel, while FindBridgeRecord remains high-only for lookup/redirect consumers. For unobstructed LandType-10 tube cells, RecalcZoneType falls to default zone type 0, which Infantry and normal ground vehicle rows pass in g_PassabilityMatrix. Rust now has focused regression coverage in stock_low_bridge_auto_shell_zone_grid_uses_low_records_without_explicit_tubes.`
- `docs/research/BRIDGE_ASTAR_COSTS_AND_ZONE_PRECHECK_GHIDRA_REPORT.md`: replace `edge flags (low byte = 1 if bridge-edge)` with:
  - `edge flags (low byte causes the 0.001 zone-precheck tiebreak when nonzero; bridge/tube insertion helpers observed here write zero, so this must not be named bridge-edge without qualification).`

## Sources

- Ghidra decompiled/rechecked: `CellClass__IsLowBridgeCell @ 0x00484AB0`
- Ghidra decompiled/rechecked: `CellClass__RecalcZoneType @ 0x00483C80`
- Ghidra decompiled/rechecked: `CellClass__RecalcAttributes @ 0x0047D2B0`
- Ghidra decompiled/rechecked: `MapClass__ComputeBridgeZones @ 0x0056D6E0`
- Ghidra decompiled/rechecked: `MapClass__UpdateBridgeZonesHelper @ 0x0056C510`
- Ghidra decompiled/rechecked: `ZoneMap__BuildZoneLevel @ 0x00581F90`
- Ghidra decompiled/rechecked: `FUN_00582D70 @ 0x00582D70`
- Ghidra decompiled/rechecked: `Zone_precheck @ 0x0042C290`
- Ghidra decompiled/rechecked: `MapClass__FindBridgeRecord @ 0x0056DA10`
- Ghidra decompiled/rechecked: `MapClass__GetZoneID @ 0x0056D230`
- Docs consulted: [BRIDGE_LOW_AND_ZONE_RECORDS_GHIDRA_SUPPLEMENT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_LOW_AND_ZONE_RECORDS_GHIDRA_SUPPLEMENT.md), `LOW_BRIDGE_TUBECLASS_GHIDRA_REPORT.md`, [LOW_BRIDGE_TUBECLASS_DOC_VERIFICATION.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/04-locomotion-height-tubes/LOW_BRIDGE_TUBECLASS_DOC_VERIFICATION.md), [BRIDGE_ZONE_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_ZONE_LIFECYCLE_GHIDRA_REPORT.md), [BRIDGE_ZONE_EDGE_FLAGS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_ZONE_EDGE_FLAGS_GHIDRA_REPORT.md), [ZONE_PASSABILITY_VERIFIED.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ZONE_PASSABILITY_VERIFIED.md), [CELLCLASS_ZONES_SPEED_BRIDGES.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/CELLCLASS_ZONES_SPEED_BRIDGES.md), `traces/PATHFIND_INFANTRY_LOW_BRIDGE_RAMP_TRACE.md`
- Rust surfaces scanned: `src/sim/pathfinding/zone_build.rs`, `src/sim/pathfinding/core.rs`, `src/map/resolved_terrain.rs`, `src/map/tube_facts.rs`, `src/sim/bridge_specs.rs`
