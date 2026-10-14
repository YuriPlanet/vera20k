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
//!   before endpoint membership. Foot's shared outside-playfield owner now
//!   calls the single Team6EC300 port, backed by Scenario waypoint authority.
//! - **Without blocker counts there is no hierarchy search.** The one
//!   production search, the live Foot search in `movement_tick`, supplies
//!   blocker counts, and every `ZoneGrid` carries levels 0/1/2. A caller
//!   without counts gets the plain cell A* once the zone test passes, where
//!   gamemd would run the hierarchy-marked search. No live trigger is
//!   established here.
//! - **gamemd's precheck retries are not modelled.** Its budget is
//!   `param_6 != -1 ? 1 : 5`, each retry re-running `UpdateHierarchicalEdges`
//!   and `Zone_precheck` (point 6); VERA runs one precheck.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/zone_map, sim/pathfinding, sim/locomotor.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

pub(crate) mod native_path_entry;

use super::{BlockerNeighborCounts, MoverSearchFacts, SearchMarkerOverlay};

use super::terrain_cost::TerrainCostGrid;
use super::zone_hierarchy::{
    MarkedZones, ZoneLevelGraph, ZonePrecheckExclusions, ZonePrecheckOutcome, ZonePrecheckThreat,
    zone_precheck_flat,
};
use super::zone_map::ZoneGrid;
use super::{
    HierarchyGate, LayeredPathStep, PathGrid, find_layered_path_marker, find_path_with_costs_marker,
};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::tube_facts::TubeSource;
use crate::rules::locomotor_type::MovementZone;
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::movement::locomotor::MovementLayer;
use native_path_entry::{NativePathEntry, prepare_native_path_entry};

/// Provenance of a returned path failure. The Foot wrapper must not turn
/// unavailable caches or compatibility-only rejection into a native core NULL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSearchFailure {
    ///Required live Foot inputs are unavailable; never a native NULL route.
    CellEntryUnavailable(String),
    ///42CB22 rejected unequal native raw labels before cell A*.
    NativeEntryRejected,
    ///A required hierarchy cell had no represented native topology.
    MissingHierarchyCell,
    ///The reduced compatibility graph rejected without native raw evidence.
    CompatibilityZoneRejected,
    ///AStar returned no route: its cell search ran out, or never ran because
    ///the start cell and height are the goal's (0x429BF3..0x429C0A).
    CellSearchExhausted,
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
        zones.get_zone_id_native(terrain, start, movement, start_bridge)?
            == zones.get_zone_id_native(terrain, goal, movement, goal_bridge)?,
    )
}

/// The level-0 corridor a passed `Zone_precheck` hands the cell A*
/// (`AStar_main_loop @ 0x00429A90`'s last argument, `allowHS`).
struct HierarchyCorridor<'a> {
    level0_zones: &'a ZoneLevelGraph,
    marked_level0: MarkedZones,
    blocker_neighbor_counts: &'a BlockerNeighborCounts,
}

impl HierarchyCorridor<'_> {
    fn gate(&self) -> HierarchyGate<'_> {
        HierarchyGate {
            level0_zones: self.level0_zones,
            marked_level0: &self.marked_level0,
            blocker_neighbor_counts: self.blocker_neighbor_counts,
        }
    }
}

/// The endpoints one search asks the zone ladder about. The flat and layered
/// searches share the ladder and differ only in these layers: the flat search
/// compares a ground start with a ground goal, and without native labels falls
/// back to the goal cell's structural-bridge flag; the layered search uses the
/// mover's layer and the goal's live `Flags & 0x100` (`0x0042C9B7`) throughout.
struct ZoneLadderQuery<'e> {
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    goal_layer: MovementLayer,
    /// The goal's layer when the zones are compared without native labels.
    fallback_goal_layer: MovementLayer,
    entry: &'e NativePathEntry,
}

/// Zone admission for one path search, after `prepare_native_path_entry`:
/// `Ok(None)` runs the plain cell A*, `Ok(Some(corridor))` the hierarchy-marked
/// one, and `Err` no cell A* at all.
fn admit_zone_search<'a>(
    zone_grid: Option<&'a ZoneGrid>,
    mz: MovementZone,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    blocker_neighbor_counts: Option<&'a BlockerNeighborCounts>,
    query: &ZoneLadderQuery<'_>,
    foot_entry: Option<&dyn super::SearchFootEntry>,
) -> Result<Option<HierarchyCorridor<'a>>, PathSearchFailure> {
    let entry = query.entry;
    if !can_use_reduced_zone_precheck(movement_zone) {
        return Ok(None);
    }
    let Some(zg) = zone_grid.filter(|_| entry.endpoints_in_playfield) else {
        return Ok(None);
    };
    let Some(zone_map) = zg.map_for(mz) else {
        return Ok(None);
    };
    // Raw results were captured before retained-cell projection/playfield.
    let zones_match = entry.raw_equal.unwrap_or_else(|| {
        zone_map.zone_at(query.start.0, query.start.1, query.start_layer)
            == zone_map.zone_at(query.goal.0, query.goal.1, query.fallback_goal_layer)
    });

    if let Some(blocker_neighbor_counts) = blocker_neighbor_counts
        && let Some(hierarchy) = zg.hierarchy_for(mz)
        && let Some(level0_zones) = hierarchy.level(0)
    {
        //42CB22..42CB3F rejects unequal native base labels before precheck.
        if !zones_match {
            return Err(if entry.raw_equal.is_some() {
                PathSearchFailure::NativeEntryRejected
            } else {
                PathSearchFailure::CompatibilityZoneRejected
            });
        }
        let start_zone = zg
            .hierarchy_zone_at_native(0, entry.hierarchy_start)
            .ok_or(PathSearchFailure::MissingHierarchyCell)?;
        let goal_zone = zg
            .hierarchy_zone_at_native(0, entry.hierarchy_goal)
            .ok_or(PathSearchFailure::MissingHierarchyCell)?;
        let padded_lookup = |index| {
            foot_entry
                .expect("provider present")
                .hierarchy_house_threat(index)
        };
        let threat = foot_entry
            .map(|provider| {
                provider
                    .finishing_threat_coefficient()
                    .map(|coefficient| ZonePrecheckThreat::new(coefficient, &padded_lookup))
            })
            .transpose()
            .map_err(PathSearchFailure::CellEntryUnavailable)?;
        return Ok(
            match zone_precheck_flat(
                hierarchy,
                start_zone,
                goal_zone,
                movement_zone.unwrap_or(mz),
                &ZonePrecheckExclusions::default(),
                threat,
            )
            .map_err(PathSearchFailure::CellEntryUnavailable)?
            {
                ZonePrecheckOutcome::Passed(result) => {
                    let [marked_level0, ..] = result.marked;
                    Some(HierarchyCorridor {
                        level0_zones,
                        marked_level0,
                        blocker_neighbor_counts,
                    })
                }
                // The zones match, so a failed precheck clears allowHS and
                // still runs the cell A*.
                ZonePrecheckOutcome::Failed => None,
            },
        );
    }

    // Same-zone precheck failures still run cell A*; a cross-zone one aborts
    // unless an explicit tube joins the endpoints.
    if zg.can_reach(
        mz,
        query.start,
        query.start_layer,
        query.goal,
        query.goal_layer,
    ) || zones_match
        || can_reach_through_explicit_tube(
            zg,
            mz,
            query.start,
            query.start_layer,
            query.goal,
            resolved_terrain,
        )
    {
        return Ok(None);
    }
    log::trace!(
        "zone_search: unreachable {:?} ({:?} layer={:?} -> {:?}), skipping A*",
        mz,
        query.start,
        query.start_layer,
        query.goal,
    );
    Err(PathSearchFailure::CompatibilityZoneRejected)
}

/// The flat (ground-only) zone-aware search without a marker overlay, blocker
/// counts or playfield bounds: the zone test gates a plain cell A*, and no
/// hierarchy-marked search runs without counts.
///
/// TODO(RE): terrain-aware nodeIndex connectivity can still be a little looser than
/// final movement legality because the recovered node flood-fill is 8-neighbor while
/// the actual step predicate also applies tighter per-move checks. Treat zone gating
/// here as a best-effort reject, not closed parity.
#[cfg(test)]
pub fn find_path_zoned(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    costs: Option<&TerrainCostGrid>,
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    mover_is_crusher: bool,
    is_infantry: bool,
) -> Option<Vec<(u16, u16)>> {
    find_path_zoned_marker(
        grid,
        start,
        goal,
        costs,
        zone_grid,
        mz,
        movement_zone,
        resolved_terrain,
        None,
        None,
        mover_is_crusher,
        is_infantry,
        true,
        None,
    )
}

#[cfg(test)]
pub(crate) fn find_path_zoned_marker(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    costs: Option<&TerrainCostGrid>,
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    blocker_neighbor_counts: Option<&BlockerNeighborCounts>,
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
        zone_grid,
        mz,
        movement_zone,
        resolved_terrain,
        marker_overlay,
        blocker_neighbor_counts,
        MoverSearchFacts {
            urgency: 0,
            mover_is_crusher,
            is_infantry,
            foot_entry: None,
        },
        allow_zone_hierarchy,
        playfield_bounds,
    )
    .ok()
    .map(|steps| steps.into_iter().map(|step| (step.rx, step.ry)).collect())
}

pub(crate) fn find_path_zoned_marker_detailed(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    costs: Option<&TerrainCostGrid>,
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    blocker_neighbor_counts: Option<&BlockerNeighborCounts>,
    facts: MoverSearchFacts<'_>,
    allow_zone_hierarchy: bool,
    playfield_bounds: Option<PlayfieldBounds>,
) -> Result<Vec<LayeredPathStep>, PathSearchFailure> {
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
    find_flat_path_after_entry(
        grid,
        start,
        goal,
        &entry,
        costs,
        zone_grid,
        mz,
        movement_zone,
        resolved_terrain,
        marker_overlay,
        facts,
        blocker_neighbor_counts,
    )
}

/// The flat search with the native entry's facts given directly (tests).
#[cfg(test)]
fn find_path_zoned_marker_inner(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    hierarchy_start: (u16, u16),
    hierarchy_goal: (u16, u16),
    native_zone_equal: Option<bool>,
    costs: Option<&TerrainCostGrid>,
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    mover_is_crusher: bool,
    is_infantry: bool,
    blocker_neighbor_counts: Option<&BlockerNeighborCounts>,
) -> Option<Vec<(u16, u16)>> {
    let entry = NativePathEntry {
        raw_equal: native_zone_equal,
        goal_bridge: false,
        hierarchy_start,
        hierarchy_goal,
        endpoints_in_playfield: true,
    };
    find_flat_path_after_entry(
        grid,
        start,
        goal,
        &entry,
        costs,
        zone_grid,
        mz,
        movement_zone,
        resolved_terrain,
        marker_overlay,
        MoverSearchFacts {
            urgency: 0,
            mover_is_crusher,
            is_infantry,
            foot_entry: None,
        },
        blocker_neighbor_counts,
    )
    .ok()
    .map(|steps| steps.into_iter().map(|step| (step.rx, step.ry)).collect())
}

fn find_flat_path_after_entry(
    grid: &PathGrid,
    start: (u16, u16),
    goal: (u16, u16),
    entry: &NativePathEntry,
    costs: Option<&TerrainCostGrid>,
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    facts: MoverSearchFacts<'_>,
    blocker_neighbor_counts: Option<&BlockerNeighborCounts>,
) -> Result<Vec<LayeredPathStep>, PathSearchFailure> {
    let goal_structural_bridge = resolved_terrain
        .and_then(|terrain| terrain.cell(goal.0, goal.1))
        .is_some_and(|cell| cell.bridge_facts.has_structural_bridge());
    let corridor = admit_zone_search(
        zone_grid,
        mz,
        movement_zone,
        resolved_terrain,
        blocker_neighbor_counts,
        &ZoneLadderQuery {
            start,
            start_layer: MovementLayer::Ground,
            goal,
            goal_layer: MovementLayer::Ground,
            fallback_goal_layer: if goal_structural_bridge {
                MovementLayer::Bridge
            } else {
                MovementLayer::Ground
            },
            entry,
        },
        facts.foot_entry,
    )?;
    //429C1A prepares the marker transaction only after42C900 has admitted
    //the search. Keep its one owner borrowed through the cell A*.
    let prepared = facts
        .foot_entry
        .map(|provider| provider.prepare_search_markers())
        .transpose()
        .map_err(PathSearchFailure::CellEntryUnavailable)?
        .flatten();
    let mut facts = facts;
    if let Some((urgency, _)) = prepared.as_ref() {
        facts.urgency = *urgency;
    }
    let marker_overlay = prepared
        .as_ref()
        .map(|(_, overlay)| &**overlay)
        .or(marker_overlay);
    find_path_with_costs_marker(
        grid,
        start,
        goal,
        costs,
        corridor.as_ref().map(HierarchyCorridor::gate),
        movement_zone,
        resolved_terrain,
        marker_overlay,
        facts,
    )
}

#[cfg(test)]
pub(crate) fn find_layered_path_zoned_marker(
    grid: &PathGrid,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    terrain_costs: Option<&TerrainCostGrid>,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    blocker_neighbor_counts: Option<&BlockerNeighborCounts>,
    mover_is_crusher: bool,
    is_infantry: bool,
    allow_zone_hierarchy: bool,
    playfield_bounds: Option<PlayfieldBounds>,
) -> Option<Vec<LayeredPathStep>> {
    find_layered_path_zoned_marker_detailed(
        grid,
        start,
        start_layer,
        goal,
        zone_grid,
        mz,
        terrain_costs,
        movement_zone,
        resolved_terrain,
        marker_overlay,
        blocker_neighbor_counts,
        MoverSearchFacts {
            urgency: 0,
            mover_is_crusher,
            is_infantry,
            foot_entry: None,
        },
        allow_zone_hierarchy,
        playfield_bounds,
    )
    .ok()
}

pub(crate) fn find_layered_path_zoned_marker_detailed(
    grid: &PathGrid,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    terrain_costs: Option<&TerrainCostGrid>,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
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
    let corridor = admit_zone_search(
        zone_grid,
        mz,
        movement_zone,
        resolved_terrain,
        blocker_neighbor_counts,
        &ZoneLadderQuery {
            start,
            start_layer: source_layer,
            goal,
            goal_layer,
            fallback_goal_layer: goal_layer,
            entry: &entry,
        },
        facts.foot_entry,
    )?;
    let prepared = facts
        .foot_entry
        .map(|provider| provider.prepare_search_markers())
        .transpose()
        .map_err(PathSearchFailure::CellEntryUnavailable)?
        .flatten();
    let mut facts = facts;
    if let Some((urgency, _)) = prepared.as_ref() {
        facts.urgency = *urgency;
    }
    let marker_overlay = prepared
        .as_ref()
        .map(|(_, overlay)| &**overlay)
        .or(marker_overlay);
    find_layered_path_marker(
        grid,
        start,
        start_layer,
        goal,
        terrain_costs,
        corridor.as_ref().map(HierarchyCorridor::gate),
        movement_zone,
        resolved_terrain,
        marker_overlay,
        facts,
    )
}

#[cfg(test)]
#[path = "zone_search_tests.rs"]
mod zone_search_tests;
