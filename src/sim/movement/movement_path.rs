//! Movement path management — the Foot's core path search for the movement
//! tick.
//!
//! Every `FootClass::Find_Path` (`0x004D3920`) reaches one core search, for
//! every Foot class: `FootClass::Run_AStar` (`0x004CBBA0`), then
//! `AStar_pathfind_search` (`0x0042C900`) from the Foot's OnBridge layer.
//! [`find_move_path`] runs it and the native finishing passes.

use crate::rules::locomotor_type::MovementZone;
use crate::sim::movement::locomotor::MovementLayer;
#[path = "path_finishing.rs"]
mod path_finishing;
use crate::sim::pathfinding::zone_search;
use crate::sim::pathfinding::{LayeredPath, SearchMarkerOverlay};

use super::PathfindingContext;

/// Internal request failures retain the layer where the return originated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MovePathFailure {
    MissingGrid,
    Search(zone_search::PathSearchFailure),
}

/// The core search of `FootClass::Find_Path` (`0x004D3920`) for every Foot
/// class: `FootClass::Run_AStar` (`0x004CBBA0`, called only at `0x004D3E4B`)
/// runs `AStar_pathfind_search` (`0x0042C900`) once, from `start_layer`, the
/// Foot's OnBridge layer (`+0x8C`). Nothing on that chain picks a search by
/// locomotor. The search's own class tests are the Aircraft heights
/// (`0x00429AF9`, `0x00429B2A`), the JumpJet infantry precheck row
/// (`0x0042CA43`) and the A*'s Passive Unit test (`0x00429D2F`, ported as
/// `is_passive_unit`). Neither an Aircraft nor a JumpJet infantry reaches
/// Find_Path in retail (`astar_search` and `zone_search` record both).
pub(super) fn find_move_path(
    ctx: PathfindingContext<'_>,
    start: (u16, u16),
    start_layer: MovementLayer,
    goal: (u16, u16),
    zone_mz: MovementZone,
    movement_zone: Option<MovementZone>,
    marker_overlay: Option<&SearchMarkerOverlay>,
    urgency: u8,
    allow_zone_hierarchy: bool,
    foot_entry: &dyn crate::sim::pathfinding::SearchFootEntry,
) -> Result<LayeredPath, MovePathFailure> {
    let facts = crate::sim::pathfinding::MoverSearchFacts {
        urgency,
        foot_entry,
    };
    let grid = ctx.path_grid.ok_or(MovePathFailure::MissingGrid)?;
    let zone_grid = ctx.zone_grid;
    let path = zone_search::find_layered_path_zoned_marker_detailed(
        grid,
        start,
        start_layer,
        goal,
        zone_grid,
        zone_mz,
        movement_zone,
        ctx.resolved_terrain,
        marker_overlay,
        ctx.blocker_neighbor_counts,
        facts,
        allow_zone_hierarchy,
        ctx.playfield_bounds,
    )
    .map_err(MovePathFailure::Search)?;
    log::trace!(
        "find_move_path: A* succeeded ({:?}→{:?}), {} steps",
        start,
        goal,
        path.len(),
    );
    // The whole route: Find_Path's copy (`FootPathQueue::install`) owns the
    // window.
    path_finishing::finish(path, ctx, grid, foot_entry, marker_overlay).map_err(|cause| {
        MovePathFailure::Search(zone_search::PathSearchFailure::CellEntryUnavailable(cause))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::sim::pathfinding::{PathGrid, SearchEntryQuery, SearchFootEntry};

    #[test]
    fn gsi_04_12_layered_failure_does_not_retry_flat_ground_path() {
        // Foot `0x004D9C60` refuses a step from deck height down to a plain
        // ground neighbour (a height difference of four without the target's
        // `0x300` flags). This receiver answers that refusal for every step
        // from a nonzero height and admits the rest.
        struct DeckStepRefused;
        impl SearchFootEntry for DeckStepRefused {
            fn classify(&self, query: SearchEntryQuery) -> Result<u8, String> {
                Ok(if query.path_height == 0 { 0 } else { 7 })
            }
            fn finishing_threat_coefficient(
                &self,
            ) -> Result<crate::util::native_x87::NativeF64Bits, String> {
                Ok(crate::util::native_x87::NativeF64Bits::POSITIVE_ZERO)
            }
            fn finishing_house_threat(&self, _coord: (i16, i16)) -> Result<i32, String> {
                Ok(0)
            }
        }
        let mut grid = PathGrid::test_all_passable(3, 1);
        // Start on a non-transition bridge deck. The A* from the deck cannot
        // descend directly to the ground-only neighbor, while a search from
        // the ground plane of the same coordinate finds (0..=2, 0).
        grid.set_cell_for_test(0, 0, 0, true, false);
        assert!(grid.is_walkable(2, 0), "goal must remain ground-walkable");

        let ground = find_move_path(
            PathfindingContext {
                path_grid: Some(&grid),
                zone_grid: None,
                resolved_terrain: None,
                playfield_bounds: None,
                blocker_neighbor_counts: None,
            },
            (0, 0),
            MovementLayer::Ground,
            (2, 0),
            MovementZone::Normal,
            Some(MovementZone::Normal),
            None,
            0,
            true,
            &DeckStepRefused,
        )
        .expect("fixture must prove a ground-plane search could succeed");
        assert_eq!(ground.0.last().copied(), Some((2, 0)));

        let deck = find_move_path(
            PathfindingContext {
                path_grid: Some(&grid),
                zone_grid: None,
                resolved_terrain: None,
                playfield_bounds: None,
                blocker_neighbor_counts: None,
            },
            (0, 0),
            MovementLayer::Bridge,
            (2, 0),
            MovementZone::Normal,
            Some(MovementZone::Normal),
            None,
            0,
            true,
            &DeckStepRefused,
        );

        assert!(
            deck.is_err(),
            "a failed deck search must remain failed instead of launching a second ground A*"
        );
    }

    /// `FootClass::Find_Path` (`0x004D3920`) reaches one core search for every
    /// Foot class, a Ship's included (`0x006A1EF3`): `0x004CBBA0`, then
    /// `AStar_pathfind_search` (`0x0042C900`), whose only gate before
    /// `AStar_main_loop` (`0x00429A90`) is its zone ladder. Nothing refuses a
    /// goal by its cell planes, so a Ship whose goal's ground is closed under a
    /// walkable deck (a structure under the span) is answered by the cell A*.
    /// That starts at the Foot's OnBridge height (`+0x8C`, the cell level off a
    /// bridge) and, for a non-aircraft, wants a `0x100` goal at `Level + 4`
    /// (`0x00429AF9..0x00429B23`), which a surface mover never reaches.
    #[test]
    fn a_ship_goal_under_a_deck_is_answered_by_the_cell_astar() {
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::components::DriveCoord;
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::movement::locomotor::LocomotorState;
        use crate::sim::pathfinding::zone_search::PathSearchFailure;
        use crate::sim::pathfinding::{BlockerNeighborCounts, SearchEntryCandidate, WalkableEntry};

        /// The ground-plane receiver, recording the heights the A* asks
        /// about the goal at.
        struct GoalWitness<'a> {
            ground: WalkableEntry<'a>,
            goal_heights: std::cell::RefCell<Vec<i32>>,
        }
        impl SearchFootEntry for GoalWitness<'_> {
            fn classify(&self, query: SearchEntryQuery) -> Result<u8, String> {
                if query.candidate == SearchEntryCandidate::CopiedCoord((2, 0)) {
                    self.goal_heights.borrow_mut().push(query.path_height);
                }
                self.ground.classify(query)
            }
        }

        // Level-0 surface; (2, 0) carries a deck at level 4 over a closed
        // ground plane.
        let mut grid = PathGrid::test_all_passable(3, 1);
        grid.set_cell_for_test(2, 0, 0, true, false);
        grid.set_blocked(2, 0, true);
        let mut entities = EntityStore::new();
        let mut ship = GameEntity::test_default(1, "DEST", "Americans", 0, 0);
        ship.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
        let id = entities.insert(ship);
        let goal = DriveCoord::cell(2, 0, 0);
        let request =
            super::super::movement_tick::FootPathRequest::track(&entities, id, goal, 0, None)
                .expect("live Ship");
        let witness = GoalWitness {
            ground: WalkableEntry::new(&grid),
            goal_heights: Default::default(),
        };
        let counts = BlockerNeighborCounts::new(3, 1);
        let found = request.search(
            goal,
            &entities,
            PathfindingContext {
                path_grid: Some(&grid),
                zone_grid: None,
                resolved_terrain: None,
                playfield_bounds: None,
                blocker_neighbor_counts: Some(&counts),
            },
            &witness,
        );

        assert_eq!(
            found,
            Err(MovePathFailure::Search(
                PathSearchFailure::CellSearchExhausted
            )),
            "the cell A* answers the Ship, not a refusal before it"
        );
        assert_eq!(
            *witness.goal_heights.borrow(),
            vec![0],
            "the A* asks about the goal once, at the Ship's surface height"
        );
    }
}
