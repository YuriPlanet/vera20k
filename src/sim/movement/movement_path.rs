//! Movement path management — path computation, repath-after-block, and bridge pathing support.
//!
//! Wraps the A* pathfinder for use by the movement tick: computes initial paths,
//! retries after blockages through the zone-aware search, and determines whether
//! an entity's locomotor supports layered bridge pathing.

use crate::rules::locomotor_type::{LocomotorKind, MovementZone};
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
#[path = "path_finishing.rs"]
mod path_finishing;
use crate::sim::pathfinding::terrain_cost::TerrainCostGrid;
use crate::sim::pathfinding::zone_search;
use crate::sim::pathfinding::{
    LayeredPath, MAX_PATH_SEGMENT_STEPS, PathGrid, SearchMarkerOverlay, truncate_layered_path,
};

use super::{MoverPathFacts, PathfindingContext};

#[cfg(test)]
pub(crate) fn reset_path_search_used_zone_grid_marker() {
    PATH_SEARCH_USED_ZONE_GRID.with(|used| used.set(false));
}

#[cfg(test)]
pub(crate) fn path_search_used_zone_grid_marker() -> bool {
    PATH_SEARCH_USED_ZONE_GRID.with(|used| used.get())
}

#[cfg(test)]
thread_local! {
    static PATH_SEARCH_USED_ZONE_GRID: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// **VERA-internal, gamemd equivalent UNCHECKED**: the locomotor whitelist and
/// the zero-size grid sentinel below. `FootClass::Find_Path` @ `0x004D3920`
/// gates on `vtable+0x2CC`, not on locomotor kind.
///
/// **Hover was added 2026-08-27 (matrix row T1-01).** The exclusion was the
/// whole of that defect: a Hover mover never reached the layered builder, so
/// `build_flat_fallback_layers` stamped `Ground` on every node of its path
/// including the deck ones, and the crossing loop's terrain test then read the
/// deck cell's raw ground-walkability — the riverbed under the span — and
/// refused. The order was never dropped, so a Robot Tank ordered across a high
/// bridge accelerated into the abutment and was snapped back to cell centre
/// about thirteen times a second, indefinitely. The bridge legality check
/// itself passed every time; it was never consulted about the right plane.
/// Measured on BayOPigs.mmx: seven refusals, every one `layer_walkable`, zero
/// `bridge_traversal`.
///
/// Stock YR gives the Hover CLSID to `[LCRF]`, `[ROBO]`, `[SAPC]` and `[YHVR]`
/// — the Robot Tank and all three amphibious transports. (`ROBO` also carries
/// `TooBigToFitUnderBridge=true`, but that flag gates nothing in movement:
/// it is draw-pipeline-only (see `ObjectType::too_big_to_fit_under_bridge`)
/// and `Can_Enter_Cell` never reads it. It is not why a Robot Tank
/// could not use a span, and it did not stop one driving under one either.)
///
/// **What the one native gate on this path actually does.** VERIFIED 2026-08-27
/// by direct reads, with one stated premise rather than a read:
///   - *Premise (not a Ghidra read):* the movers this predicate admits are all
///     `[VehicleTypes]` — `ROBO`, `LCRF`, `SAPC`, `YHVR` alongside `MTNK`,
///     checked in `ini/rulesmd.ini` — hence `UnitClass` instances. That is what
///     makes the vtable read below the same slot the `CALL [EDX + 0x2CC]`
///     dispatches through for them: the read is at a fixed vtable, the call is
///     through the mover's own. Named because reading an offset is not the same
///     as proving a slot, and conflating the two has produced a wrong label in
///     this project before.
///   - `UnitClass`'s vtable is `0x007F5C70` — installed by
///     `UnitClass::Constructor` @ `0x0073543A`, and by its destructor and
///     `Load`. Identity rests on that existing Ghidra label; **UNCHECKED**
///     against RTTI/COL, which is the reliable route here.
///   - `0x007F5C70 + 0x2CC` = `0x007F5F3C`, which holds `0x004D3810`. Slot read
///     directly; not inferred from a callee offset, which is the trap that has
///     produced a wrong label in this project before.
///   - `FootClass::Find_Path` calls that slot at `0x004D397F`
///     (`CALL dword ptr [EDX + 0x2CC]`), and on a false return takes the
///     clear-and-return-0 exit at `0x004D3989`.
///   - `0x004D3810` (`CanReachDestination`) reads the type class through
///     `vtable+0x84`, takes `TechnoTypeClass+0x5B4` (**MovementZone**), returns
///     1 at once when that is `-1`, and otherwise tail-calls
///     `MapClass::Can_Reach_Zone` with it.
///
/// So the gate is a zone **reachability abort** — should the search run at all —
/// keyed on MovementZone, with no locomotor term. **Scoped claim:** at this
/// gate, nothing selects a pathing plane by locomotor kind. Whether anything
/// elsewhere in the binary does is UNCHECKED, and this comment does not assert
/// a binary-wide negative.
///
/// That is enough, because the change here **deletes** a VERA gate rather than
/// adding one: removing a restriction needs the absence of a verified native
/// gate demanding it, not an affirmative native proof. The fix's positive
/// evidence is production — two ordinary undisabled crossings on two retail
/// maps with different span axes.
///
/// Do NOT reason "same vtable slot, therefore same answer": a shared virtual
/// that reads type data answers differently per type, and this one does exactly
/// that. `ROBO` is `MovementZone=AmphibiousDestroyer` and `MTNK` is `Normal`, so
/// `0x004D3810` genuinely can separate them — it just separates them by zone,
/// not by locomotor, and never by pathing plane. An earlier draft of this
/// comment argued from `[VehicleTypes]` membership to a shared slot value to a
/// shared answer; the middle step is true and the last does not follow.
///
/// `Can_Enter_Cell` @ `0x0073F0A0`, `CheckBridgeTraversal` @ `0x004D9C60` and
/// the `ILocomotion+0x1C` slot are **UNCHECKED here** — reported as corroboration
/// that the downstream legality path is locomotor-agnostic, not re-read at this
/// callsite, and not the gate. Do not cite them as if they resolved `+0x2CC`.
///
/// The remaining `Drive | Walk | Hover` list stays VERA-internal. The other
/// five kinds — Ship, Fly, Teleport, Jumpjet and Rocket — are excluded because
/// admitting them is a separate question with its own blast radius, not because
/// the gate above excludes them; it excludes nothing by kind.
pub(super) fn supports_layered_bridge_pathing(
    loco: &LocomotorState,
    grid: &PathGrid,
    on_bridge: bool,
) -> bool {
    if grid.width() == 0 || grid.height() == 0 {
        return false;
    }
    matches!(
        loco.kind,
        LocomotorKind::Drive | LocomotorKind::Walk | LocomotorKind::Hover
    ) || on_bridge
}

fn is_bridge_layer_walkable(grid: Option<&PathGrid>, cell: (u16, u16)) -> bool {
    grid.is_some_and(|g| g.is_walkable_on_layer(cell.0, cell.1, MovementLayer::Bridge))
}

/// **VERA-internal, gamemd has no equivalent.** A goal reachable only on the
/// bridge layer fails the flat `find_move_path` before any A*.
/// `FootClass::Find_Path` @ `0x004D3920` has exactly **one** clear-and-return-0
/// exit — `vtable+0x2CC` at `0x004D3989`, whose tail is
/// `POP/POP/POP; ADD ESP,0x1F9C; RET 0xC` at `0x004D3993`. Its other two
/// `MOV [EBP+0x5E0],-1` sites are not failures: `0x004D393A` is the entry
/// `prefix == 0` clear, and `0x004D3E22` clears on the preserved-prefix length
/// and then falls straight into `FootClass::Run_AStar` at `0x004D3E4B`. Nothing
/// in that head gates the search on a bridge layer — Find_Path does read
/// `Cell+0x140 & 0x100` in its *failure* tail, when it decides scatter and
/// team-removal, but that is after the search, not instead of it.
///
/// Trigger: a mover outside [`supports_layered_bridge_pathing`] searching to a
/// cell whose ground plane is impassable but whose bridge deck is walkable;
/// Drive, Walk and Hover movers take the layered branch and never reach it.
/// Player effect: VERA's search fails where retail runs it. Downstream risk:
/// removing it re-opens the layered-goal case the bridge-parity boundary at
/// the top of this module records, so it must land with that work.
fn is_bridge_only_goal(grid: &PathGrid, goal: (u16, u16)) -> bool {
    !grid.is_walkable(goal.0, goal.1) && is_bridge_layer_walkable(Some(grid), goal)
}

/// Internal request failures retain the layer where the return originated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MovePathFailure {
    MissingGrid,
    BridgeOnlyGoal,
    Search(zone_search::PathSearchFailure),
}

pub(super) fn find_move_path(
    ctx: PathfindingContext<'_>,
    layered_pathing: bool,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    terrain_costs: Option<&TerrainCostGrid>,
    zone_mz: MovementZone,
    movement_zone: Option<MovementZone>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    facts: MoverPathFacts,
    allow_zone_hierarchy: bool,
    foot_entry: Option<&dyn crate::sim::pathfinding::SearchFootEntry>,
) -> Result<LayeredPath, MovePathFailure> {
    let grid = ctx.path_grid.ok_or(MovePathFailure::MissingGrid)?;
    let zone_grid = ctx.zone_grid;
    #[cfg(test)]
    if zone_grid.is_some() {
        PATH_SEARCH_USED_ZONE_GRID.with(|used| used.set(true));
    }
    let resolved_terrain = ctx.resolved_terrain;
    if layered_pathing {
        let layered_result = zone_search::find_layered_path_zoned_marker_detailed(
            grid,
            start,
            start_layer,
            goal,
            zone_grid,
            zone_mz,
            terrain_costs,
            movement_zone,
            resolved_terrain,
            marker_overlay,
            ctx.blocker_neighbor_counts,
            crate::sim::pathfinding::MoverSearchFacts {
                urgency: facts.urgency,
                mover_is_crusher: facts.mover_is_crusher,
                is_infantry: facts.is_infantry,
                foot_entry,
            },
            allow_zone_hierarchy,
            ctx.playfield_bounds,
        );
        let path = layered_result.map_err(MovePathFailure::Search)?;

        log::trace!(
            "find_move_path: layered A* succeeded ({:?}→{:?}), {} steps",
            start,
            goal,
            path.len(),
        );
        let (coords, layers) = path_finishing::finish(
            path,
            ctx,
            grid,
            foot_entry,
            terrain_costs,
            movement_zone,
            facts,
            marker_overlay,
        )
        .map_err(|cause| {
            MovePathFailure::Search(zone_search::PathSearchFailure::CellEntryUnavailable(cause))
        })?;
        let (coords, layers) = truncate_layered_path(coords, layers, MAX_PATH_SEGMENT_STEPS);
        return Ok((coords, layers));
    }

    if is_bridge_only_goal(grid, goal) {
        return Err(MovePathFailure::BridgeOnlyGoal);
    }

    let path = zone_search::find_path_zoned_marker_detailed(
        grid,
        start,
        goal,
        terrain_costs,
        zone_grid,
        zone_mz,
        movement_zone,
        resolved_terrain,
        marker_overlay,
        ctx.blocker_neighbor_counts,
        crate::sim::pathfinding::MoverSearchFacts {
            urgency: facts.urgency,
            mover_is_crusher: facts.mover_is_crusher,
            is_infantry: facts.is_infantry,
            foot_entry,
        },
        allow_zone_hierarchy,
        ctx.playfield_bounds,
    )
    .map_err(MovePathFailure::Search)?;

    let (path, path_layers) = path_finishing::finish(
        path,
        ctx,
        grid,
        foot_entry,
        terrain_costs,
        movement_zone,
        facts,
        marker_overlay,
    )
    .map_err(|cause| {
        MovePathFailure::Search(zone_search::PathSearchFailure::CellEntryUnavailable(cause))
    })?;
    Ok(truncate_layered_path(
        path,
        path_layers,
        MAX_PATH_SEGMENT_STEPS,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{
        ResolvedTerrainCell, ResolvedTerrainGrid, YR_CELL_LAND_TUNNEL,
    };
    use crate::map::tube_facts::{TubeFact, TubeId};
    use crate::sim::pathfinding::zone_map::ZoneGrid;

    fn make_resolved_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
        }
    }

    #[test]
    fn path_failure_detail_distinguishes_missing_grid_from_executed_search() {
        use crate::sim::pathfinding::zone_search::PathSearchFailure;
        fn search(grid: Option<&PathGrid>) -> Result<LayeredPath, MovePathFailure> {
            find_move_path(
                PathfindingContext {
                    path_grid: grid,
                    zone_grid: None,
                    resolved_terrain: None,
                    playfield_bounds: None,
                    blocker_neighbor_counts: None,
                },
                false,
                (0, 1),
                MovementLayer::Ground,
                (4, 1),
                None,
                MovementZone::Normal,
                Some(MovementZone::Normal),
                None,
                super::MoverPathFacts::fixture(0, false, true),
                false,
                None,
            )
        }
        assert_eq!(search(None), Err(MovePathFailure::MissingGrid));
        let mut grid = PathGrid::test_all_passable(5, 3);
        let (path, layers) = search(Some(&grid)).expect("open cell search");
        assert_eq!(path.first(), Some(&(0, 1)));
        assert_eq!(path.last(), Some(&(4, 1)));
        assert_eq!(layers, vec![MovementLayer::Ground; path.len()]);
        for y in 0..3 {
            grid.set_blocked(2, y, true);
        }
        assert_eq!(
            search(Some(&grid)),
            Err(MovePathFailure::Search(
                PathSearchFailure::CellSearchExhausted
            ))
        );
    }

    #[test]
    fn explicit_tube_path_survives_zone_precheck_and_smoothing() {
        let mut cells: Vec<_> = (0..5).map(|x| make_resolved_cell(x, 0)).collect();
        cells[0].yr_cell_land_type = YR_CELL_LAND_TUNNEL;
        cells[0].tube_index = Some(TubeId(0));
        for cell in &mut cells[1..4] {
            cell.ground_walk_blocked = true;
            cell.base_ground_walk_blocked = true;
        }
        let terrain = ResolvedTerrainGrid::from_cells_with_tubes(
            5,
            1,
            cells,
            vec![TubeFact::explicit((0, 0), (4, 0), 2, vec![2, 2, 2, 2])],
        );
        let grid = PathGrid::from_resolved_terrain(&terrain);
        let zone_grid = ZoneGrid::build_with_terrain(&grid, &terrain, &[], 5, 1);

        let (path, layers) = find_move_path(
            PathfindingContext {
                path_grid: Some(&grid),
                zone_grid: Some(&zone_grid),
                resolved_terrain: Some(&terrain),
                playfield_bounds: None,
                blocker_neighbor_counts: None,
            },
            false,
            (0, 0),
            MovementLayer::Ground,
            (4, 0),
            None,
            MovementZone::Normal,
            Some(MovementZone::Normal),
            None,
            super::MoverPathFacts::fixture(0, false, false),
            true,
            None,
        )
        .expect("movement path should use explicit tube despite disconnected zones");

        assert_eq!(path, vec![(0, 0), (4, 0)]);
        assert_eq!(layers, vec![MovementLayer::Ground, MovementLayer::Ground]);
    }

    #[test]
    fn move_path_marker_overlay_survives_path_smoothing() {
        let grid = PathGrid::test_all_passable(5, 3);
        let mut marker_overlay = SearchMarkerOverlay::new();
        marker_overlay.toggle((1, 1));
        marker_overlay.toggle((2, 1));
        marker_overlay.toggle((3, 1));

        let (path, layers) = find_move_path(
            PathfindingContext {
                path_grid: Some(&grid),
                zone_grid: None,
                resolved_terrain: None,
                playfield_bounds: None,
                blocker_neighbor_counts: None,
            },
            false,
            (0, 1),
            MovementLayer::Ground,
            (4, 1),
            None,
            MovementZone::Normal,
            Some(MovementZone::Normal),
            Some(&marker_overlay),
            super::MoverPathFacts::fixture(0, false, false),
            true,
            None,
        )
        .expect("marker overlay should still allow a path");

        assert_eq!(path.first().copied(), Some((0, 1)));
        assert_eq!(path.last().copied(), Some((4, 1)));
        assert!(
            !path
                .iter()
                .any(|cell| matches!(cell, (1, 1) | (2, 1) | (3, 1))),
            "path smoothing must not collapse the marker-avoiding route back through {:?}",
            path
        );
        assert_eq!(path.len(), layers.len());
    }

    #[test]
    fn gsi_04_12_layered_failure_does_not_retry_flat_ground_path() {
        let mut grid = PathGrid::test_all_passable(3, 1);
        // Start on a non-transition bridge deck. Native layered A* cannot
        // descend directly to the ground-only neighbor, while a separate flat
        // search from the same coordinate would incorrectly find (0..=2, 0).
        grid.set_cell_for_test(0, 0, 0, true, false);
        assert!(grid.is_walkable(2, 0), "goal must remain ground-walkable");

        let flat = find_move_path(
            PathfindingContext {
                path_grid: Some(&grid),
                zone_grid: None,
                resolved_terrain: None,
                playfield_bounds: None,
                blocker_neighbor_counts: None,
            },
            false,
            (0, 0),
            MovementLayer::Ground,
            (2, 0),
            None,
            MovementZone::Normal,
            Some(MovementZone::Normal),
            None,
            super::MoverPathFacts::fixture(0, false, false),
            true,
            None,
        )
        .expect("fixture must prove the removed ground-only retry could succeed");
        assert_eq!(flat.0.last().copied(), Some((2, 0)));

        let layered = find_move_path(
            PathfindingContext {
                path_grid: Some(&grid),
                zone_grid: None,
                resolved_terrain: None,
                playfield_bounds: None,
                blocker_neighbor_counts: None,
            },
            true,
            (0, 0),
            MovementLayer::Bridge,
            (2, 0),
            None,
            MovementZone::Normal,
            Some(MovementZone::Normal),
            None,
            super::MoverPathFacts::fixture(0, false, false),
            true,
            None,
        );

        assert!(
            layered.is_err(),
            "a failed layered search must remain failed instead of launching a second flat A*"
        );
    }

    /// A wall line that **disconnects** the map is refused before the live
    /// Foot `+0x1AC` receiver classifies any cell, even when it would price the
    /// wall for a mover that can shoot it.
    ///
    /// `zone_search` compares the start and goal base zone labels and returns
    /// `Err` without calling `astar_search`; a non-crushable `Wall=yes` reduces
    /// to `zone_class::WALL`, which the zone flood fill treats as impassable, so
    /// a line across the only route puts the endpoints in different zones.
    /// Native rejects on the same comparison and under the same condition:
    /// `0x0042CB2A CMP EAX,EDX` / `0x0042CB30 JZ` (labels equal, continue) /
    /// `0x0042CB34 JZ 0x0042CB8B` (hierarchy unusable, fall through) /
    /// `0x0042CB39 XOR EAX,EAX; RET`. VERA guards its own rejection on
    /// `hierarchy_counts_available` and a live level-0 hierarchy, which is the
    /// same shape. Pricing (`astar_entry_tests`) only matters for a wall line
    /// the zone map can route around.
    #[test]
    fn a_wall_line_that_disconnects_the_map_is_refused_before_the_cost_class_runs() {
        use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
        use crate::rules::locomotor_type::SpeedType;
        use crate::sim::pathfinding::{SearchEntryCandidate, SearchEntryQuery, SearchFootEntry};

        let mut cells = Vec::with_capacity(15);
        for ry in 0..3u16 {
            for rx in 0..5u16 {
                let mut cell = ResolvedTerrainCell::clear_for_test(rx, ry);
                cell.speed_costs.track = Some(100);
                if rx == 2 {
                    cell.zone_type = zone_class::WALL;
                    cell.overlay_zone_type = Some(zone_class::WALL);
                    cell.overlay_blocks = true;
                }
                cells.push(cell);
            }
        }
        let terrain = ResolvedTerrainGrid::from_cells(5, 3, cells);
        let grid = PathGrid::from_resolved_terrain(&terrain);
        let costs = TerrainCostGrid::from_resolved_terrain(&terrain, SpeedType::Track);
        let zone_grid = ZoneGrid::build_with_terrain(&grid, &terrain, &[], 5, 3);
        let counts = crate::sim::pathfinding::BlockerNeighborCounts::new(5, 3);

        // The receiver of an armed mover whose warhead has `Wall=`: 5 for the
        // unowned wall line, clear elsewhere. It counts every question.
        struct WallLine(std::cell::Cell<usize>);
        impl SearchFootEntry for WallLine {
            fn classify(&self, query: SearchEntryQuery) -> Result<u8, String> {
                self.0.set(self.0.get() + 1);
                Ok(match query.candidate {
                    SearchEntryCandidate::CopiedCoord((2, _)) => 5,
                    _ => 0,
                })
            }
        }
        let entry = WallLine(std::cell::Cell::new(0));
        let path = find_move_path(
            PathfindingContext {
                path_grid: Some(&grid),
                zone_grid: Some(&zone_grid),
                resolved_terrain: Some(&terrain),
                playfield_bounds: None,
                blocker_neighbor_counts: Some(&counts),
            },
            false,
            (0, 1),
            MovementLayer::Ground,
            (4, 1),
            Some(&costs),
            MovementZone::Normal,
            Some(MovementZone::Normal),
            None,
            super::MoverPathFacts::fixture(0, false, true),
            true,
            Some(&entry),
        );
        assert!(
            path.is_err(),
            "a wall line that splits the map is refused by the zone comparison: {path:?}"
        );
        assert_eq!(
            entry.0.get(),
            0,
            "the zone comparison refuses before any cell is classified"
        );
    }
}
