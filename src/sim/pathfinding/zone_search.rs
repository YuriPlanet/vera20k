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
//! 6. Retry budget is `maxSteps != -1 ? 1 : 5` (`0x0042CB8B..0x0042CBA1`),
//!    and both callers pass -1. A cell A* that fails with `allowHS` set
//!    (`0x0042CC13..0x0042CC19`) calls
//!    `PathfinderClass::UpdateHierarchicalEdges` @ `0x0042CCD0` and the reset
//!    `0x0042A5B0` (open lists emptied, search serial `+0x28` bumped), then
//!    re-reads `allowHS` from `PathfinderClass+0x38`. With tries left it
//!    re-runs `Zone_precheck`, whose failure ends the loop, or with `allowHS`
//!    clear the plain A*, which runs once.
//!
//! ## What VERA does, and the remaining recorded gaps
//!
//! The live route is faithful in shape: `zone_precheck_flat` on the level-0
//! hierarchy, then a hierarchy-marked cell A*, retried as point 6 says
//! ([`find_layered_path_after_entry`], [`precheck_retry`]); a precheck failure
//! with matching zones falls back to the plain A*, and one with differing
//! zones returns `None`. What is missing:
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
//! - **The retry keeps whole routes.** `Zone_precheck` writes each level's
//!   route into 500 words with no bound (`0x0042C8A0..0x0042C8CE`), level 0
//!   last, so a level-0 route over 500 zones overwrites level 1's, which a
//!   split at level 1 hands `InvalidateZoneEdge`. VERA keeps each route
//!   whole. Trigger: a failed corridor search after a precheck route of more
//!   than 500 level-0 zones, each within one 2x2 block, so a route winding
//!   for hundreds of cells. Effect: native may exclude other level-1 edges
//!   or clear `+0x38`. Frequency: unmeasured, rare at retail map sizes.
//!   [`super::core::HierarchyGate`] records the route-progress residuals.
//! - **Without resolved terrain there is no retry.** The flood reads map
//!   cells, so a failed corridor search without them ends with
//!   `CellEntryUnavailable`. Every loaded map has them; fixtures may not.
//! - **The JumpJet infantry precheck row is not ported.** For an Infantry
//!   (`WhatAmI` 0xF) whose type sets `JumpJet=` (`+0xD94` through `+0x6C0`),
//!   `0x0042CA43..0x0042CA69` hands `Zone_precheck` row 7 (Infantry) instead
//!   of the type's row, then reads its locomotor's `IPersist::GetClassID`
//!   (`0x0042CA7A..0x0042CAB5`) into a local nothing uses. VERA passes the
//!   type's row. Trigger: such an infantry reaching `FootClass::Find_Path`.
//!   No retail case does: Find_Path's callers are the Drive, Walk, Ship,
//!   Hover and Mech locomotors, and both retail `JumpJet=` infantry, the
//!   Rocketeer (`JUMPJET`) and the Cosmonaut (`LUNR`), move by the Jumpjet
//!   locomotor.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/zone_map, sim/pathfinding, sim/locomotor.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

pub(crate) mod native_path_entry;
mod precheck_retry;

use std::cell::Cell;

use super::{BlockerNeighborCounts, MoverSearchFacts, RouteProgress, SearchMarkerOverlay};

use super::zone_hierarchy::{
    MarkedZones, ZONE_PRECHECK_LEVELS, ZoneHierarchy, ZoneLevelGraph, ZonePrecheckExclusions,
    ZonePrecheckOutcome, ZonePrecheckResult, ZonePrecheckThreat, zone_precheck_flat,
};
use super::zone_map::{ZoneGrid, ZoneId};
use super::{HierarchyGate, LayeredPathStep, PathGrid, find_layered_path_marker};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::tube_facts::TubeSource;
use crate::rules::locomotor_type::MovementZone;
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::movement::locomotor::MovementLayer;
use crate::util::native_x87::NativeF64Bits;
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

/// Whether the path entry may answer reachability from the zone ladder before
/// running A*.
///
/// `AStar_pathfind_search` (`0x0042C900`) takes the row from the type's
/// `MovementZone=` (`+0x5B4`, `0x0042C963..0x0042C96D`), compares the raw
/// labels (`0x0042CB22`) and runs `Zone_precheck` (`0x0042C290`) for every
/// row, Water and WaterBeach included. Only the Foot's `+0x2CC` (`0x004D3810`,
/// `foot_path_zone_precheck`) skips a row: `-1`, at `0x004D3825`.
///
/// `Invalid` (`-1`) stays exempt, VERA-internal: past `+0x2CC`, `0x0042C900`
/// reads row `-1`, before its label and passability tables, which Rust does
/// not reproduce. Trigger: a type whose `MovementZone=` names no row; no
/// retail type has one. Effect: VERA runs the plain cell A*.
fn can_use_reduced_zone_precheck(movement_zone: Option<MovementZone>) -> bool {
    movement_zone != Some(MovementZone::Invalid)
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

/// The level-0 corridor a passed `Zone_precheck` hands the cell A*
/// (`AStar_main_loop @ 0x00429A90`'s last argument, `allowHS`), the routes
/// it chose and the search's progress along them.
struct HierarchyCorridor<'a> {
    level0_zones: &'a ZoneLevelGraph,
    marked_level0: MarkedZones,
    blocker_neighbor_counts: &'a BlockerNeighborCounts,
    /// Each level's route, start zone first (`PathfinderClass+0xBC`, 500
    /// words a level; lengths at `+0xC74`).
    routes: [Vec<ZoneId>; ZONE_PRECHECK_LEVELS],
    progress: Cell<RouteProgress>,
}

impl HierarchyCorridor<'_> {
    fn gate(&self) -> HierarchyGate<'_> {
        HierarchyGate {
            level0_zones: self.level0_zones,
            marked_level0: &self.marked_level0,
            blocker_neighbor_counts: self.blocker_neighbor_counts,
            level0_route: &self.routes[0],
            progress: &self.progress,
        }
    }
}

/// One search's `Zone_precheck` (`0x0042C290`, called at `0x0042CB58` and by
/// each retry at `0x0042CCB3`): the hierarchy, the projected endpoints'
/// level-0 zones, the row and the Foot's threat coefficient, all fixed for
/// the search.
struct CorridorPrecheck<'a> {
    zone_grid: &'a ZoneGrid,
    hierarchy: &'a ZoneHierarchy,
    level0_zones: &'a ZoneLevelGraph,
    blocker_neighbor_counts: &'a BlockerNeighborCounts,
    start_zone: ZoneId,
    goal_zone: ZoneId,
    movement_zone: MovementZone,
    threat_coefficient: NativeF64Bits,
}

impl<'a> CorridorPrecheck<'a> {
    /// The corridor the precheck marks for a cell search from `start` while
    /// `exclusions` hold, or `None` when it fails.
    fn run(
        &self,
        exclusions: &ZonePrecheckExclusions,
        foot_entry: &dyn super::SearchFootEntry,
        start: (u16, u16),
    ) -> Result<Option<HierarchyCorridor<'a>>, PathSearchFailure> {
        let padded_lookup = |index| foot_entry.hierarchy_house_threat(index);
        let threat = ZonePrecheckThreat::new(self.threat_coefficient, &padded_lookup);
        let outcome = zone_precheck_flat(
            self.hierarchy,
            self.start_zone,
            self.goal_zone,
            self.movement_zone,
            exclusions,
            Some(threat),
        )
        .map_err(PathSearchFailure::CellEntryUnavailable)?;
        let ZonePrecheckOutcome::Passed(ZonePrecheckResult {
            paths,
            marked: [marked_level0, ..],
            ..
        }) = outcome
        else {
            return Ok(None);
        };
        Ok(Some(HierarchyCorridor {
            level0_zones: self.level0_zones,
            marked_level0,
            blocker_neighbor_counts: self.blocker_neighbor_counts,
            routes: paths,
            progress: Cell::new(RouteProgress {
                index: 0,
                cell: start,
            }),
        }))
    }
}

/// The cell searches one path search may run. `AStar_pathfind_search`
/// allows `maxSteps != -1 ? 1 : 5` (`0x0042CB8B..0x0042CBA1`), and both its
/// callers pass -1 (`0x004CBC1E`, `0x00504D9B`), the limit the cell search
/// reads as [`super::MAX_SEARCH_NODES`]. `0x005048A0`, the second caller,
/// has no caller of its own and no port.
const CELL_SEARCH_TRIES: usize = 5;

/// The endpoints one search asks the zone ladder about: the mover's OnBridge
/// layer (`Foot+0x8C`, `0x0042C973`) and the goal's live `Flags & 0x100`
/// (`0x0042C9B7`).
struct ZoneLadderQuery<'e> {
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    goal_layer: MovementLayer,
    entry: &'e NativePathEntry,
}

/// Zone admission for one path search, after `prepare_native_path_entry`:
/// `Ok(None)` runs the plain cell A*, `Ok(Some(..))` the hierarchy-marked one
/// with the precheck its retries run again, and `Err` no cell A* at all.
fn admit_zone_search<'a>(
    zone_grid: Option<&'a ZoneGrid>,
    mz: MovementZone,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    blocker_neighbor_counts: Option<&'a BlockerNeighborCounts>,
    query: &ZoneLadderQuery<'_>,
    foot_entry: &dyn super::SearchFootEntry,
) -> Result<Option<(CorridorPrecheck<'a>, HierarchyCorridor<'a>)>, PathSearchFailure> {
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
            == zone_map.zone_at(query.goal.0, query.goal.1, query.goal_layer)
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
        let precheck = CorridorPrecheck {
            zone_grid: zg,
            hierarchy,
            level0_zones,
            blocker_neighbor_counts,
            start_zone,
            goal_zone,
            movement_zone: movement_zone.unwrap_or(mz),
            threat_coefficient: foot_entry
                .finishing_threat_coefficient()
                .map_err(PathSearchFailure::CellEntryUnavailable)?,
        };
        // The zones match, so a failed precheck clears allowHS and still
        // runs the cell A*. The exclusions start empty (0x42C912..0x42C925).
        return Ok(precheck
            .run(&ZonePrecheckExclusions::default(), foot_entry, query.start)?
            .map(|corridor| (precheck, corridor)));
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

/// 429C1A prepares the marker transaction only after 42C900 has admitted
/// the search; the receiver's transaction, else the caller's overlay, holds
/// through the cell A*. Returns the facts with the prepared urgency.
fn prepared_markers<'a>(
    facts: MoverSearchFacts<'a>,
) -> Result<
    (
        MoverSearchFacts<'a>,
        Option<std::cell::Ref<'a, SearchMarkerOverlay>>,
    ),
    PathSearchFailure,
> {
    let prepared = facts
        .foot_entry
        .prepare_search_markers()
        .map_err(PathSearchFailure::CellEntryUnavailable)?;
    Ok(match prepared {
        Some((urgency, overlay)) => (
            MoverSearchFacts {
                urgency,
                foot_entry: facts.foot_entry,
            },
            Some(overlay),
        ),
        None => (facts, None),
    })
}

/// [`find_layered_path_zoned_marker_detailed`] asking [`super::WalkableEntry`].
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn find_layered_path_zoned_marker(
    grid: &PathGrid,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    blocker_neighbor_counts: Option<&BlockerNeighborCounts>,
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
        movement_zone,
        resolved_terrain,
        marker_overlay,
        blocker_neighbor_counts,
        MoverSearchFacts {
            urgency: 0,
            foot_entry: &super::WalkableEntry::new(grid),
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
    // zone/hierarchy admission and runs the ordinary cell A*.
    let entry = prepare_native_path_entry(
        zone_grid,
        resolved_terrain,
        movement_zone.unwrap_or(mz),
        start,
        start_layer == MovementLayer::Bridge,
        goal,
        allow_zone_hierarchy,
        playfield_bounds,
    );
    find_layered_path_after_entry(
        grid,
        start,
        start_layer,
        goal,
        &entry,
        zone_grid,
        mz,
        movement_zone,
        resolved_terrain,
        marker_overlay,
        facts,
        blocker_neighbor_counts,
        playfield_bounds,
    )
}

/// The search after `prepare_native_path_entry`: the zone ladder over `entry`,
/// then the cell A* from `start_layer`, tried again while it fails
/// (`0x0042CB8B..0x0042CCCB`).
///
/// A failed corridor search runs `UpdateHierarchicalEdges` from its
/// progress cell, which excludes zone pairs or clears `+0x38`
/// ([`precheck_retry`]). With tries left, a cleared `+0x38` runs the plain
/// cell A* once more, whatever it returns (`0x0042CC9B` -> `0x0042CBA5`), and
/// otherwise `Zone_precheck` runs again under the exclusions; a failed one
/// ends the search. The exclusions, `+0x38` and the routes live for this one
/// search: native resets them as it starts (`0x0042C909`, `0x0042C912`) and
/// each passed precheck rewrites the routes. The reset `0x0042A5B0` after
/// each update (`0x0042CC80`) gives the next precheck and cell search fresh
/// marks, as each run here builds its own. The debug logs at `0x0042CBDE`
/// and `0x0042CC65` are not ported.
fn find_layered_path_after_entry(
    grid: &PathGrid,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    entry: &NativePathEntry,
    zone_grid: Option<&ZoneGrid>,
    mz: MovementZone,
    movement_zone: Option<MovementZone>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    facts: MoverSearchFacts<'_>,
    blocker_neighbor_counts: Option<&BlockerNeighborCounts>,
    playfield_bounds: Option<PlayfieldBounds>,
) -> Result<Vec<LayeredPathStep>, PathSearchFailure> {
    let layer = |bridge: bool| {
        if bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        }
    };
    let admitted = admit_zone_search(
        zone_grid,
        mz,
        movement_zone,
        resolved_terrain,
        blocker_neighbor_counts,
        &ZoneLadderQuery {
            start,
            start_layer: layer(start_layer == MovementLayer::Bridge),
            goal,
            goal_layer: layer(entry.goal_bridge),
            entry,
        },
        facts.foot_entry,
    )?;
    let (facts, prepared) = prepared_markers(facts)?;
    // Native builds and drops the marker transaction around each
    // AStar_main_loop (0x429C1A, then 0x42A42D or 0x42A44C). Nothing between
    // two tries changes what it reads, so the one transaction serves them all.
    let overlay = prepared.as_deref().or(marker_overlay);
    let search = |gate: Option<HierarchyGate<'_>>| {
        find_layered_path_marker(
            grid,
            start,
            start_layer,
            goal,
            gate,
            resolved_terrain,
            overlay,
            facts,
        )
    };
    let Some((precheck, mut corridor)) = admitted else {
        return search(None);
    };
    let mut exclusions = ZonePrecheckExclusions::default();
    let mut tries = 0;
    loop {
        match search(Some(corridor.gate())) {
            Err(PathSearchFailure::CellSearchExhausted) => {}
            found => return found,
        }
        tries += 1;
        let retry = precheck_retry::RetryInputs {
            zone_grid: precheck.zone_grid,
            hierarchy: precheck.hierarchy,
            terrain: resolved_terrain.ok_or_else(|| {
                PathSearchFailure::CellEntryUnavailable(
                    "the hierarchy retry floods map cells".into(),
                )
            })?,
            bounds: playfield_bounds,
            movement_zone: precheck.movement_zone,
            foot_entry: facts.foot_entry,
        };
        let hierarchy_usable = precheck_retry::update_hierarchical_edges(
            &retry,
            &corridor.routes,
            corridor.progress.get().cell,
            &mut exclusions,
        )?;
        if tries >= CELL_SEARCH_TRIES {
            return Err(PathSearchFailure::CellSearchExhausted);
        }
        if !hierarchy_usable {
            return search(None);
        }
        corridor = precheck
            .run(&exclusions, facts.foot_entry, start)?
            .ok_or(PathSearchFailure::CellSearchExhausted)?;
    }
}

#[cfg(test)]
#[path = "zone_search_tests.rs"]
mod zone_search_tests;
