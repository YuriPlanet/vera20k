//! A shared reverse field over the map's ground layer, for one goal cell.
//!
//! VERA-only: gamemd has no such structure. `AStar_pathfind_search`
//! (`0x0042C900`) runs one cell A* per Foot, and the path batch
//! ([`crate::sim::movement::path_batch`]) runs one per registration, so a
//! mass order of six hundred units onto one cell runs six hundred searches
//! toward the same goal.
//!
//! The field is the reverse of that: one breadth-first search from the goal
//! over the path grid's ground plane, giving every cell the step that
//! shortens its way to the goal. Every mover heading for that goal shares
//! it, and each one's path is then a walk down the field's gradient instead
//! of a search of its own. The mover's own admission still decides every
//! cell of that walk: the field is a guide, not a route, and a cell the
//! mover may not enter sends the search back to the stock one
//! ([`crate::sim::pathfinding::core`]).
//!
//! The field's plane is the path grid's ground walkability, which is VERA's
//! own ([`PathGrid`] has no native counterpart): it knows rock, water,
//! overlays and terrain objects, but not the mover's SpeedType row, and not
//! the units standing in the cells. So the walk the field gives is the
//! shortest one in steps over the ground, which is not always the route the
//! mover would have searched for.
//!
//! The step cost is one per cell, the class-0 base of the native cost table
//! (`CLASS_BASE`, `0x0081870C`), so the field's route is the shortest one in
//! steps over its own plane.

use std::collections::VecDeque;

use crate::util::direction::DIRECTION_DELTAS;

use super::PathGrid;

/// A cell with no step toward the goal: it does not reach it.
const NO_STEP: u8 = u8::MAX;

/// The reverse field of one goal cell: every cell's step toward the goal.
///
/// The field is built once per goal and read by every mover that shares it,
/// so it holds no per-mover state.
pub struct GoalField {
    width: usize,
    height: usize,
    /// The goal cell's index.
    goal: usize,
    /// Each cell's next step toward the goal, as a direction index into
    /// [`DIRECTION_DELTAS`]; [`NO_STEP`] when the cell does not reach the
    /// goal.
    step: Vec<u8>,
    /// The cells the build reached: the ground plane's walkable cells that
    /// reach the goal.
    reached: usize,
}

impl std::fmt::Debug for GoalField {
    /// The field's own plane is a byte per cell, so the summary names the
    /// map and the goal instead of printing it.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GoalField")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("goal", &self.goal)
            .field("reached", &self.reached)
            .finish()
    }
}

impl GoalField {
    /// The field of `goal` over `grid`'s ground plane, or `None` when the
    /// goal is off the grid.
    ///
    /// The goal cell is the field's source whether or not it is walkable: a
    /// goal inside a building is still where the movers head, and the
    /// search's own blocked-goal tail stops them as close as it can.
    pub fn build(grid: &PathGrid, goal: (u16, u16)) -> Option<Self> {
        let width = grid.width() as usize;
        let height = grid.height() as usize;
        if width == 0 || height == 0 {
            return None;
        }
        let (gx, gy) = (goal.0 as usize, goal.1 as usize);
        if gx >= width || gy >= height {
            return None;
        }
        let goal_index = gy * width + gx;
        let mut step = vec![NO_STEP; width * height];
        // The visited marks are the field's own scratch, dropped with it.
        let mut visited = vec![false; width * height];
        let mut queue = VecDeque::new();
        visited[goal_index] = true;
        queue.push_back(goal_index);
        let mut reached = 1;
        while let Some(index) = queue.pop_front() {
            let (x, y) = (index % width, index / width);
            for (direction, (dx, dy)) in DIRECTION_DELTAS.iter().enumerate() {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                    continue;
                }
                let neighbour = ny as usize * width + nx as usize;
                if visited[neighbour] || !grid.is_walkable(nx as u16, ny as u16) {
                    continue;
                }
                visited[neighbour] = true;
                // The neighbour's step toward the goal is back to this cell,
                // which is the opposite compass direction.
                step[neighbour] = ((direction + 4) & 7) as u8;
                queue.push_back(neighbour);
                reached += 1;
            }
        }
        Some(Self {
            width,
            height,
            goal: goal_index,
            step,
            reached,
        })
    }

    /// The cells the field covers: the ground plane's cells that reach the
    /// goal. The build visits each of them once, so this is its own work.
    pub fn cells(&self) -> usize {
        self.reached
    }

    /// The bytes the field holds: one byte per cell of the map it was built
    /// over, whether or not the cell reaches the goal. This is the memory a
    /// cache over fields has to charge it.
    pub fn footprint(&self) -> usize {
        self.width * self.height
    }

    /// The field's route from `start` to the goal, as cell indices, the goal
    /// last.
    fn route_from(&self, start: (u16, u16)) -> Option<Vec<usize>> {
        let (sx, sy) = (start.0 as usize, start.1 as usize);
        if sx >= self.width || sy >= self.height {
            return None;
        }
        let start_index = sy * self.width + sx;
        if start_index == self.goal {
            return None;
        }
        let mut route = Vec::new();
        let mut index = start_index;
        while index != self.goal {
            let direction = self.step[index];
            if direction == NO_STEP {
                return None;
            }
            route.push(index);
            let (x, y) = (index % self.width, index / self.width);
            let (dx, dy) = DIRECTION_DELTAS[usize::from(direction)];
            let (nx, ny) = (x as i32 + dx, y as i32 + dy);
            if nx < 0 || ny < 0 || nx >= self.width as i32 || ny >= self.height as i32 {
                return None;
            }
            index = ny as usize * self.width + nx as usize;
            if route.len() > self.step.len() {
                // The steps form a tree toward the goal, so this cannot
                // happen; a loop would hang the search.
                return None;
            }
        }
        route.push(self.goal);
        Some(route)
    }

    /// The field's route from `start` to the goal, as cells, the start
    /// first and the goal last.
    pub fn route_cells_from(&self, start: (u16, u16)) -> Option<Vec<(u16, u16)>> {
        Some(
            self.route_from(start)?
                .into_iter()
                .map(|cell| ((cell % self.width) as u16, (cell / self.width) as u16))
                .collect(),
        )
    }
}
