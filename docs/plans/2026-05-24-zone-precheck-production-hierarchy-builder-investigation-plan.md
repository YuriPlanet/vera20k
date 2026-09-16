# Zone Precheck Production Hierarchy Builder And Blocker Counts - Investigation Plan

> **For Codex:** This plan scopes a `/re-investigate` pass. Execute it by
> running `/re-investigate zone precheck production hierarchy builder and blocker counts`
> with this plan loaded as context, OR split the function inventory into the
> batched follow-ups listed in Section 10.

**Topic:** Production `Zone_precheck` hierarchy builder, bridge/tube edge emission, and `CellClass+0x122` blocker-neighbor counts.
**Scope Size:** Large - approx. 35 functions, 8 relevant INI/data keys.
**Est. Effort:** ~8-12 hours of `/re-investigate` work if done as one pass; recommended as 4 batches.
**Prior Research:** Substantial existing Ghidra reports; this plan scopes gaps and verification, not a blank-slate recovery.
**Expected Output:** research document at
`docs/research/ZONE_PRECHECK_PRODUCTION_HIERARCHY_BUILDER_AND_BLOCKER_COUNTS_GHIDRA_REPORT.md`
**Next Pipeline Step:** `/brainstorm` then implement production hierarchy builder and blocker-count producer.

---

## 1. Goal

Determine exactly how standard YR builds and maintains the three-level hierarchy consumed by `Zone_precheck`, including level records, parent links, per-cell level-0 zone lookup, edge order, edge flags, bridge/tube insertion, and incremental bridge mutation paths.

Also determine the production source for `CellClass+0x122` blocker-neighbor counts well enough to wire Rust's existing `BlockerNeighborCounts` surface without over-pruning hierarchical A*.

This investigation must end with an implementation handoff that says what Rust should build at map load, what it should rebuild after bridge/terrain/object changes, what can stay deferred, and which exact tests should guard player-visible bridge route parity.

## 2. Prior Research Inventory

| Report | Scope | Confidence | Known Gaps |
|--------|-------|------------|------------|
| [ZONE_MAP_BUILD_LEVEL_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/ZONE_MAP_BUILD_LEVEL_GHIDRA_REPORT.md) | `ZoneMap__BuildZoneLevel @ 0x00581F90`, scanline builder, temp graph, final graph, incremental sibling. | High for core build shape. | Needs implementation-facing condensation into exact Rust builder contract and tests. |
| [BRIDGE_ZONE_PRECHECK_HIERARCHY_WRITER_ORDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_ZONE_PRECHECK_HIERARCHY_WRITER_ORDER_GHIDRA_REPORT.md) | Writer order, parent/type fields, final adjacency emission, bridge/tube insertion position. | High for static writer order; medium for repeated mutation incidence. | Does not give a production Rust builder design or runtime route oracle. |
| [BRIDGE_ZONE_EDGE_FLAG_WRITER_SEMANTICS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_ZONE_EDGE_FLAG_WRITER_SEMANTICS_GHIDRA_REPORT.md) | `edge+4` low-byte writer/reader semantics. | High for scoped semantics. | Stock-map frequency of nonzero flags not measured. |
| [ZONE_PRECHECK_0042C290_HIERARCHY_EXCLUSIONS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ZONE_PRECHECK_0042C290_HIERARCHY_EXCLUSIONS_GHIDRA_REPORT.md) | Consumer-side hierarchy search, parent gate, exclusions, output. | High. | Consumer is mostly implemented; still verify builder output compatibility. |
| [ZONE_PRECHECK_0042C290_INSERTION_ORDER_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ZONE_PRECHECK_0042C290_INSERTION_ORDER_GHIDRA_REPORT.md) | Strict lower-cost replacement, insertion-order ties, no ZoneId tie. | High. | No production graph route assertion. |
| [ASTAR_MAIN_LOOP_LEVEL0_MARKER_GATE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/ASTAR_MAIN_LOOP_LEVEL0_MARKER_GATE_GHIDRA_REPORT.md) | Cell A* marker gate and `CellClass+0x122` exception. | High for gate; medium for first-slice sufficiency. | Requires production `+0x122` count producer in Rust. |
| [CELL_0x122_CAN_ENTER_CELL_SEMANTIC_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/CELL_0x122_CAN_ENTER_CELL_SEMANTIC_GHIDRA_REPORT.md) | Identifies `+0x122` as occupied-neighbor refcount and lists writer sites. | High. | Needs Rust-facing lifecycle grouping and bridge-layer implications checked. |
| [UPDATE_HIERARCHICAL_EDGES_RETRY_PRODUCER_SCOPE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/UPDATE_HIERARCHICAL_EDGES_RETRY_PRODUCER_SCOPE_GHIDRA_REPORT.md) | Retry-local exclusion producer after failed hierarchical A*. | High for producer scope. | Automatic retry producer remains deferred from current implementation. |
| [PATHFINDER_ZONE_EDGE_UPDATE_INVALIDATION_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/pathfinding/PATHFINDER_ZONE_EDGE_UPDATE_INVALIDATION_GHIDRA_REPORT.md) | Edge invalidation and retry exclusion details. | High for scoped retry behavior. | Needs integration decision: now or later. |
| [BRIDGE_ZONE_LIFECYCLE_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/02-cell-state-layering-zones/BRIDGE_ZONE_LIFECYCLE_GHIDRA_REPORT.md) | Bridge record lifecycle, global zone rebuild, add/remove bridge zone edges, GetZoneID. | High for many bridge zone lifecycle functions. | Very broad; extract only hierarchy-builder facts needed here. |
| `LOW_BRIDGE_ZONE_PRECHECK_LANDTYPE10_CONNECTIVITY_GHIDRA_REPORT.md` | Low bridge records included in hierarchy and precheck. | High. | Guardrail only; do not conflate low tubes with explicit direction-8 jumps. |
| [STOCK_LOW_BRIDGE_ROUTE_FIXTURE_READINESS_GHIDRA_REPORT.md](https://github.com/YuriPlanet/vera20k/blob/108924bc237d14342b68b8bb78f2bba0400d2443/docs/research/bridges/04-locomotion-height-tubes/STOCK_LOW_BRIDGE_ROUTE_FIXTURE_READINESS_GHIDRA_REPORT.md) | Carville fixture readiness. | High that fixture is valid; medium hook plan. | Exact route/zone chain still not logged. |

**Conflicts between reports:** Existing reports already resolve several stale claims:
- `CellClass+0x122` is not ore/fog/water state; it is occupied-neighbor count.
- Bridge/tube hierarchy edges are zero-flagged for `edge+4`; nonzero flags come from scanline boundary temp entries.
- Retry exclusions are pathfinder-local edges, not global graph mutations and not whole-zone bans.
- Direction-8 low-bridge tube movement is separate from hierarchy marker-gated normal compass edges.

## 3. Function Inventory

| # | Phase | Address | Current Name | Scope Reason | Depth Target | TS-Legacy Risk |
|---|-------|---------|--------------|--------------|--------------|----------------|
| 1 | 1 | `0x00567110` | `MapClass::InitZoneMap` / zone init caller | Establish full hierarchy build call order and data ownership. | FULL | Low |
| 2 | 1 | `0x00581F90` | `ZoneMap__BuildZoneLevel` | Primary production hierarchy builder for levels 2, 1, 0. | FULL | Low |
| 3 | 1 | `0x0042C1C0` | `PathfinderClass::AllocZoneArrays` / refresh arrays | Coupled scratch sizing after hierarchy rebuild. | MEDIUM | Low |
| 4 | 1 | `0x0042C290` | `Zone_precheck` | Consumer contract to validate builder output fields and array shapes. | MEDIUM | Low |
| 5 | 1 | `0x0042C900` | `AStar_pathfind_search` | Wrapper decides hierarchy enabled, same-zone failure, retry loop, marker-gated A*. | FULL | Low |
| 6 | 1 | `0x00429A90` | `AStar_main_loop` | Consumes level-0 markers and `+0x122` exception. | MEDIUM | Low |
| 7 | 2 | `0x0056C510` | `MapClass::UpdateBridgeZonesHelper` | Persistent zone rebuild path; must distinguish from hierarchy rebuild. | FULL | Low |
| 8 | 2 | `0x0056CB90` | `ZoneFloodFillScanLine` | Base cluster flood fill and boundary edge emission. | FULL | Low |
| 9 | 2 | `0x00582D70` | bridge/tube temp-edge injector | Adds active bridge/tube temp edges during hierarchy build. | FULL | Low |
| 10 | 2 | `0x00584550` | incremental hierarchy block rebuild | Local rebuild sibling; verify same temp/final graph semantics. | FULL | Low |
| 11 | 2 | `0x005851B0` | `MapClass::AddBridgeZoneEdges` | Direct bridge repair/validation edge insertion into final hierarchy graph. | FULL | Low |
| 12 | 2 | `0x00584E50` | `MapClass::RemoveBridgeZoneEdges` | Direct bridge collapse/invalidation edge removal; inverse check. | FULL | Low |
| 13 | 2 | `0x0056D6E0` | `MapClass::ComputeBridgeZones` | Builds high/low bridge records that feed edge insertion. | MEDIUM | Low |
| 14 | 2 | `0x0056DA10` | `MapClass::FindBridgeRecord` | High-only lookup; verify not used as low bridge hierarchy gate. | MEDIUM | Low |
| 15 | 2 | `0x0056DAE0` | `MapClass::InvalidateBridgeZones` | Collapse path to edge removal and active record state. | MEDIUM | Low |
| 16 | 2 | `0x0056DB70` | `MapClass::ValidateBridgeZones` | Repair path to edge addition and active record state. | MEDIUM | Low |
| 17 | 2 | `0x0056D230` | `MapClass::GetZoneID` | Cell-to-zone lookup and bridge redirect/perpendicular walk. | MEDIUM | Low |
| 18 | 2 | `0x00483C80` | `CellClass::RecalcZoneType` | Source of reduced zone type used by hierarchy records. | MEDIUM | Low |
| 19 | 2 | `0x005840C0` | `ZoneMap__FloodFillReachableZones` | Retry producer flood-fill helper; also validates hierarchy cell lookup semantics. | FULL | Low |
| 20 | 2 | `0x0042CCD0` | `PathfinderClass__UpdateHierarchicalEdges` | Automatic retry-local exclusion producer. | FULL | Low |
| 21 | 2 | `0x0042CF80` | `PathfinderClass__InvalidateZoneEdge` | Path-chain based invalidation/common-neighbor exclusions. | FULL | Low |
| 22 | 2 | `0x0042D170` | blocked-destination zone cost helper | Alternate caller of `Zone_precheck`; confirm if production builder output must serve it. | MEDIUM | Low |
| 23 | 3 | `0x005FC570` | `OverlayClass::Mark` | `CellClass+0x122` INC for wall overlay placement. | MEDIUM | Low |
| 24 | 3 | `0x00480630` | `CellClass::PostDestructionWallCleanup` | `+0x122` DEC on wall cleanup. | MEDIUM | Low |
| 25 | 3 | `0x00480CB0` | `CellClass::DestroyOverlay` | `+0x122` DEC on overlay destruction. | MEDIUM | Low |
| 26 | 3 | `0x00440580` | `BuildingClass::Unlimbo` | `+0x122` INC for building footprint placement. | MEDIUM | Low |
| 27 | 3 | `0x00445880` | `BuildingClass::Limbo` | `+0x122` DEC for building footprint removal. | MEDIUM | Low |
| 28 | 3 | `0x004D7170` | `FootClass::Unlimbo` | `+0x122` INC for unit entering cell. | MEDIUM | Low |
| 29 | 3 | `0x004DB260` | `FootClass::Limbo` | `+0x122` DEC for unit leaving cell. | MEDIUM | Low |
| 30 | 3 | `0x004D85D0` | `FootClass::PerCellProcess` | `+0x122` DEC/INC while moving between cells. | FULL | Low |
| 31 | 3 | `0x004CE840` | `FlyLocomotionClass::Descent_Step` | Aircraft landing transition writes to `+0x122`. | MEDIUM | Low |
| 32 | 3 | `0x0071C930` | `TerrainClass::Limbo` | `+0x122` DEC for terrain object removal. | MEDIUM | Low |
| 33 | 3 | `0x0071D000` | `TerrainClass::Unlimbo` | `+0x122` INC for terrain object placement. | MEDIUM | Low |
| 34 | 3 | `0x0042ACF0` | `PathfinderClass::UpdateBridgePassability` | Distinguish temporary `0x40000` bridge marker from `+0x122` blocker counts. | LIGHT | Low |
| 35 | 3 | `0x00429830` | `AStar_compute_edge_cost` | Ensure edge-cost facts are not mixed with hierarchy gating. | LIGHT | Low |

**Phase 1 checkpoint rule:** After functions #1-#6, stop and summarize whether the builder and consumer data contracts still match current Rust's `ZoneHierarchy` model. If not, revise the plan before continuing.

## 4. Detail Checklist

- **Hierarchy levels:** Verify level build order, block sizes (`8`, `4`, `2`), row-major zone id assignment, zone `0` sentinel, and per-cell level zone storage.
- **Record layout:** Extract final zone block fields: parent/coarser id, reduced zone type, edge pointer/count, and any count/capacity fields required for Rust.
- **Parent links:** Verify how level 0 links to level 1 and level 1 to level 2, including edge cases at block boundaries and sentinel zones.
- **Edge emission order:** Confirm temp bucket iteration, scanline edge insertion, bridge/tube temp edge insertion, final bidirectional emission, and duplicate rules.
- **Edge flags:** Confirm which producers set nonzero `edge+4` low byte, and prove bridge/tube add paths write zero flags.
- **Bridge/tube records:** Verify high bridge, wood bridge, low bridge/tube record inclusion, active/intact flags, and repair/collapse add/remove paths.
- **Incremental rebuilds:** Determine when full rebuild, incremental hierarchy rebuild, direct add/remove, or pathfinder scratch refresh is used.
- **Marker handoff:** Verify `Zone_precheck` selected paths and marker arrays are the only hierarchy output consumed by cell A*.
- **`+0x122` counts:** Group every writer into Rust lifecycle categories: walls/overlays, buildings, units, moving units, aircraft landing, terrain objects, resize/copy.
- **Layering:** Decide whether counts are ground-only or need bridge-layer handling. Do not infer; verify writer inputs and cell object-list semantics.
- **Explicit tubes:** Keep direction-8 tube jump semantics separate from hierarchy-gated normal compass edge expansion.
- **Slope:** Record exact blocker facts for `Zone_Estimate_Slope_Cost` and `FootClass+0x21C`, but do not require slope implementation for the first production builder unless the evidence makes it cheap.
- **TS-legacy checks:** For every path, state Active in YR: Yes/No/Conditional and note if stock YR defaults gate it off.

## 5. INI Keys in Scope

| Key | Section | Default | Suspected Purpose | Currently Parsed in Rust? |
|-----|---------|---------|-------------------|----------------------------|
| `MovementZone=` | unit/infantry/aircraft sections | varies | Selects passability matrix row used by `Zone_precheck` and Rust pathing. | Yes |
| `SpeedType=` | unit/infantry sections | varies | Terrain speed/cost input; not the direct matrix row in `Zone_precheck`. | Yes |
| `TooBigToFitUnderBridge=` | unit sections | varies | Bridge under/deck legality; adjacent to pathing but not hierarchy builder field. | Yes/Partial |
| `BridgeRepairHut=` | building sections | `CABHUT=yes` | Enables bridge repair hut behavior that can validate bridge zones. | Yes |
| `BridgeStrength=` | `[General]` | `1500` | Damage threshold leading to bridge collapse and zone mutation. | Yes |
| `DestroyableBridges=` | `[General]` | `yes` | Global bridge damage/collapse gate. | Yes |
| `BridgeDestruction=` | map `[SpecialFlags]` | map-specific | Scenario gate for bridge destruction; verify Carville/default behavior. | Yes/Partial |
| `TunnelSpeed=` | `[General]` | `1` | Low bridge/tunnel movement adjacent; ensure not confused with hierarchy builder. | Yes/Partial |

## 6. Caller & Integration Map

| Caller Address | Calls Into | When Invoked | Should Executor Decompile? |
|----------------|------------|--------------|----------------------------|
| `0x00567110` | `0x00581F90`, `0x0042C1C0` | Full map/zone init | YES - primary context |
| `0x00581F50` | `0x00581F90` | All-level hierarchy rebuild helper | YES - light wrapper/context |
| `0x00584550` | `0x00582D70`, final emission | Incremental hierarchy rebuild | YES |
| `0x0056C510` | zone flood fill and bridge edge baking | Persistent zone map rebuild | YES |
| `0x0056DAE0` | `0x00584E50` | Bridge invalidation/collapse | YES |
| `0x0056DB70` | `0x005851B0` | Bridge validation/repair | YES |
| `0x004CBBA0` | `0x0042C900` | Foot path wrapper | LIGHT - establish live path |
| `0x0042C900` | `0x0042C290`, `0x00429A90`, `0x0042CCD0` | Main A* wrapper/retry | YES |
| `0x00429A90` | `0x00429830`, CanEnter vtable | Cell A* loop | YES - marker gate only |
| `0x0042CCD0` | `0x005840C0`, `0x0042CF80` | Retry after failed hierarchical A* | YES |
| `0x0042D170` | `0x0042C290` | Blocked destination/path quality helper | YES - medium |

**Rust integration notes:**
- Current Rust consumer types exist in `src/sim/pathfinding/zone_hierarchy.rs`.
- `ZoneGrid` can store optional hierarchy in `src/sim/pathfinding/zone_map.rs`.
- `zone_search.rs` only uses hierarchy when test/private blocker counts are supplied; public production path remains compatibility corridor.
- `core.rs` has `BlockerNeighborCounts` and `HierarchyGate`, but no production count source.
- `zone_build.rs` still builds flat `ZoneMap`/`ZoneAdjacency` and bridge redirects, not production three-level hierarchy.
- Incremental bridge refresh currently updates path grids/zones through Rust surfaces in `zone_incremental.rs`, world bridge orchestrator, and `Simulation::rebuild_zone_grid`; exact hierarchy invalidation must be mapped before production wiring.

## 7. TS-Legacy Risk Register

- **TS-style fog:** Not in scope. Do not interpret `CellClass+0x122` as fog/shroud.
- **Subterranean/fog features:** Verify any unusual `MovementZone` rows are active before building acceptance tests around them.
- **Low bridge tube shells:** Standard YR uses low bridge/tube records, but zero-step auto tube shells are not explicit direction-8 jumps. Keep these paths separate.
- **`PathfinderClass+0x3C`:** Retry urgency is active in YR and not a bridge mode. Do not use it as a hierarchy-builder switch.
- **`CellClass+0x122` stale label:** Previous ore-neighbor naming is wrong. Treat as occupied-neighbor refcount unless the new pass finds contrary writer evidence.
- **`BridgeDestruction`/SpecialFlags:** Scenario-level gate affects collapse/mutation scenarios; verify defaults before using stock-map tests.

## 8. Current Rust Implementation Surface

- `src/sim/pathfinding/zone_hierarchy.rs`: new `ZoneHierarchy`, `ZoneLevelGraph`, `ZoneRecord`, `ZoneEdgeRecord`, `ZonePrecheckExclusions`, `zone_precheck_flat`; synthetic tests cover search order, parent gate, edge flags, exclusions, invalid parents/types.
- `src/sim/pathfinding/zone_map.rs`: `ZoneGrid` stores optional `ZoneHierarchy`; mutation APIs clear stale hierarchy.
- `src/sim/pathfinding/zone_search.rs`: private hierarchy branch can run when blocker counts are supplied; public path still compatibility corridor when counts are absent.
- `src/sim/pathfinding/core.rs`: `BlockerNeighborCounts`, `HierarchyGate`, and hierarchy-gated A* wrapper exist; no production count producer.
- `src/sim/pathfinding/zone_build.rs`: flat flood-fill, movement-zone zone maps, bridge adjacency injection, bridge redirect table.
- `src/sim/pathfinding/zone_incremental.rs`: incremental flat zone refresh; potential future owner for hierarchy invalidation or rebuild.
- `src/sim/bridge_state/*` and `src/sim/world/bridge_orchestrator.rs`: bridge damage/repair/collapse mutation sources that must mark hierarchy stale/rebuilt.
- `src/sim/movement/*`: occupancy and movement surfaces likely needed for production `+0x122` counts.

## 9. Deferred Open Questions

1. What exact Rust builder can reproduce binary level grouping without importing legacy memory layout?
2. Are exact packed-pair duplicate rules required for production route parity, or only final edge order/flags?
3. Should production hierarchy be rebuilt wholesale on every current `ZoneGrid` rebuild first, with direct add/remove deferred?
4. Where should Rust produce `BlockerNeighborCounts`: persistent `PathGrid`, `ZoneGrid`, world occupancy, or per-search scratch?
5. How do bridge/deck occupants affect `+0x122` writer semantics, if at all?
6. What exact stock Carville route/zone chain should be logged once the builder/count path exists?
7. Does `movement_zone.unwrap_or(mz)` in Rust need a stronger invariant or API split before production wiring?
8. Which explicit-tube scenarios must continue bypassing hierarchy-gated A*?

## 10. Execution Strategy

**Batched follow-ups** are recommended. Do not run one giant `/re-investigate` unless the user explicitly wants a long single report.

Recommended batches:

1. **Hierarchy full-build contract:** functions #1-#4, #8-#10, #18. Output: exact production `ZoneHierarchy` builder contract.
2. **Bridge/tube mutation contract:** functions #11-#17 plus low-bridge guardrails. Output: rebuild/stale/add/remove strategy.
3. **A* wrapper and retry contract:** functions #5-#6, #19-#22, #34-#35. Output: public wiring and deferred retry producer contract.
4. **Blocker-neighbor count lifecycle:** functions #23-#33. Output: Rust production source for `BlockerNeighborCounts`.

Use subagents only if the user explicitly asks for delegated execution. If delegated, each batch has disjoint research ownership and writes one report.

## 11. Success Criteria

The executed research document must:
- Answer every question in Section 1.
- Include every function from Section 3, or explicitly justify omission.
- Resolve every deferred question from Section 9 or re-document it as unresolved.
- State `Active in YR: Yes/No/Conditional` for every finding.
- Cite Ghidra addresses for every high-confidence claim.
- Produce an implementation handoff that names Rust files, required deltas, non-deltas, and acceptance tests.
- Separate production hierarchy builder facts from retry producer facts and from temporary bridge passability marker facts.
- Include negative facts: do not claim stock route parity, do not use one-ring corridor expansion as parity, do not treat missing blocker counts as zero, and do not conflate low bridge auto tubes with explicit direction-8 jumps.

## Sources

- Ghidra addresses sampled during planning: `0x0042C290`, `0x0042CCD0`, `0x005840C0`.
- Ghidra strings sampled: `SubzoneConnectionStruct`, `SubzoneTrackingStruct`, `ZoneConnectionClass`, `MovementZone`, bridge/ground failure strings.
- Docs searched: `docs/`, `docs/plans/`, `docs/research/`.
- INI files checked: `ini/rules.ini`, `ini/rulesmd.ini`, `ini/art.ini`, `ini/artmd.ini`.
- Related plans: `docs/plans/2026-05-23-bridge-zone-precheck-foundation-plan.md`, `docs/plans/2026-05-23-bridge-astar-cost-zone-precheck-plan.md`.
