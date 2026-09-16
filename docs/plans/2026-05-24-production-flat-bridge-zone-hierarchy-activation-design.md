# Production Flat Bridge Zone Hierarchy Activation Design

## Goal

Activate gamemd-style hierarchy precheck and marker-gated A* for production flat pathing, including flat retry parity, without claiming layered high-bridge, explicit-tube, slope, or stock-route parity.

## Architecture Context

Rust pathfinding currently has two surfaces:

- Compatibility pathing in `zone_search.rs`: one-level `ZoneGrid` reachability, `find_zone_corridor`, one-ring corridor expansion, then A* corridor filtering.
- New parity scaffolding in `zone_hierarchy.rs` and `core.rs`: three-level `ZoneHierarchy`, `zone_precheck_flat`, per-level selected paths/markers, `HierarchyGate`, and explicit `BlockerNeighborCounts`.

Production movement still follows the compatibility surface because:

- `ZoneGrid::build_with_terrain` stores no production `ZoneHierarchy`.
- `find_path_zoned_marker` only enters hierarchy mode when both a hierarchy and `BlockerNeighborCounts` are supplied.
- Public movement callers do not build or pass blocker-neighbor counts.
- `zone_precheck_flat` can consume manual exclusions, but the production failed-A* retry producer is not wired.

Relevant module boundaries:

- `sim/pathfinding/zone_build.rs` owns zone graph construction.
- `sim/pathfinding/zone_map.rs` owns `ZoneGrid` storage and invalidation.
- `sim/pathfinding/zone_hierarchy.rs` owns hierarchy records, precheck search, markers, paths, and exclusions.
- `sim/pathfinding/core.rs` owns A* candidate expansion, including `HierarchyGate`.
- `sim/pathfinding/zone_search.rs` owns the precheck/A* orchestration and retry loop.
- `sim/movement/*` builds dynamic blocker inputs and calls zone-aware pathfinding.

The design stays inside `sim/` and does not add render, UI, sidebar, audio, or net dependencies.

## Impact Analysis

Likely touched modules:

- `src/sim/pathfinding/zone_build.rs`
- `src/sim/pathfinding/zone_map.rs`
- `src/sim/pathfinding/zone_hierarchy.rs`
- `src/sim/pathfinding/zone_search.rs`
- `src/sim/pathfinding/core.rs`
- `src/sim/movement/bump_crush.rs`
- `src/sim/movement/movement_path.rs`
- focused tests under `src/sim/pathfinding/*_tests.rs` and movement tests where call signatures change.

Blast radius:

- Shared ground movement route selection.
- Dynamic blocker handling around units, buildings, walls, and bridge collapse.
- Deterministic route choice, because hierarchy adjacency order affects equal-cost paths.
- Incremental zone invalidation, because stale hierarchy data must not survive map/bridge/building updates.

Regression risks:

- Enabling marker-gated A* without real blocker-neighbor counts can over-prune valid gamemd routes.
- Building hierarchy from sorted sets can flip equal-cost detour selection.
- Retrying by banning whole corridors instead of exact failed hierarchy edges can hide reachable alternatives.
- Including explicit tube direction-8 before its marker semantics are designed can apply normal marker/cost behavior to a non-normal edge.
- Accidentally routing layered high-bridge paths through the flat hierarchy path would overclaim parity.

## Chosen Approach

Approach B: production flat activation with retry included.

Build a production flat `ZoneHierarchy` for eligible ground movement zones, derive real blocker-neighbor counts from the same deterministic blocker surfaces used for pathfinding, run `zone_precheck_flat`, feed level-0 markers into A*, and retry failed hierarchy/A* attempts up to gamemd's five-total-attempt budget using search-local undirected edge exclusions.

Do not activate this path when any required parity input is missing. Fall back to the compatibility path for unsupported cases rather than silently using all-zero counts or approximating explicit-tube/layered behavior.

## Tiny-Detail Ledger

- Hierarchy build/search order is `2 -> 1 -> 0`. Source: [BRIDGE_ZONE_PRECHECK_HIERARCHY_WRITER_ORDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_ZONE_PRECHECK_HIERARCHY_WRITER_ORDER_GHIDRA_REPORT.md); spot-check `FUN_00567110`.
- Level records carry parent/coarser zone id and reduced zone type. Source: [BRIDGE_ZONE_PRECHECK_HIERARCHY_WRITER_ORDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_ZONE_PRECHECK_HIERARCHY_WRITER_ORDER_GHIDRA_REPORT.md); `Zone_precheck @ 0x0042C290`.
- Lower levels are parent-gated by next-coarser chosen markers, except reduced type `1`. Source: [ZONE_PRECHECK_0042C290_INSERTION_ORDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ZONE_PRECHECK_0042C290_INSERTION_ORDER_GHIDRA_REPORT.md); spot-check `Zone_precheck @ 0x0042C290`.
- Type `1` bypasses only the parent gate; passability matrix and exclusions still apply. Source: `Zone_precheck @ 0x0042C290`.
- Candidate cost is parent cost plus zone base cost, optional slope cost, and edge flag addend. Source: `Zone_precheck @ 0x0042C290`.
- Edge flag low byte adds `0.001`; it is not a bridge-edge flag. Source: [BRIDGE_ZONE_EDGE_FLAG_WRITER_SEMANTICS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_ZONE_EDGE_FLAG_WRITER_SEMANTICS_GHIDRA_REPORT.md); spot-check `Zone_precheck @ 0x0042C290`.
- Bridge/tube direct edges are zero-flagged for the edge-flag addend. Source: [BRIDGE_ZONE_EDGE_FLAG_WRITER_SEMANTICS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_ZONE_EDGE_FLAG_WRITER_SEMANTICS_GHIDRA_REPORT.md).
- Final adjacency order is writer/insertion order, not `ZoneId` order. Source: [BRIDGE_ZONE_PRECHECK_HIERARCHY_WRITER_ORDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_ZONE_PRECHECK_HIERARCHY_WRITER_ORDER_GHIDRA_REPORT.md).
- Equal-cost candidates do not replace earlier candidates and equal heap entries do not bubble ahead by `ZoneId`. Source: [ZONE_PRECHECK_0042C290_INSERTION_ORDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ZONE_PRECHECK_0042C290_INSERTION_ORDER_GHIDRA_REPORT.md); spot-check `Zone_precheck @ 0x0042C290`.
- `Zone_precheck` writes selected paths and marker arrays for every level. Source: [ZONE_PRECHECK_0042C290_INSERTION_ORDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ZONE_PRECHECK_0042C290_INSERTION_ORDER_GHIDRA_REPORT.md).
- A* uses the level-0 marker array for normal candidate pruning. Source: [ASTAR_MAIN_LOOP_LEVEL0_MARKER_GATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ASTAR_MAIN_LOOP_LEVEL0_MARKER_GATE_GHIDRA_REPORT.md); spot-check `AStar_main_loop @ 0x00429A90`.
- Off-marker normal candidates are allowed when `CellClass+0x122 != 0`. Source: [ASTAR_MAIN_LOOP_LEVEL0_MARKER_GATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ASTAR_MAIN_LOOP_LEVEL0_MARKER_GATE_GHIDRA_REPORT.md); [CELL_0x122_CAN_ENTER_CELL_SEMANTIC_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/CELL_0x122_CAN_ENTER_CELL_SEMANTIC_GHIDRA_REPORT.md); spot-check `AStar_main_loop @ 0x00429A90`.
- `CellClass+0x122` is a blocker-neighbor refcount, not fog, shroud, water, bridge, ore, or amphibious state. Source: [CELL_0x122_CAN_ENTER_CELL_SEMANTIC_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/CELL_0x122_CAN_ENTER_CELL_SEMANTIC_GHIDRA_REPORT.md).
- Same-zone initial precheck failure disables hierarchy and continues to cell A*. Source: [BRIDGE_ASTAR_PRECHECK_RETRY_INTEGRATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/03-traversal-pathfinding-entry/BRIDGE_ASTAR_PRECHECK_RETRY_INTEGRATION_GHIDRA_REPORT.md).
- Cross-zone initial hierarchy failure returns no path before cell A*. Source: [BRIDGE_ASTAR_PRECHECK_RETRY_INTEGRATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/03-traversal-pathfinding-entry/BRIDGE_ASTAR_PRECHECK_RETRY_INTEGRATION_GHIDRA_REPORT.md).
- Default retry budget is five total attempts, not one attempt plus five retries. Source: [BRIDGE_ASTAR_PRECHECK_RETRY_INTEGRATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/03-traversal-pathfinding-entry/BRIDGE_ASTAR_PRECHECK_RETRY_INTEGRATION_GHIDRA_REPORT.md).
- Retry exclusions are search-local undirected hierarchy edges, not permanent graph mutations. Source: [UPDATE_HIERARCHICAL_EDGES_RETRY_PRODUCER_SCOPE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UPDATE_HIERARCHICAL_EDGES_RETRY_PRODUCER_SCOPE_GHIDRA_REPORT.md); [BRIDGE_ASTAR_PRECHECK_RETRY_INTEGRATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/03-traversal-pathfinding-entry/BRIDGE_ASTAR_PRECHECK_RETRY_INTEGRATION_GHIDRA_REPORT.md).
- Direction-8 explicit tube expansion bypasses normal edge cost and remains out of this activation scope. Source: [BRIDGE_DUAL_LAYER_ASTAR_SYSTEM_MODEL_SYNTHESIS.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/00-system-models/BRIDGE_DUAL_LAYER_ASTAR_SYSTEM_MODEL_SYNTHESIS.md); `LOW_BRIDGE_TUBECLASS_GHIDRA_REPORT.md`.
- Exact slope cost context and `Foot+0x21C` lifecycle remain deferred for this flat/no-slope activation. Source: [ZONE_ESTIMATE_SLOPE_COST_PARITY_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ZONE_ESTIMATE_SLOPE_COST_PARITY_GHIDRA_REPORT.md).
- Layered high-bridge route parity remains deferred. Source: [LAYERED_BRIDGE_PATH_ENTRY_FOUNDATION_SCOPE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/03-traversal-pathfinding-entry/LAYERED_BRIDGE_PATH_ENTRY_FOUNDATION_SCOPE_GHIDRA_REPORT.md).
- Stock Carville route cells remain trace-only, not an oracle. Source: [STOCK_LOW_BRIDGE_ROUTE_FIXTURE_READINESS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/04-locomotion-height-tubes/STOCK_LOW_BRIDGE_ROUTE_FIXTURE_READINESS_GHIDRA_REPORT.md).

## Design

### Components

#### Production Hierarchy Builder

Add a production hierarchy builder in `zone_build.rs`, separate from the compatibility one-level `ZoneMap` builder.

The builder should produce `ZoneHierarchy` for each eligible ground `MovementZone` while preserving the existing one-level `ZoneMap`, `ZoneAdjacency`, and `SuperZoneMap` outputs.

Builder requirements:

- Build levels in order `2`, `1`, `0`.
- Use the existing reduced zone type source from `ResolvedTerrainGrid` / `PathGrid` instead of raw TMP land type.
- Assign zone ids deterministically by row-major first discovery inside each level's aligned blocks.
- Write level 0 and level 1 parent ids from the next-coarser level; level 2 parent remains `0`.
- Preserve ordered edge records in `Vec<ZoneEdgeRecord>`.
- Avoid `BTreeSet`, `sort`, or `ZoneId` ordering for final parity adjacency.
- Append bridge/tube hierarchy edges after scanline edges, and set their edge flag to `0`.
- Keep exact duplicate handling explicit: if a scanline edge already establishes the exact pair, do not reorder it just because a bridge/tube record also mentions it.

Initial production eligibility should be conservative:

- `resolved_terrain.is_some()`;
- normal flat ground pathing only;
- movement zone can use the passability matrix row;
- no explicit map tube route is involved;
- slope contribution is intentionally zero until slope context is implemented.

If the builder cannot satisfy those inputs, it should leave `hierarchies` empty for that movement zone and the search path should remain on compatibility behavior.

#### Blocker-Neighbor Counts

Add a deterministic builder for `BlockerNeighborCounts`.

The count surface should be built from the current world snapshot, not stored in `PathGrid`, because it depends on current object and terrain-object lifecycle state.

Source inputs:

- live ground and bridge object-list occupants;
- building origins plus rules-derived foundation dimensions;
- wall overlay blockers and terrain-object blockers represented by resolved terrain.

Counting rule for this design:

- For every wall, terrain-object, and single-cell object source, increment the count on each of its 8 neighboring cells that is inside map bounds.
- For each building, increment the one-time expanded foundation rectangle `(origin.x - 1 ..= origin.x + width, origin.y - 1 ..= origin.y + height)`.
- Clamp to `u8::MAX`.
- Do not approximate buildings by adding 8-neighbor contributions for each foundation cell.
- Keep the count global, not selected-layer-only; bridge/deck occupants contribute to the same flat count surface.

This is intentionally a `CellClass+0x122` equivalent, not a replacement for `Can_Enter_Cell`.

#### Flat Hierarchy Search Orchestrator

Refactor `find_path_zoned_marker_inner` so hierarchy activation is a first-class branch:

1. Reject hierarchy mode when no hierarchy exists, no real `BlockerNeighborCounts` exists, explicit tubes are present, or the movement zone is unsupported.
2. Compute start and goal level-0 hierarchy zones from `ZoneHierarchy.level(0)`.
3. Run up to five total attempts.
4. Each attempt runs `zone_precheck_flat` with current search-local exclusions.
5. On precheck pass, call A* with `HierarchyGate`.
6. On A* success, return path.
7. On A* failure, update search-local hierarchy edge exclusions from the selected `ZonePrecheckResult` and retry.
8. On same-zone initial precheck failure, clear hierarchy for this search and run normal A*.
9. On cross-zone initial precheck failure, return `None`.

The old `SuperZoneMap::can_reach` precheck must not run before eligible hierarchy precheck. It remains only on the compatibility path.

#### Retry Producer

Implement the flat retry producer in `zone_hierarchy.rs` or `zone_search.rs` using `ZonePrecheckResult.paths` as one input, not the whole producer state.

First production slice should model the verified retry shape without trying to solve layered pathing:

- exclusions are search-local;
- exclusions are undirected canonical `ZoneEdgeKey`s;
- exclusions ban only the selected edge, not the endpoint zones;
- failed A* attempt uses the A* hierarchy progress cell, per-level cell-zone ids, `FloodFillReachableZones` split result, retained per-level paths, and graph adjacency;
- producer append order and duplicates are preserved even if consumer lookup uses canonical membership;
- retry count is five total attempts;
- no `ZoneGrid` rebuild happens as part of retry.

If the exact failed-cell-to-edge mapping is not sufficiently expressible from current Rust A* failure output, do not guess. Add a narrow blocker in the implementation plan for exposing enough A* failure context, or run a targeted re-investigation on `UpdateHierarchicalEdges` failure input mapping before implementing this subpiece.

#### Layered / Tube Boundary

Do not route `find_layered_path_zoned_marker` through this branch.

Do not enable hierarchy mode when explicit map tubes are involved. Direction-8 tube expansion is not a normal neighbor and does not receive normal edge-cost behavior; this design does not define its marker interaction.

### Interfaces / Contracts

Add or refine internal interfaces:

```rust
pub(crate) fn build_zone_hierarchy_with_terrain(
    path_grid: &PathGrid,
    resolved_terrain: &ResolvedTerrainGrid,
    bridge_records: &[BridgeEndpointRecord],
    mz: MovementZone,
    width: u16,
    height: u16,
) -> Option<ZoneHierarchy>
```

```rust
pub(crate) fn build_blocker_neighbor_counts(
    width: u16,
    height: u16,
    hard_blocks: Option<&BTreeSet<(u16, u16)>>,
    soft_blocks: Option<&LayeredEntityBlockMap>,
    layer: MovementLayer,
) -> BlockerNeighborCounts
```

```rust
pub(crate) fn add_retry_exclusions_from_failed_path(
    result: &ZonePrecheckResult,
    exclusions: &mut ZonePrecheckExclusions,
) -> bool
```

The exact signatures can change during implementation, but the contracts should not:

- missing hierarchy means compatibility path;
- missing real blocker counts means compatibility path;
- hierarchy-gated A* must always receive counts;
- hierarchy retry state is local to one path request;
- no sorted adjacency should feed parity precheck.

### Data Flow

Production rebuild:

1. `Simulation::rebuild_zone_grid` calls `ZoneGrid::build_with_terrain`.
2. `ZoneGrid::build_with_terrain` builds existing compatibility maps.
3. For eligible movement zones, it also builds and stores `ZoneHierarchy`.
4. Mutable zone updates continue to clear hierarchy data. Terrain-aware incremental updates currently fall back to full rebuild, which is acceptable.

Production path request:

1. Movement builds owner-specific hard and soft blocker maps.
2. Movement builds `BlockerNeighborCounts` from those maps.
3. `find_move_path_with_marker` passes counts into `find_path_zoned_marker`.
4. `find_path_zoned_marker` selects hierarchy branch only if all guardrails pass.
5. `zone_precheck_flat` produces selected paths and level-0 markers.
6. `core.rs` A* uses `HierarchyGate` marker plus blocker-neighbor exception.
7. Failed A* updates search-local edge exclusions and retries.
8. Unsupported cases fall back to compatibility path.

### Error Handling

- Invalid hierarchy zone ids fail closed inside hierarchy precheck.
- Invalid reduced zone types fail closed.
- Missing hierarchy or missing counts does not fail the whole path request; it uses compatibility path.
- Cross-zone hierarchy precheck failure fails the path request.
- Same-zone hierarchy precheck failure runs normal A*.
- Retry producer failure to identify a new edge stops retrying and returns `None` after the failed attempt, unless same-zone fallback applies.

### Testing Strategy

Hierarchy builder tests:

- `zone_hierarchy_builds_levels_2_1_0_from_resolved_terrain`
- `zone_hierarchy_writes_parent_ids_for_lower_levels`
- `zone_hierarchy_preserves_scanline_edge_insertion_order`
- `zone_hierarchy_bridge_edges_append_after_scanline_and_zero_flag`
- `zone_hierarchy_not_built_without_resolved_terrain`

Blocker-neighbor tests:

- `blocker_neighbor_counts_marks_eight_neighbors_only`
- `blocker_neighbor_counts_clamps_at_u8_max`
- `blocker_neighbor_counts_uses_owner_specific_hard_and_soft_blockers`
- `hierarchy_activation_requires_real_blocker_counts`

Activation tests:

- `production_flat_hierarchy_bypasses_reduced_superzone_abort`
- `production_flat_hierarchy_uses_marker_gate_not_one_ring_corridor`
- `production_flat_hierarchy_allows_off_marker_blocker_neighbor_exception`
- `production_flat_hierarchy_same_zone_failure_runs_astar`
- `production_flat_hierarchy_cross_zone_failure_aborts_before_astar`

Retry tests:

- `production_flat_hierarchy_retries_five_total_attempts`
- `production_flat_hierarchy_retry_exclusions_are_search_local`
- `production_flat_hierarchy_retry_excludes_edges_not_zones`
- `production_flat_hierarchy_retry_preserves_alternate_endpoint_route`

Boundary tests:

- `explicit_tube_scenario_stays_on_compatibility_path`
- `layered_pathing_does_not_use_flat_hierarchy_activation`
- `flat_activation_does_not_claim_carville_route_oracle`

Suggested verification:

```powershell
cargo test -q zone_hierarchy --lib
cargo test -q zone_map --lib
cargo test -q zone_search --lib
cargo test -q core --lib
cargo test -q movement_path --lib
cargo check -q
```

## Architectural Decisions

- Keep `ZoneHierarchy` separate from `SuperZoneMap`. They answer different questions: route selection vs reachability.
- Keep hierarchy activation behind explicit input availability. This avoids treating missing counts as all-zero counts, which would over-prune.
- Include flat retry now because production activation without retry can create visible no-path/wrong-detour mismatches.
- Keep explicit tubes out of scope because direction-8 is not a normal neighbor and bypasses normal edge-cost behavior.
- Keep layered pathing out of scope because layered high-bridge marker/retry integration has separate route-selection risks.
- Keep slope out of scope because the current design is flat/no-slope and `Foot+0x21C` lifecycle is still a separate target.

Tech debt intentionally left:

- Compatibility corridor remains for unsupported cases.
- Exact layered high-bridge route parity remains deferred.
- Explicit tube marker semantics remain deferred.
- Slope cost contribution remains deferred.
- Stock Carville route cells remain unasserted.

## Alternatives Considered

### A. Builder + Counts Only, No Activation

This would add production hierarchy and count surfaces without switching movement to use them.

Rejected as the chosen approach because it would still leave the player-visible bridge/pathing mismatch untouched.

### B. Flat Activation With Retry Included

Chosen. It activates hierarchy for the narrow production path that has enough evidence and includes retry behavior so activation does not introduce a new visible failure mode.

### C. Full Bridge Zone Parity Bundle

This would include flat activation, layered bridge marker handoff, exact explicit-tube behavior, slope context, and Carville route oracle.

Rejected for this design because several of those details need targeted trace/re-investigation and would blur the line between verified flat behavior and unresolved layered/tube/slope behavior.

## Open Questions

- Can the first retry producer derive correct failed edge exclusions from current A* failure output, or does A* need to expose the failed marker-path frontier?
- Which current blocker surfaces most closely match `CellClass+0x122` writer timing for normal movement: hard blocks only, hard plus soft blockers, or a narrower object-list-derived set?
- Does production hierarchy builder need a closer temp-bucket model before first activation, or can insertion-order tests against representative bridge/collapse fixtures establish sufficient parity for this flat slice?

If any of these questions blocks a precise implementation plan, run a narrow re-investigation or trace-swarm on that specific point rather than reopening the whole bridge zone system.
