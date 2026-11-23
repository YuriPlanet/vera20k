//! The shared goal field and the corridor it names.
//!
//! The field is VERA's own structure; these tests pin its shape, not a
//! native behavior.

use super::{GoalField, PathGrid};

/// A grid of `width` by `height` cells, every cell walkable.
fn flat(width: u16, height: u16) -> PathGrid {
    PathGrid::test_all_passable(width, height)
}

#[test]
fn the_field_route_from_a_start_reaches_the_goal() {
    let grid = flat(5, 5);
    let field = GoalField::build(&grid, (4, 4)).expect("the goal is on the grid");
    let route = field
        .route_cells_from((0, 0))
        .expect("the start reaches the goal");
    assert_eq!(route.first(), Some(&(0, 0)));
    assert_eq!(route.last(), Some(&(4, 4)));
    // Every step of the route is one cell away from the one before it.
    for pair in route.windows(2) {
        let (x, y) = (pair[0].0 as i32, pair[0].1 as i32);
        let (nx, ny) = (pair[1].0 as i32, pair[1].1 as i32);
        assert!(
            (nx - x).abs() <= 1 && (ny - y).abs() <= 1 && (nx, ny) != (x, y),
            "the route steps from {pair:?}"
        );
    }
}

#[test]
fn the_field_route_is_the_shortest_one_in_steps() {
    let grid = flat(5, 5);
    let field = GoalField::build(&grid, (4, 4)).expect("the goal is on the grid");
    // Four steps east and four north is eight steps; the diagonal makes it
    // four.
    let route = field
        .route_cells_from((0, 0))
        .expect("the start reaches the goal");
    assert_eq!(route.len(), 5, "the diagonal route");
}

#[test]
fn a_cell_that_cannot_reach_the_goal_has_no_route() {
    let mut grid = flat(5, 5);
    // A wall across the middle, so the left half cannot reach the goal.
    for y in 0..5 {
        grid.block_structure_cell(2, y);
    }
    let field = GoalField::build(&grid, (4, 2)).expect("the goal is on the grid");
    assert!(
        field.route_cells_from((0, 2)).is_none(),
        "the wall cuts the route"
    );
    assert!(
        field.route_cells_from((3, 2)).is_some(),
        "the goal's side reaches it"
    );
}

#[test]
fn the_field_starts_from_a_goal_that_is_not_walkable() {
    let mut grid = flat(5, 5);
    // The goal is inside a building: the movers head for it and stop as
    // close as they can, so the field still starts there.
    grid.block_structure_cell(4, 4);
    let field = GoalField::build(&grid, (4, 4)).expect("the goal is on the grid");
    let route = field
        .route_cells_from((0, 0))
        .expect("the start reaches the goal");
    assert_eq!(route.last(), Some(&(4, 4)));
}

#[test]
fn the_start_that_is_the_goal_has_no_route() {
    let grid = flat(5, 5);
    let field = GoalField::build(&grid, (2, 2)).expect("the goal is on the grid");
    assert!(field.route_cells_from((2, 2)).is_none());
}

#[test]
fn a_goal_off_the_grid_has_no_field() {
    let grid = flat(5, 5);
    assert!(GoalField::build(&grid, (5, 0)).is_none());
    assert!(GoalField::build(&grid, (0, 9)).is_none());
}
