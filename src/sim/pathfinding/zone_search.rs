//! Zone-aware pathfinding — zone connectivity for fast unreachability detection
//! and hierarchical search-space reduction.
//!
//! ## What gamemd does
//!
//! `AStar_pathfind_search` @ `0x0042C900` is the single entrypoint, and the
//! hierarchical/regular split is one boolean it computes for itself:
//!
//! 1. Source zone from `MapClass::GetZoneID` with the mover's `Foot+0x8C`
//!    on-bridge flag; destination zone with the goal cell's `Flags & 0x100`.
//! 2. `MapClass::ResolvePathCoord_BridgeAware` projects both endpoints to
//!    logical coordinates. The projection feeds `Zone_precheck`, the playfield
//!    test below and the three failure logs — but **not** the search:
//!    `AStar_main_loop` @ `0x00429A90` receives the raw endpoints.
//! 3. `allowHS` = `GetTechnoType()->[+0xC94] == 0` **and** `mover->[+0x3D5] != 0`
//!    **and** `[mover vtable+0x320]()` (`0x004DA1D0`) `== 0` **and** both
//!    projected endpoints pass `MapClass::Is_Cell_In_Playfield(cell, 1)`
//!    (`0x0042CAC2` through `0x0042CB22`).
//! 4. **Zones equal** → if `allowHS`, run `Zone_precheck` @ `0x0042C290`; on
//!    failure log "Hierarchical findpath failure", clear `allowHS`, and run the
//!    A* anyway. **Zones differ** → if `allowHS`, return 0 with no A* at all;
//!    otherwise fall through to the A*.
//! 5. `AStar_main_loop` receives `allowHS` as its last argument, and it **is** a
//!    corridor filter: `Zone_precheck` stamps every zone on the coarse route
//!    into the level-0 array at `PathfinderClass+0x40` with the search serial
//!    from `+0x28`, and the neighbour loop at `0x00429EB1` skips any cell whose
//!    zone is unstamped unless the cell itself carries `CellClass+0x122`.
//!    [`super::core::HierarchyGate`] is that rule, with `BlockerNeighborCounts`
//!    standing in for `+0x122`. There is a **second exemption**: the whole skip
//!    is reached only when the layer byte at `[ESP+0x60]` is non-zero, and
//!    `0x00429E54`-`0x00429E7A` computes that byte as `1` *unless* the
//!    neighbour carries `Flags & 0x100` **and**
//!    `|PathfinderClass+0x30 − cell[+0x11B]| > 1`. A bridge-layer neighbour at a
//!    differing height therefore jumps from `0x00429EAF` straight to
//!    `0x00429F04` and is expanded regardless of the corridor stamp, the
//!    `+0x122` byte and `allowHS`.
//! 6. Retry budget is `param_6 != -1 ? 1 : 5`. Each retry calls
//!    `PathfinderClass::UpdateHierarchicalEdges` @ `0x0042CCD0`, re-reads
//!    `allowHS` from `PathfinderClass+0x38`, and re-runs `Zone_precheck`;
//!    a failed precheck ends the loop.
//!
//! ## What VERA does, and the remaining recorded gaps
//!
//! The live route is faithful in shape: `zone_precheck_flat` on the level-0
//! hierarchy, then a hierarchy-marked cell A*; a precheck failure with matching
//! zones falls back to the plain A*, and one with differing zones returns
//! `None`. What is missing:
//!
//! - **Two uncommon `allowHS` terms remain unmodelled.** VERA now threads the
//!   canonical `TechnoClass+0x3D5` byte and applies exact bridge-resolved mode-1
//!   endpoint membership before admitting hierarchy. It still does not model
//!   `IsTrain=` (the INI key
//!   behind `TechnoTypeClass+0xC94`, string `0x008444BC`, stored at
//!   `0x00712284` — absent from stock `rulesmd.ini`, so this term never fires);
//!   the `0x004DA1D0` predicate, which — given the other terms already hold at
//!   the call site — reduces to
//!   `mover+0x3D4 != 0` **or** current-or-queued mission == Retreat(4) **or**
//!   (`mover+0x5D4` non-null and `FUN_006EC300`). Player effect: for such a
//!   mover gamemd runs an unrestricted A*
//!   and can return a route where VERA answers "unreachable" from the zone map,
//!   so the unit refuses an order retail accepts. Frequency: a unit on Retreat
//!   or linked to the remaining team predicate — uncommon in ordinary
//!   skirmish, but not zero. Team6EC300 can perform mode1 waypoint lookups
//!   before endpoint membership, changing shared dummy state. Its Team+7F and
//!   action3 authority/order remain open; the current bool is supplied externally.
//! - **The corridor-Dijkstra fallback defines its corridor differently.**
//!   gamemd's corridor is the set of zones `Zone_precheck` stamped, widened by
//!   the per-cell `+0x122` escape (point 5). When VERA has no level-0 hierarchy
//!   or no blocker counts it instead builds a Dijkstra zone corridor, expands
//!   it by one ring, and retries up to [`MAX_CORRIDOR_RETRIES`] excluding
//!   corridor edges. The count coincides with gamemd's 5, but gamemd's retries
//!   re-run `UpdateHierarchicalEdges` and re-precheck rather than excluding
//!   edges. The former explicit-Tube bypass was removed with582D70 delivery.
//!   Every production caller supplies blocker counts (`movement_tick`,
//!   `world_commands`, the miner system, the production queue) and the
//!   production `ZoneGrid` always carries levels 0/1/2; fallback availability
//!   remains for incomplete callers. Player effect: a corridor excluding a route
//!   can make the unit refuse to move where retail walks it. No current live
//!   missing-cache trigger is established here. Downstream risk: it is a whole
//!   alternative search; replacing it is row GSI-06.03's work, not this row's.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/zone_map, sim/pathfinding, sim/locomotor.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::cmp::Ordering;
use std::collections::{BTreeSet, BinaryHeap};

use super::{
    BlockerNeighborCounts, LayeredEntityBlockMap, MoverSearchFacts, SearchCellCostClassifier,
    SearchMarkerOverlay,
};

use super::terrain_cost::TerrainCostGrid;
use super::zone_hierarchy::{ZonePrecheckExclusions, ZonePrecheckOutcome, zone_precheck_flat};
use super::zone_map::{ZONE_INVALID, ZoneAdjacency, ZoneGrid, ZoneId, ZoneMap};
use super::{
    LayeredPathStep, PathGrid, find_layered_path_hierarchy_marker, find_layered_path_marker,
    find_path_with_costs_corridor_marker, find_path_with_costs_hierarchy_marker_progress,
    find_path_with_costs_marker,
};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::tube_facts::TubeSource;
use crate::rules::locomotor_type::MovementZone;
use crate::sim::cell_rect::{PlayfieldBounds, cell_is_in_playfield_height_aware};
use crate::sim::movement::locomotor::MovementLayer;

/// Provenance of a returned path failure. The Foot wrapper must not turn
/// unavailable caches or compatibility-only rejection into a native core NULL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathSearchFailure {
    ///42CB22 rejected unequal native raw labels before cell A*.
    NativeEntryRejected,
    ///A required hierarchy cell had no represented native topology.
    MissingHierarchyCell,
    ///The reduced compatibility graph rejected without native raw evidence.
    CompatibilityZoneRejected,
    ///The existing cell search ran and returned no route.
    CellSearchExhausted,
    ///The compatibility corridor retry policy exhausted its alternatives.
    CompatibilityCorridorExhausted,
}

/// Maximum corridor Dijkstra attempts with zone-edge exclusions.
/// The recovered path entry contract uses a default total attempt cap of 5.
const MAX_CORRIDOR_RETRIES: u8 = 5;

#[allow(dead_code)]
const BLOCKED_DESTINATION_ALTERNATE_MARGIN: i32 = 6;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub(super) struct ZoneEdge {
    a: ZoneId,
    b: ZoneId,
}

impl ZoneEdge {
    pub(super) fn new(a: ZoneId, b: ZoneId) -> Option<Self> {
        if a == ZONE_INVALID || b == ZONE_INVALID || a == b {
            return None;
        }
        Some(if a < b {
            Self { a, b }
        } else {
            Self { a: b, b: a }
        })
    }
}

/// Whether the path entry may answer reachability from the reduced per-row zone
/// map before running A*.
///
/// Gamemd gates every row: `Can_Reach_Zone` short-circuits to "reachable" only on
/// `mzRow == -1`, and the A*-entry precheck reads whatever row the type's
/// `MovementZone=` gives. Stock rulesmd puts every main battle tank in
/// `Destroyer`, every ore miner in `Crusher` and the Battle Fortress in
/// `CrusherAll`, so excluding those rows bypassed the gate for the majority of
/// all path searches in a match.
///
/// `Water` / `WaterBeach` remain excluded — VERA-internal, gamemd equivalent
/// UNCHECKED. The terrain-aware zone builder's water/beach surface legality is
/// still coarser than the runtime water-surface predicate, so hard-gating naval
/// movers here would refuse orders gamemd accepts. Remove the exception once the
/// naval surface classes are pinned.
fn can_use_reduced_zone_precheck(movement_zone: Option<MovementZone>) -> bool {
    match movement_zone {
        None => true,
        // `mzRow == -1` short-circuits to "reachable" in the engine, so the
        // reduced zone gate must NOT be allowed to refuse the search.
        Some(MovementZone::Invalid) => false,
        Some(MovementZone::Water | MovementZone::WaterBeach) => false,
        Some(_) => true,
    }
}

fn can_reach_same_or_zoned(
    zg: &ZoneGrid,
    mz: MovementZone,
    from: (u16, u16),
    from_layer: MovementLayer,
    to: (u16, u16),
    to_layer: MovementLayer,
) -> bool {
    from == to || zg.can_reach(mz, from, from_layer, to, to_layer)
}

fn can_reach_through_explicit_tube(
    zg: &ZoneGrid,
    mz: MovementZone,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    resolved_terrain: Option<&ResolvedTerrainGrid>,
) -> bool {
    let Some(terrain) = resolved_terrain else {
        return false;
    };
    terrain.tube_facts().iter().any(|tube| {
        tube.source == TubeSource::ExplicitMap
            && tube.path_len() > 0
            && tube.exit != (0, 0)
            && can_reach_same_or_zoned(
                zg,
                mz,
                start,
                start_layer,
                tube.entry,
                MovementLayer::Ground,
            )
            && can_reach_same_or_zoned(
                zg,
                mz,
                tube.exit,
                MovementLayer::Ground,
                goal,
                MovementLayer::Ground,
            )
    })
}

// Original42C900 compares raw GetZoneID results, including distinct invalid
// row labels1/FFFF. Compatibility-only test/cache grids have no raw row.
#[cfg(test)]
fn native_path_zone_equality(
    zones: &ZoneGrid,
    terrain: Option<&ResolvedTerrainGrid>,
    movement: MovementZone,
    start: (u16, u16),
    start_bridge: bool,
    goal: (u16, u16),
    goal_bridge: bool,
) -> Option<bool> {
    let terrain = terrain?;
    Some(
        zones.get_path_zone_id_native(terrain, start, movement, start_bridge)?
            == zones.get_path_zone_id_native(terrain, goal, movement, goal_bridge)?,
    )
}

include!("native_path_entry.rs");

/// Zone-aware path search for flat (ground-only) paths.
///
/// Uses zone reachability plus a corridor-Dijkstra approximation, then runs A*
/// restricted to that corridor. If the bounded hierarchical attempts are
/// exhausted, the search fails rather than running an unrestricted fallback.
///
/// TODO(RE): terrain-aware nodeIndex connectivity can still be a little looser than
/// final movement legality because the recovered node flood-fill is 8-neighbor while
/// the actual step predicate also applies tighter per-move checks. Treat zone gating
/// here as a best-effort reject, not closed parity.
pub fn find_path_zoned(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    costs: Option<&TerrainCostGrid>,
    entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    urgency: u8,
    mover_is_crusher: bool,
    is_infantry: bool,
) -> Option<Vec<(u16, u16)>> {
    find_path_zoned_marker(
        grid,
        start,
        goal,
        costs,
        entity_blocks,
        zone_grid,
        mz,
        movement_zone,
        resolved_terrain,
        entity_block_map,
        None,
        None,
        urgency,
        mover_is_crusher,
        is_infantry,
        true,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn find_path_zoned_marker(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    costs: Option<&TerrainCostGrid>,
    entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    blocker_neighbor_counts: Option<&BlockerNeighborCounts>,
    urgency: u8,
    mover_is_crusher: bool,
    is_infantry: bool,
    allow_zone_hierarchy: bool,
    playfield_bounds: Option<PlayfieldBounds>,
) -> Option<Vec<(u16, u16)>> {
    find_path_zoned_marker_detailed(
        grid,
        start,
        goal,
        costs,
        entity_blocks,
        zone_grid,
        mz,
        movement_zone,
        resolved_terrain,
        entity_block_map,
        marker_overlay,
        blocker_neighbor_counts,
        MoverSearchFacts {
            urgency,
            mover_is_crusher,
            is_infantry,
            wall_cost: None,
        },
        allow_zone_hierarchy,
        playfield_bounds,
    )
    .ok()
}

pub(crate) fn find_path_zoned_marker_detailed(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    costs: Option<&TerrainCostGrid>,
    entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    blocker_neighbor_counts: Option<&BlockerNeighborCounts>,
    facts: MoverSearchFacts<'_>,
    allow_zone_hierarchy: bool,
    playfield_bounds: Option<PlayfieldBounds>,
) -> Result<Vec<(u16, u16)>, PathSearchFailure> {
    let entry = prepare_native_path_entry(
        zone_grid,
        resolved_terrain,
        movement_zone.unwrap_or(mz),
        start,
        false,
        goal,
        allow_zone_hierarchy,
        playfield_bounds,
    );
    find_path_zoned_marker_inner_detailed(
        grid,
        start,
        goal,
        entry.hierarchy_start,
        entry.hierarchy_goal,
        entry.raw_equal,
        costs,
        entity_blocks,
        if entry.endpoints_in_playfield {
            zone_grid
        } else {
            None
        },
        mz,
        movement_zone,
        resolved_terrain,
        entity_block_map,
        marker_overlay,
        facts,
        blocker_neighbor_counts,
    )
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn find_path_zoned_marker_inner(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    hierarchy_start: (u16, u16),
    hierarchy_goal: (u16, u16),
    native_zone_equal: Option<bool>,
    costs: Option<&TerrainCostGrid>,
    entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    urgency: u8,
    mover_is_crusher: bool,
    is_infantry: bool,
    blocker_neighbor_counts: Option<&BlockerNeighborCounts>,
) -> Option<Vec<(u16, u16)>> {
    find_path_zoned_marker_inner_detailed(
        grid,
        start,
        goal,
        hierarchy_start,
        hierarchy_goal,
        native_zone_equal,
        costs,
        entity_blocks,
        zone_grid,
        mz,
        movement_zone,
        resolved_terrain,
        entity_block_map,
        marker_overlay,
        MoverSearchFacts {
            urgency,
            mover_is_crusher,
            is_infantry,
            wall_cost: None,
        },
        blocker_neighbor_counts,
    )
    .ok()
}

fn find_path_zoned_marker_inner_detailed(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    hierarchy_start: (u16, u16),
    hierarchy_goal: (u16, u16),
    native_zone_equal: Option<bool>,
    costs: Option<&TerrainCostGrid>,
    entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    facts: MoverSearchFacts<'_>,
    blocker_neighbor_counts: Option<&BlockerNeighborCounts>,
) -> Result<Vec<(u16, u16)>, PathSearchFailure> {
    if !can_use_reduced_zone_precheck(movement_zone) {
        return find_path_with_costs_marker(
            grid,
            start,
            goal,
            costs,
            entity_blocks,
            movement_zone,
            resolved_terrain,
            entity_block_map,
            marker_overlay,
            facts,
        )
        .ok_or(PathSearchFailure::CellSearchExhausted);
    }

    let Some(zg) = zone_grid else {
        return find_path_with_costs_marker(
            grid,
            start,
            goal,
            costs,
            entity_blocks,
            movement_zone,
            resolved_terrain,
            entity_block_map,
            marker_overlay,
            facts,
        )
        .ok_or(PathSearchFailure::CellSearchExhausted);
    };

    let Some(zone_map) = zg.map_for(mz) else {
        return find_path_with_costs_marker(
            grid,
            start,
            goal,
            costs,
            entity_blocks,
            movement_zone,
            resolved_terrain,
            entity_block_map,
            marker_overlay,
            facts,
        )
        .ok_or(PathSearchFailure::CellSearchExhausted);
    };
    let start_zone = zone_map.zone_at(start.0, start.1, MovementLayer::Ground);
    let goal_bridge = resolved_terrain
        .and_then(|terrain| terrain.cell(goal.0, goal.1))
        .is_some_and(|cell| cell.bridge_facts.has_structural_bridge());
    let goal_zone = zone_map.zone_at(
        goal.0,
        goal.1,
        if goal_bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        },
    );
    let zones_match = native_zone_equal.unwrap_or(start_zone == goal_zone);

    let hierarchy_counts_available = blocker_neighbor_counts.is_some();
    if hierarchy_counts_available
        && let Some(hierarchy) = zg.hierarchy_for(mz)
        && let Some(level0_zones) = hierarchy.level(0)
    {
        //42CB22..42CB3F rejects unequal native base labels before precheck.
        if !zones_match {
            return Err(if native_zone_equal.is_some() {
                PathSearchFailure::NativeEntryRejected
            } else {
                PathSearchFailure::CompatibilityZoneRejected
            });
        }
        let hierarchy_start_zone = zg
            .hierarchy_zone_at_native(0, hierarchy_start)
            .ok_or(PathSearchFailure::MissingHierarchyCell)?;
        let hierarchy_goal_zone = zg
            .hierarchy_zone_at_native(0, hierarchy_goal)
            .ok_or(PathSearchFailure::MissingHierarchyCell)?;
        match zone_precheck_flat(
            hierarchy,
            hierarchy_start_zone,
            hierarchy_goal_zone,
            movement_zone.unwrap_or(mz),
            &ZonePrecheckExclusions::default(),
        ) {
            ZonePrecheckOutcome::Passed(result) => {
                return find_path_with_costs_hierarchy_marker_progress(
                    grid,
                    start,
                    goal,
                    costs,
                    entity_blocks,
                    level0_zones,
                    &result.marked[0],
                    blocker_neighbor_counts.expect("checked above"),
                    &result.paths[0],
                    movement_zone,
                    resolved_terrain,
                    entity_block_map,
                    marker_overlay,
                    facts,
                )
                .map(|result| result.path)
                .ok_or(PathSearchFailure::CellSearchExhausted);
            }
            ZonePrecheckOutcome::Failed if zones_match => {
                return find_path_with_costs_marker(
                    grid,
                    start,
                    goal,
                    costs,
                    entity_blocks,
                    movement_zone,
                    resolved_terrain,
                    entity_block_map,
                    marker_overlay,
                    facts,
                )
                .ok_or(PathSearchFailure::CellSearchExhausted);
            }
            ZonePrecheckOutcome::Failed => {
                return Err(PathSearchFailure::CompatibilityZoneRejected);
            }
        }
    }

    let zone_precheck_passed = zg.can_reach(
        mz,
        start,
        MovementLayer::Ground,
        goal,
        MovementLayer::Ground,
    );

    // Same-zone precheck failures disable hierarchy and still run cell A*.
    if !zone_precheck_passed && zones_match {
        return find_path_with_costs_marker(
            grid,
            start,
            goal,
            costs,
            entity_blocks,
            movement_zone,
            resolved_terrain,
            entity_block_map,
            marker_overlay,
            facts,
        )
        .ok_or(PathSearchFailure::CellSearchExhausted);
    }

    // Cross-zone precheck failure aborts without cell A*.
    if !zone_precheck_passed {
        if can_reach_through_explicit_tube(
            zg,
            mz,
            start,
            MovementLayer::Ground,
            goal,
            resolved_terrain,
        ) {
            return find_path_with_costs_marker(
                grid,
                start,
                goal,
                costs,
                entity_blocks,
                movement_zone,
                resolved_terrain,
                entity_block_map,
                marker_overlay,
                facts,
            )
            .ok_or(PathSearchFailure::CellSearchExhausted);
        }
        log::trace!(
            "zone_search: unreachable {:?} ({:?}→{:?}), skipping A*",
            mz,
            start,
            goal,
        );
        return Err(PathSearchFailure::CompatibilityZoneRejected);
    }

    let Some(adjacency) = zg.adjacency_for(mz) else {
        return find_path_with_costs_marker(
            grid,
            start,
            goal,
            costs,
            entity_blocks,
            movement_zone,
            resolved_terrain,
            entity_block_map,
            marker_overlay,
            facts,
        )
        .ok_or(PathSearchFailure::CellSearchExhausted);
    };

    let start_zone = zone_map.zone_at(start.0, start.1, MovementLayer::Ground);
    let goal_zone = zone_map.zone_at(goal.0, goal.1, MovementLayer::Ground);

    // Same zone — no corridor needed, run A* directly.
    if start_zone == goal_zone && start_zone != ZONE_INVALID {
        return find_path_with_costs_marker(
            grid,
            start,
            goal,
            costs,
            entity_blocks,
            movement_zone,
            resolved_terrain,
            entity_block_map,
            marker_overlay,
            facts,
        )
        .ok_or(PathSearchFailure::CellSearchExhausted);
    }

    // Try corridor-restricted A* with retry on failure.
    let mut excluded_edges: BTreeSet<ZoneEdge> = BTreeSet::new();
    for attempt in 0..MAX_CORRIDOR_RETRIES {
        if let Some(corridor_zones) =
            find_zone_corridor(zone_map, adjacency, start_zone, goal_zone, &excluded_edges)
        {
            // Expand corridor by one ring of neighbor zones for flexibility.
            let allowed = expand_corridor(&corridor_zones, adjacency);
            if let Some(path) = find_path_with_costs_corridor_marker(
                grid,
                start,
                goal,
                costs,
                entity_blocks,
                zone_map,
                &allowed,
                movement_zone,
                resolved_terrain,
                entity_block_map,
                marker_overlay,
                facts,
            ) {
                return Ok(path);
            }
            // Corridor A* failed — exclude all corridor zones and retry.
            log::trace!(
                "zone_search: corridor A* failed attempt {} ({} zones), retrying with exclusions",
                attempt + 1,
                corridor_zones.len(),
            );
            if !exclude_corridor_edges(&corridor_zones, &mut excluded_edges) {
                break;
            }
        } else {
            break; // Dijkstra couldn't find alternative route
        }
    }

    Err(PathSearchFailure::CompatibilityCorridorExhausted)
}

/// Zone-aware path search for layered (bridge-capable) paths.
///
/// Checks zone connectivity before invoking the layered A* pathfinder.
/// Bridge cells redirect to ground endpoint zones via `zone_at(Bridge)`,
/// so a single ground-layer reachability check covers cross-bridge paths.
pub fn find_layered_path_zoned(
    grid: &PathGrid,
    ground_blocks: Option<&BTreeSet<(u16, u16)>>,
    bridge_blocks: Option<&BTreeSet<(u16, u16)>>,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    terrain_costs: Option<&TerrainCostGrid>,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    urgency: u8,
    mover_is_crusher: bool,
    is_infantry: bool,
) -> Option<Vec<LayeredPathStep>> {
    find_layered_path_zoned_marker(
        grid,
        ground_blocks,
        bridge_blocks,
        start,
        start_layer,
        goal,
        zone_grid,
        mz,
        terrain_costs,
        movement_zone,
        resolved_terrain,
        entity_block_map,
        None,
        None,
        urgency,
        mover_is_crusher,
        is_infantry,
        true,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn find_layered_path_zoned_marker(
    grid: &PathGrid,
    ground_blocks: Option<&BTreeSet<(u16, u16)>>,
    bridge_blocks: Option<&BTreeSet<(u16, u16)>>,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    terrain_costs: Option<&TerrainCostGrid>,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    blocker_neighbor_counts: Option<&BlockerNeighborCounts>,
    urgency: u8,
    mover_is_crusher: bool,
    is_infantry: bool,
    allow_zone_hierarchy: bool,
    playfield_bounds: Option<PlayfieldBounds>,
) -> Option<Vec<LayeredPathStep>> {
    find_layered_path_zoned_marker_detailed(
        grid,
        ground_blocks,
        bridge_blocks,
        start,
        start_layer,
        goal,
        zone_grid,
        mz,
        terrain_costs,
        movement_zone,
        resolved_terrain,
        entity_block_map,
        marker_overlay,
        blocker_neighbor_counts,
        MoverSearchFacts {
            urgency,
            mover_is_crusher,
            is_infantry,
            wall_cost: None,
        },
        allow_zone_hierarchy,
        playfield_bounds,
    )
    .ok()
}

pub(crate) fn find_layered_path_zoned_marker_detailed(
    grid: &PathGrid,
    ground_blocks: Option<&BTreeSet<(u16, u16)>>,
    bridge_blocks: Option<&BTreeSet<(u16, u16)>>,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    terrain_costs: Option<&TerrainCostGrid>,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_block_map: Option<&LayeredEntityBlockMap>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    blocker_neighbor_counts: Option<&BlockerNeighborCounts>,
    facts: MoverSearchFacts<'_>,
    allow_zone_hierarchy: bool,
    playfield_bounds: Option<PlayfieldBounds>,
) -> Result<Vec<LayeredPathStep>, PathSearchFailure> {
    // `AStar @ 0x0042CAD6` admits hierarchy only while the mover's stored
    // TechnoClass+0x3D5 byte is true. False is not a hard failure: it bypasses
    // zone/hierarchy admission and runs the ordinary flat/layered cell A*.
    let source_layer = if start_layer == MovementLayer::Bridge {
        MovementLayer::Bridge
    } else {
        MovementLayer::Ground
    };
    let entry = prepare_native_path_entry(
        zone_grid,
        resolved_terrain,
        movement_zone.unwrap_or(mz),
        start,
        source_layer == MovementLayer::Bridge,
        goal,
        allow_zone_hierarchy,
        playfield_bounds,
    );
    let goal_layer = if entry.goal_bridge {
        MovementLayer::Bridge
    } else {
        MovementLayer::Ground
    };
    let hierarchy_start = entry.hierarchy_start;
    let hierarchy_goal = entry.hierarchy_goal;
    let zone_grid = if entry.endpoints_in_playfield {
        zone_grid
    } else {
        None
    };
    if !can_use_reduced_zone_precheck(movement_zone) {
        return find_layered_path_marker(
            grid,
            ground_blocks,
            bridge_blocks,
            start,
            start_layer,
            goal,
            terrain_costs,
            movement_zone,
            resolved_terrain,
            entity_block_map,
            marker_overlay,
            facts,
        )
        .ok_or(PathSearchFailure::CellSearchExhausted);
    }

    if let Some(zg) = zone_grid {
        // Raw results were captured before retained-cell projection/playfield.
        let zones_match = entry.raw_equal.unwrap_or_else(|| {
            zg.map_for(mz).is_some_and(|zone_map| {
                zone_map.zone_at(start.0, start.1, source_layer)
                    == zone_map.zone_at(goal.0, goal.1, goal_layer)
            })
        });

        if blocker_neighbor_counts.is_some()
            && resolved_terrain.is_some()
            && let Some(hierarchy) = zg.hierarchy_for(mz)
            && let Some(level0_zones) = hierarchy.level(0)
        {
            if !zones_match {
                return Err(if entry.raw_equal.is_some() {
                    PathSearchFailure::NativeEntryRejected
                } else {
                    PathSearchFailure::CompatibilityZoneRejected
                });
            }
            match zone_precheck_flat(
                hierarchy,
                zg.hierarchy_zone_at_native(0, hierarchy_start)
                    .ok_or(PathSearchFailure::MissingHierarchyCell)?,
                zg.hierarchy_zone_at_native(0, hierarchy_goal)
                    .ok_or(PathSearchFailure::MissingHierarchyCell)?,
                movement_zone.unwrap_or(mz),
                &ZonePrecheckExclusions::default(),
            ) {
                ZonePrecheckOutcome::Passed(result) => {
                    return find_layered_path_hierarchy_marker(
                        grid,
                        ground_blocks,
                        bridge_blocks,
                        start,
                        start_layer,
                        goal,
                        terrain_costs,
                        level0_zones,
                        &result.marked[0],
                        blocker_neighbor_counts.expect("checked above"),
                        &result.paths[0],
                        movement_zone,
                        resolved_terrain,
                        entity_block_map,
                        marker_overlay,
                        facts,
                    )
                    .ok_or(PathSearchFailure::CellSearchExhausted);
                }
                ZonePrecheckOutcome::Failed if zones_match => {
                    return find_layered_path_marker(
                        grid,
                        ground_blocks,
                        bridge_blocks,
                        start,
                        start_layer,
                        goal,
                        terrain_costs,
                        movement_zone,
                        resolved_terrain,
                        entity_block_map,
                        marker_overlay,
                        facts,
                    )
                    .ok_or(PathSearchFailure::CellSearchExhausted);
                }
                ZonePrecheckOutcome::Failed => {
                    return Err(PathSearchFailure::CompatibilityZoneRejected);
                }
            }
        }

        if !zg.can_reach(mz, start, source_layer, goal, goal_layer) {
            if zones_match {
                return find_layered_path_marker(
                    grid,
                    ground_blocks,
                    bridge_blocks,
                    start,
                    start_layer,
                    goal,
                    terrain_costs,
                    movement_zone,
                    resolved_terrain,
                    entity_block_map,
                    marker_overlay,
                    facts,
                )
                .ok_or(PathSearchFailure::CellSearchExhausted);
            }
            if can_reach_through_explicit_tube(zg, mz, start, start_layer, goal, resolved_terrain) {
                return find_layered_path_marker(
                    grid,
                    ground_blocks,
                    bridge_blocks,
                    start,
                    start_layer,
                    goal,
                    terrain_costs,
                    movement_zone,
                    resolved_terrain,
                    entity_block_map,
                    marker_overlay,
                    facts,
                )
                .ok_or(PathSearchFailure::CellSearchExhausted);
            }
            log::trace!(
                "zone_search: layered unreachable {:?} ({:?} layer={:?} -> {:?}), skipping A*",
                mz,
                start,
                start_layer,
                goal,
            );
            return Err(PathSearchFailure::CompatibilityZoneRejected);
        }
    }

    find_layered_path_marker(
        grid,
        ground_blocks,
        bridge_blocks,
        start,
        start_layer,
        goal,
        terrain_costs,
        movement_zone,
        resolved_terrain,
        entity_block_map,
        marker_overlay,
        facts,
    )
    .ok_or(PathSearchFailure::CellSearchExhausted)
}

// ---------------------------------------------------------------------------
// Hierarchical zone Dijkstra
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct ZoneQueueEntry {
    cost: i32,
    sequence: u32,
    zone: ZoneId,
}

impl Ord for ZoneQueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .cmp(&self.cost)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for ZoneQueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Find the cheapest coarse route through the zone adjacency graph.
/// Returns an ordered sequence of zone IDs from start to goal.
///
/// Edge cost still uses Rust's centroid Manhattan approximation, but equal-cost
/// ties follow gamemd.exe `Zone_precheck`: adjacency discovery order wins and
/// `ZoneId` is not a tie key.
pub(super) fn find_zone_corridor(
    zone_map: &ZoneMap,
    adjacency: &ZoneAdjacency,
    start_zone: ZoneId,
    goal_zone: ZoneId,
    excluded_edges: &BTreeSet<ZoneEdge>,
) -> Option<Vec<ZoneId>> {
    if start_zone == ZONE_INVALID || goal_zone == ZONE_INVALID {
        return None;
    }
    if start_zone == goal_zone {
        return Some(vec![start_zone]);
    }

    // Dijkstra on the zone graph with stable insertion-order ties.
    let zone_count = zone_map.zone_count as usize;
    let mut dist: Vec<i32> = vec![i32::MAX; zone_count + 1]; // 1-indexed
    let mut prev: Vec<ZoneId> = vec![ZONE_INVALID; zone_count + 1];
    let mut heap: BinaryHeap<ZoneQueueEntry> = BinaryHeap::new();
    let mut next_sequence: u32 = 1;

    dist[start_zone as usize] = 0;
    heap.push(ZoneQueueEntry {
        cost: 0,
        sequence: 0,
        zone: start_zone,
    });

    while let Some(ZoneQueueEntry { cost, zone, .. }) = heap.pop() {
        if zone == goal_zone {
            // Reconstruct path.
            let mut path = Vec::new();
            let mut cur = goal_zone;
            while cur != ZONE_INVALID {
                path.push(cur);
                cur = prev[cur as usize];
            }
            path.reverse();
            return Some(path);
        }
        if cost > dist[zone as usize] {
            continue; // stale entry
        }
        for &neighbor in adjacency.neighbors_of(zone) {
            if ZoneEdge::new(zone, neighbor).is_some_and(|edge| excluded_edges.contains(&edge)) {
                continue;
            }
            let Some(n_info) = zone_map.info_for(neighbor) else {
                continue;
            };
            // Edge cost: Manhattan distance between zone centers.
            let edge_cost = manhattan(
                zone_map.info_for(zone).map(|i| i.center).unwrap_or((0, 0)),
                n_info.center,
            );
            let new_cost = cost + edge_cost;
            if new_cost < dist[neighbor as usize] {
                dist[neighbor as usize] = new_cost;
                prev[neighbor as usize] = zone;
                heap.push(ZoneQueueEntry {
                    cost: new_cost,
                    sequence: next_sequence,
                    zone: neighbor,
                });
                next_sequence = next_sequence.wrapping_add(1);
            }
        }
    }

    None // No route through zone graph
}

pub(super) fn exclude_corridor_edges(
    corridor: &[ZoneId],
    excluded_edges: &mut BTreeSet<ZoneEdge>,
) -> bool {
    let mut inserted_any = false;
    for pair in corridor.windows(2) {
        if let Some(edge) = ZoneEdge::new(pair[0], pair[1]) {
            inserted_any |= excluded_edges.insert(edge);
        }
    }
    inserted_any
}

/// Manhattan distance between two cell coordinates.
fn manhattan(a: (u16, u16), b: (u16, u16)) -> i32 {
    (a.0 as i32 - b.0 as i32).abs() + (a.1 as i32 - b.1 as i32).abs()
}

#[allow(dead_code)]
fn chebyshev(a: (u16, u16), b: (u16, u16)) -> i32 {
    (a.0 as i32 - b.0 as i32)
        .abs()
        .max((a.1 as i32 - b.1 as i32).abs())
}

#[allow(dead_code)]
pub(crate) fn zone_cost_estimate(
    zg: &ZoneGrid,
    mz: MovementZone,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    goal_layer: MovementLayer,
) -> i32 {
    if !zg.can_reach(mz, start, start_layer, goal, goal_layer) {
        return i32::MAX;
    }

    let Some(zone_map) = zg.map_for(mz) else {
        return chebyshev(start, goal);
    };
    let start_zone = zone_map.zone_at(start.0, start.1, start_layer);
    let goal_zone = zone_map.zone_at(goal.0, goal.1, goal_layer);
    if start_zone == ZONE_INVALID || goal_zone == ZONE_INVALID {
        return i32::MAX;
    }
    if start_zone == goal_zone {
        return chebyshev(start, goal);
    }

    let Some(adjacency) = zg.adjacency_for(mz) else {
        return chebyshev(start, goal);
    };
    let empty_exclusions = BTreeSet::new();
    let Some(corridor) = find_zone_corridor(
        zone_map,
        adjacency,
        start_zone,
        goal_zone,
        &empty_exclusions,
    ) else {
        return i32::MAX;
    };

    let Some(start_center) = zone_map.info_for(start_zone).map(|info| info.center) else {
        return i32::MAX;
    };
    let Some(goal_center) = zone_map.info_for(goal_zone).map(|info| info.center) else {
        return i32::MAX;
    };

    let mut estimate = chebyshev(start, start_center);
    for pair in corridor.windows(2) {
        let Some(from) = zone_map.info_for(pair[0]).map(|info| info.center) else {
            return i32::MAX;
        };
        let Some(to) = zone_map.info_for(pair[1]).map(|info| info.center) else {
            return i32::MAX;
        };
        estimate = estimate.saturating_add(chebyshev(from, to));
    }
    estimate.saturating_add(chebyshev(goal_center, goal))
}

#[allow(dead_code)]
pub(crate) fn accepts_blocked_destination_alternate(
    helper_result: i32,
    original: (u16, u16),
    alternate: (u16, u16),
) -> bool {
    helper_result != i32::MAX
        && helper_result <= chebyshev(original, alternate) + BLOCKED_DESTINATION_ALTERNATE_MARGIN
}

/// Expand a corridor by adding all 1-hop neighbor zones.
/// This gives A* flexibility to route through cells near corridor boundaries.
fn expand_corridor(corridor: &[ZoneId], adjacency: &ZoneAdjacency) -> BTreeSet<ZoneId> {
    let mut allowed: BTreeSet<ZoneId> = corridor.iter().copied().collect();
    for &zone in corridor {
        for &neighbor in adjacency.neighbors_of(zone) {
            allowed.insert(neighbor);
        }
    }
    allowed
}

#[cfg(test)]
#[path = "zone_search_tests.rs"]
mod zone_search_tests;
