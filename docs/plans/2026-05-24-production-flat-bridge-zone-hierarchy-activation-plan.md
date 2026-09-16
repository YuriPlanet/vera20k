# Production Flat Bridge Zone Hierarchy Activation - Implementation Plan

> **For Codex:** Execute this plan task-by-task. Keep patches small. Do not claim layered high-bridge route parity, explicit-tube direction-8 marker parity, slope-cost parity, or exact stock Carville route parity from this plan.

**Goal:** Activate gamemd-style `Zone_precheck` and marker-gated A* for eligible production flat paths, including real hierarchy construction, real blocker-neighbor counts, and flat five-total-attempt retry behavior.

**Design Doc:** [docs/plans/2026-05-24-production-flat-bridge-zone-hierarchy-activation-design.md](2026-05-24-production-flat-bridge-zone-hierarchy-activation-design.md)

---

## Grounding Summary

- `Zone_precheck @ 0x0042C290` searches hierarchy levels `2 -> 1 -> 0`, uses parent/type fields, passability matrix, strict lower-cost replacement, per-level paths/markers, and search-local undirected exclusions.
- `AStar_main_loop @ 0x00429A90` consumes the level-0 marker array for normal neighbor expansion, with `CellClass+0x122 != 0` as the off-marker blocker-neighbor exception.
- `FUN_00567110` builds hierarchy levels in `2 -> 1 -> 0` order.
- Current Rust has the hierarchy/precheck scaffold, but production `ZoneGrid::build_with_terrain` does not store real hierarchies and public movement calls do not pass real `BlockerNeighborCounts`.
- Production activation without retry can create a visible mismatch where gamemd retries a hierarchy edge exclusion and Rust returns no path. Flat retry belongs in this activation plan.

## Non-Goals

- Do not route `find_layered_path_zoned_marker` through this flat hierarchy branch.
- Do not enable hierarchy mode for explicit map-tube direction-8 scenarios.
- Do not implement exact slope contribution or `Foot+0x21C` lifecycle.
- Do not assert exact stock Carville post-collapse route cells or zone ids.
- Do not delete the compatibility corridor path; unsupported or missing-input cases must still use it.
- Do not treat missing blocker-neighbor counts as an all-zero grid.
- Do not mutate `ZoneGrid` or rebuild zones as part of retry.

## File Map

| Action | Path | Responsibility |
|---|---|---|
| Modify | `src/sim/pathfinding/zone_hierarchy.rs` | Production-safe hierarchy builders/helpers, retry exclusion producer helpers |
| Modify | `src/sim/pathfinding/zone_build.rs` | Build production `ZoneHierarchy` from `ResolvedTerrainGrid`, `PathGrid`, and bridge records |
| Modify | `src/sim/pathfinding/zone_map.rs` | Store built hierarchies during `ZoneGrid::build_with_terrain`; preserve invalidation |
| Modify | `src/sim/pathfinding/core.rs` | Add production `BlockerNeighborCounts` builder and keep `HierarchyGate` count-required |
| Modify | `src/sim/pathfinding/zone_search.rs` | Add flat hierarchy retry orchestration and public count plumbing |
| Modify | `src/sim/movement/movement_path.rs` | Build/pass flat blocker-neighbor counts for production flat paths |
| Modify/read | `src/sim/movement/bump_crush.rs` | Reuse hard/soft blocker surfaces; avoid duplicating blocker classification |
| Modify | `src/sim/pathfinding/*_tests.rs` | Builder, counts, activation, retry, and boundary tests |
| Modify | `src/sim/movement/*tests.rs` if needed | Call-signature and production movement activation tests |

## Interface Decisions

- Keep `ZoneHierarchy` distinct from `SuperZoneMap`.
- Add production hierarchy construction as an optional companion to existing one-level zone maps.
- Keep final hierarchy adjacency in insertion-order `Vec`s. Do not use sorted maps/sets as the route-choice surface.
- Keep `BlockerNeighborCounts` as explicit search input.
- Public flat pathing should accept `Option<&BlockerNeighborCounts>` or a new count-aware wrapper. The hierarchy branch must require `Some`; the compatibility branch must work with `None`.
- Retry exclusions are `ZonePrecheckExclusions` owned by one path request.
- Retry budget is five total attempts, including the first attempt.
- If retry producer cannot add a new exclusion, stop retrying instead of looping.
- `SuperZoneMap::can_reach` remains a compatibility precheck only and must not preempt eligible hierarchy precheck.

## Sim Checklist

- [ ] Stay inside `sim/` for gameplay logic.
- [ ] No `sim/` dependency on render/ui/sidebar/audio/net.
- [ ] No floating point in new sim cost or count logic.
- [ ] Preserve deterministic iteration and route tie order.
- [ ] Keep `EntityStore` as `BTreeMap`.
- [ ] Do not add new crates.
- [ ] Leave unrelated dirty worktree changes alone.

## Risks

- **Builder order drift:** a sorted or deduped hierarchy edge list can flip equal-cost detours.
- **Over-pruning:** marker-gated A* without real blocker-neighbor counts rejects valid gamemd candidates.
- **Retry overreach:** banning every edge in a selected path may be too broad if exact failed-edge mapping needs A* frontier context. This plan must not enable production retry from a guessed mapping.
- **Count-source drift:** using Rust's convenient hard/soft blocker sets is not sufficient unless they are verified to match `CellClass+0x122` writer timing for the scoped flat search.
- **Tube bleed-through:** explicit tube direction-8 is a separate edge type and must remain on compatibility behavior.
- **Layer bleed-through:** layered high-bridge routing must not use this flat activation branch.

---

## Tasks

### Task 1: Make hierarchy records production-buildable

**Why:** Current hierarchy scaffolding was built for tests. Production builder needs small crate-private mutation/access helpers without exposing internals broadly.

**Files:**
- Modify: `src/sim/pathfinding/zone_hierarchy.rs`

**Steps:**

1. Keep `SuperZoneMap` behavior unchanged.
2. Add or unhide crate-private helpers needed by `zone_build.rs`:
   - create a `ZoneLevelGraph` with width/height and sentinel zone;
   - set per-cell level zone ids;
   - append `ZoneRecord`s in deterministic id order;
   - append ordered `ZoneEdgeRecord`s;
   - query `zone_count`, `record`, and edge slices for tests.
3. Preserve zone `0` as invalid sentinel.
4. Do not expose helpers publicly outside `crate`.
5. Add unit tests proving helpers preserve insertion order and sentinel behavior.

**Acceptance:**

- Existing `zone_hierarchy` tests pass.
- Production builder can be written without direct field mutation from another module.

**Run:**

```powershell
cargo test -q zone_hierarchy --lib
```

### Task 2: Build one hierarchy level from terrain classes

**Why:** Production activation needs real level graphs, not synthetic fixtures.

**Files:**
- Modify: `src/sim/pathfinding/zone_build.rs`
- Modify: `src/sim/pathfinding/zone_hierarchy.rs` if helper gaps appear
- Modify: `src/sim/pathfinding/zone_map_tests.rs` or add focused hierarchy builder tests

**Steps:**

1. Add an internal level-builder helper for level `0`, `1`, or `2`.
2. Use block size `1 << (level + 1)`:
   - level 0 = 2x2;
   - level 1 = 4x4;
   - level 2 = 8x8.
3. For every aligned block, scan cells row-major and flood-fill passable cells for the target movement zone.
4. Use reduced zone type from `ResolvedTerrainGrid` / current movement-class derivation, not raw TMP land type.
5. Assign zone ids by first discovery order.
6. Store each cell's level-zone id in `ZoneLevelGraph`.
7. Store each zone's reduced type from the representative/discovered cells.
8. For this task, set parent ids to `0`; Task 3 wires true parents.
9. Extract scanline temporary edges into a temp-bucket/order-preserving staging surface that can emit final adjacency in the verified writer order:
   - bucket order first;
   - insertion order within each bucket;
   - low-halfword endpoint directed edge before high-halfword reverse edge.
10. Append final directed edges to `Vec`s without sorting.
11. Add duplicate suppression only if it preserves the first inserted edge/flag and never moves an existing edge.
12. If the exact temp-bucket key cannot be derived from the current research, stop this builder task and run a narrow re-investigation on `ZoneMap__BuildZoneLevel` temp edge bucket keys. Do not create or store a production `ZoneHierarchy` from an approximate edge-order model.

**Tests:**

- `zone_hierarchy_level_builder_uses_block_size_by_level`
- `zone_hierarchy_level_builder_assigns_ids_by_row_major_discovery`
- `zone_hierarchy_level_builder_emits_temp_bucket_order`
- `zone_hierarchy_level_builder_emits_low_halfword_edge_before_reverse`
- `zone_hierarchy_level_builder_rejects_invalid_zone_types`

**Run:**

```powershell
cargo test -q zone_hierarchy_level_builder --lib
cargo test -q zone_map --lib
```

### Task 3: Build full three-level hierarchy with parent ids

**Why:** `Zone_precheck` lower levels depend on parent/coarser ids, and flat activation without them would reproduce the old corridor approximation.

**Files:**
- Modify: `src/sim/pathfinding/zone_build.rs`
- Modify: tests

**Steps:**

1. Add `build_zone_hierarchy_with_terrain(...) -> Option<ZoneHierarchy>`.
2. Require `resolved_terrain`.
3. Build levels in order `2`, `1`, `0`.
4. After building level 2, assign level 1 parent ids by mapping each level-1 zone's representative cell into level 2.
5. After building level 1, assign level 0 parent ids by mapping each level-0 zone's representative cell into level 1.
6. Keep level 2 parent ids as `0`.
7. Fail closed with `None` if required dimensions/data are inconsistent.
8. Do not add bridge/tube edges yet; Task 4 handles them.

**Tests:**

- `zone_hierarchy_builds_levels_2_1_0`
- `zone_hierarchy_writes_level0_and_level1_parent_ids`
- `zone_hierarchy_top_level_parent_is_zero`
- `zone_hierarchy_missing_resolved_terrain_not_built`

**Run:**

```powershell
cargo test -q zone_hierarchy_builds_levels --lib
cargo test -q zone_precheck --lib
```

### Task 4: Add bridge hierarchy edges with zero flags

**Why:** Bridge collapse/repair changes the hierarchy graph. Bridge/tube direct edges must be appended after scanline edges and must not receive the `0.001` edge flag just because they are bridge edges.

**Files:**
- Modify: `src/sim/pathfinding/zone_build.rs`
- Modify: `src/sim/pathfinding/zone_map_tests.rs`
- Modify: `src/sim/pathfinding/zone_hierarchy.rs` if helper gaps appear

**Steps:**

1. Reuse existing `BridgeEndpointRecord` filtering facts:
   - all-active records for graph connectivity;
   - keep high-only redirect behavior separate and unchanged.
2. For each hierarchy level, map bridge endpoint cells to that level's zone ids.
3. Insert bridge edges into the same temp-edge staging surface after scanline temp edges, then emit through the same bucket/insertion-order finalizer.
4. Use edge flag `0`.
5. Preserve existing scanline edge order if the bridge pair already exists.
6. Do not replace low-bridge records with high-bridge redirect semantics.
7. Add tests that verify low records still contribute to graph connectivity where existing one-level behavior expects them.

**Tests:**

- `zone_hierarchy_bridge_edges_append_after_scanline_temp_edges`
- `zone_hierarchy_bridge_edges_emit_in_temp_bucket_order`
- `zone_hierarchy_bridge_edges_are_zero_flagged`
- `zone_hierarchy_duplicate_bridge_edge_does_not_reorder_scanline_edge`
- `zone_get_high_bridge_redirect_ignores_low_bridge_records`
- existing low-bridge zone map tests

**Run:**

```powershell
cargo test -q bridge_redirect --lib
cargo test -q bridge_edges --lib
cargo test -q zone_hierarchy --lib
```

### Task 5: Store production hierarchies in `ZoneGrid`

**Why:** Built hierarchies must become available to flat path search, while stale data must not survive dynamic pathing updates.

**Files:**
- Modify: `src/sim/pathfinding/zone_map.rs`
- Modify: `src/sim/pathfinding/zone_map_tests.rs`

**Steps:**

1. In `ZoneGrid::build_with_terrain`, after each one-level movement-zone map is built, call `build_zone_hierarchy_with_terrain` for eligible movement zones.
2. Insert hierarchy into `hierarchies` only on success.
3. Leave `hierarchies` empty when `resolved_terrain` is absent.
4. Preserve existing `map_mut`, `adjacency_mut`, and `set_super_zone` invalidation.
5. Confirm terrain-aware incremental updates still fall back to full rebuild, so hierarchy is rebuilt rather than incrementally patched.
6. Add tests around full build and mutation invalidation.

**Tests:**

- `zone_grid_build_with_terrain_stores_hierarchy_for_ground_mz`
- `zone_grid_build_without_terrain_stores_no_hierarchy`
- `zone_grid_hierarchy_accessors_clear_on_mutation`
- existing `zone_map` suite

**Run:**

```powershell
cargo test -q zone_grid_hierarchy --lib
cargo test -q zone_map --lib
```

### Task 6: Verify and build real blocker-neighbor counts

**Why:** The hierarchy marker gate is wrong without the `CellClass+0x122` off-marker exception.

**Files:**
- Modify: `src/sim/pathfinding/core.rs`
- Modify: `src/sim/pathfinding/core_tests.rs`

**Steps:**

1. Before adding production activation, verify that the selected Rust blocker inputs match the scoped `CellClass+0x122` writer behavior for flat normal pathing.
2. Use existing [CELL_0x122_CAN_ENTER_CELL_SEMANTIC_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/CELL_0x122_CAN_ENTER_CELL_SEMANTIC_GHIDRA_REPORT.md) evidence first. If it does not identify the writer timing/source precisely enough for Rust's hard/soft blocker split, stop this plan and run a narrow `/re-investigate CellClass+0x122 writer timing for flat AStar`.
3. Only after the source is verified, add `BlockerNeighborCounts::from_blockers(...)` or an equivalent crate-private builder.
4. Inputs must be the verified count-source surfaces:
   - grid width/height;
   - live single-cell object sources from both ground and bridge object-list layers;
   - wall overlays and terrain-object blockers from resolved terrain;
   - building origins plus rules-derived foundation dimensions.
5. For wall/terrain/single-cell object sources, increment all 8 in-bounds neighboring cells.
6. For buildings, increment the one-time expanded foundation rectangle `(origin.x - 1 ..= origin.x + width, origin.y - 1 ..= origin.y + height)`. Do not approximate buildings as 8-neighbor contributions from every foundation cell.
7. Clamp counts at `u8::MAX`.
8. Counts are global `CellClass+0x122` equivalents, not selected-layer-only. Bridge/deck occupants must contribute to the same flat count surface.
9. Keep existing `new`, `set_count`, and `count_at` test helpers if useful.
10. Do not infer counts inside A*.

**Tests:**

- `blocker_neighbor_counts_marks_eight_neighbors_only`
- `blocker_neighbor_counts_ignores_out_of_bounds_neighbors`
- `blocker_neighbor_counts_clamps_at_u8_max`
- `blocker_neighbor_counts_uses_verified_writer_source`
- `blocker_neighbor_counts_keeps_layers_separate`

**Run:**

```powershell
cargo test -q blocker_neighbor_counts --lib
cargo test -q astar_hierarchy --lib
cargo test -q core --lib
```

### Task 7: Plumb counts from movement to flat path search

**Why:** Production movement is the first real caller that can supply dynamic blocker context.

**Files:**
- Modify: `src/sim/movement/movement_path.rs`
- Modify: `src/sim/pathfinding/zone_search.rs`
- Modify: tests affected by function signature

**Steps:**

0. Do not start this task until Task 6 has verified the count source and mapped it to concrete Rust inputs. If Task 6 requires re-investigation and the result is inconclusive or maps to an unavailable Rust surface, keep production hierarchy activation disabled.
1. Add a count-aware flat pathing entry point or extend `find_path_zoned_marker` with an optional `BlockerNeighborCounts`.
2. Keep a compatibility wrapper for tests/callers that do not have counts.
3. Build `BlockerNeighborCounts` from the world/entity/terrain snapshot before movement pathing, where structure rules and both object-list layers are available.
4. Pass counts only to the flat path branch.
5. Do not pass counts to layered pathing.
6. Keep water/fly/no-zone unsupported cases on existing behavior.

**Tests:**

- `movement_flat_path_passes_blocker_neighbor_counts_to_zone_search`
- `hierarchy_activation_requires_real_blocker_counts`
- existing movement path tests compile and pass.

**Run:**

```powershell
cargo test -q movement_path --lib
cargo test -q zone_search --lib
```

### Task 8: Verify and add flat hierarchy retry producer helpers

**Why:** Production activation should retry failed hierarchy/A* attempts the way gamemd does, rather than returning no path after the first marker-gated failure.

**Files:**
- Modify: `src/sim/pathfinding/zone_hierarchy.rs`
- Modify: `src/sim/pathfinding/zone_search_tests.rs`

**Steps:**

1. Before implementing production retry, prove the failed-A*-to-zone-edge mapping for this flat scope.
2. Use [UPDATE_HIERARCHICAL_EDGES_RETRY_PRODUCER_SCOPE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UPDATE_HIERARCHICAL_EDGES_RETRY_PRODUCER_SCOPE_GHIDRA_REPORT.md) and [BRIDGE_ASTAR_PRECHECK_RETRY_INTEGRATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/03-traversal-pathfinding-entry/BRIDGE_ASTAR_PRECHECK_RETRY_INTEGRATION_GHIDRA_REPORT.md) first.
3. If those reports do not identify which selected-path edge(s) are invalidated from the failed A* state, stop this plan and run `/re-investigate UpdateHierarchicalEdges failed A* edge selection input`.
4. Only after the mapping is verified, add a helper that attempts to add search-local exclusions from a failed attempt.
5. Use retained `ZonePrecheckResult.paths` plus the A* hierarchy progress cell, per-level cell-zone ids, graph adjacency, and a `FloodFillReachableZones`-equivalent split detector. No A* frontier object is required.
6. Exclusions consumed by precheck are canonical undirected `ZoneEdgeKey`s, but the producer append surface must preserve order and duplicates for parity tests.
7. Exclusions must be per-level.
8. Exclusions must ban only edges, not endpoint zones.
9. `InvalidateZoneEdge` direct edge append comes first; common-neighbor append is asymmetric and appends only the earlier path-edge endpoint to each common neighbor in binary reverse scan order.
10. Return `false` only when the verified producer cannot append an actionable edge or clears hierarchy validity.
11. Do not enable production retry from a broad "ban selected path edge(s)" approximation unless a new source proves that approximation matches gamemd for the scoped flat path.

**Tests:**

- `retry_exclusions_add_verified_failed_edge`
- `retry_exclusions_are_undirected`
- `retry_exclusions_are_search_local`
- `retry_exclusions_do_not_ban_endpoint_zone`
- `retry_exclusions_return_false_when_no_new_edge_exists`

**Run:**

```powershell
cargo test -q retry_exclusions --lib
cargo test -q zone_precheck --lib
```

### Task 9: Replace one-shot hierarchy branch with five-attempt flat retry loop

**Why:** gamemd uses five total attempts for hierarchy pathing; a one-shot activation can create visible no-path mismatches.

**Files:**
- Modify: `src/sim/pathfinding/zone_search.rs`
- Modify: `src/sim/pathfinding/zone_search_tests.rs`

**Steps:**

0. Do not start this task until Task 8 has a verified retry producer. If Task 8 stopped for re-investigation, keep production hierarchy activation disabled.
1. In the eligible hierarchy branch, allocate `ZonePrecheckExclusions::default()` per path request.
2. Run attempts `0..5`, with the first attempt included in the count.
3. On each attempt, run `zone_precheck_flat`.
4. On pass, call marker-gated A* with `HierarchyGate`.
5. On A* success, return the path.
6. On A* failure, call the retry-exclusion helper and continue only if it added a new exclusion and attempts remain.
7. On initial same-zone precheck failure, clear hierarchy for this search and run normal A*.
8. On initial cross-zone precheck failure, return `None`.
9. On later precheck failure after exclusions, stop retrying and return `None`.
10. Keep compatibility `SuperZoneMap`/corridor behavior unchanged for no-hierarchy/no-count/explicit-tube cases.

**Tests:**

- `production_flat_hierarchy_retries_five_total_attempts`
- `production_flat_hierarchy_retry_stops_when_no_new_exclusion`
- `production_flat_hierarchy_retry_exclusions_are_search_local_between_calls`
- `production_flat_hierarchy_same_zone_failure_runs_astar`
- `production_flat_hierarchy_cross_zone_failure_aborts_before_astar`
- `production_flat_hierarchy_bypasses_reduced_superzone_abort`

**Run:**

```powershell
cargo test -q production_flat_hierarchy --lib
cargo test -q zone_search --lib
```

### Task 10: Add activation guardrails for explicit tubes and layered paths

**Why:** Flat activation must not accidentally claim unresolved tube or layered bridge parity.

**Files:**
- Modify: `src/sim/pathfinding/zone_search.rs`
- Modify: `src/sim/pathfinding/zone_search_tests.rs`
- Modify: layered path tests if needed

**Steps:**

1. Keep `has_explicit_tube_scenario` or equivalent guard before hierarchy activation.
2. Add a test fixture where explicit-tube terrain exists and verify the compatibility branch is used.
3. Verify `find_layered_path_zoned_marker` does not call the flat hierarchy branch.
4. Add comments near both branches naming deferred layered/tube parity.
5. Ensure no marker gate is applied to direction-8 tube expansion in this activation.

**Tests:**

- `explicit_tube_scenario_stays_on_compatibility_path`
- `layered_pathing_does_not_use_flat_hierarchy_activation`
- existing layered bridge pathing tests

**Run:**

```powershell
cargo test -q explicit_tube --lib
cargo test -q layered_path --lib
cargo test -q zone_search --lib
```

### Task 11: Production-route fixture tests

**Why:** Synthetic unit tests prove mechanics; production-style tests prove the actual movement call chain supplies hierarchy and counts.

**Files:**
- Modify: `src/sim/pathfinding/zone_search_tests.rs`
- Modify: `src/sim/movement/movement_path.rs` tests or nearby movement tests
- Modify: `src/sim/world/world_tests.rs` only if a world-level bridge-collapse fixture is already local and focused

**Steps:**

0. Do not add production retry success fixtures until Task 8 has verified the failed-edge retry producer and Task 9 has enabled the five-attempt loop. If Task 8 is blocked or inconclusive, keep production hierarchy activation disabled and limit this task to non-activation guardrail tests.
1. Add a flat path fixture where hierarchy markers prune an old one-ring corridor route.
2. Add a fixture where an off-marker cell is allowed only because blocker-neighbor count is nonzero.
3. Add a fixture where a first selected hierarchy path fails A*, retry excludes one verified failed edge, and the second selected path succeeds.
4. Add a production movement-path fixture that builds `ZoneGrid::build_with_terrain`, builds counts from blocker maps, and reaches the hierarchy branch.
5. Do not use stock Carville as an exact route oracle in this plan.

**Tests:**

- `production_flat_hierarchy_uses_marker_gate_not_one_ring_corridor`
- `production_flat_hierarchy_allows_off_marker_blocker_neighbor_exception`
- `production_flat_hierarchy_retry_selects_alternate_route`
- `movement_flat_path_activates_hierarchy_when_inputs_exist`

**Run:**

```powershell
cargo test -q production_flat_hierarchy --lib
cargo test -q movement_flat_path --lib
```

### Task 12: Focused verification and cleanup

**Why:** This touches shared pathfinding and movement call surfaces.

**Steps:**

1. Run formatting.
2. Run focused test groups.
3. Run `cargo check`.
4. Review comments to ensure they state deferred parity accurately.
5. Confirm no unsupported case now fails just because hierarchy data is missing.

**Run:**

```powershell
cargo fmt
cargo test -q zone_hierarchy --lib
cargo test -q zone_map --lib
cargo test -q zone_search --lib
cargo test -q core --lib
cargo test -q movement_path --lib
cargo check -q
```

If unrelated failures appear from the existing dirty worktree, report them and do not revert unrelated changes.

## Acceptance Criteria

- `ZoneGrid::build_with_terrain` stores a production `ZoneHierarchy` for eligible flat ground movement zones.
- Hierarchy levels are built in `2 -> 1 -> 0` order and carry correct parent ids.
- Hierarchy edge order follows verified temp-bucket/insertion order with low-halfword directed edge before reverse; it is not sorted `ZoneId` order.
- Bridge hierarchy edges enter after scanline temp edges, emit through the same final writer order, and are zero-flagged.
- Missing `resolved_terrain`, missing hierarchy, missing counts, explicit-tube cases, and layered paths remain on compatibility behavior.
- `BlockerNeighborCounts` are derived from verified `CellClass+0x122` writer-equivalent inputs and are required for hierarchy-gated A*.
- Marker-gated A* allows off-marker cells with nonzero blocker-neighbor count and rejects off-marker cells with zero count.
- Flat hierarchy branch uses five total attempts only after the failed-edge retry producer is verified.
- Retry exclusions are verified search-local, undirected, per-level edges.
- Same-zone precheck failure runs normal A*; cross-zone precheck failure aborts before A*.
- Eligible hierarchy precheck is not preempted by old `SuperZoneMap` reachability.
- No production path treats missing blocker counts as all zeros.
- Layered high-bridge route parity, explicit-tube direction-8 marker parity, slope-cost parity, and stock Carville route parity remain explicitly unclaimed.

## Follow-Up Queue After This Plan

1. If Task 8 reveals missing failure context, run a narrow `/re-investigate UpdateHierarchicalEdges failed A* edge selection input`.
2. Design and implement layered high-bridge marker/retry integration.
3. Re-investigate explicit-tube direction-8 marker behavior before enabling hierarchy there.
4. Re-investigate `Foot+0x21C` slope context lifecycle before slope-cost route parity.
5. Run a stock Carville low-bridge route trace and only then add exact route oracle tests.
