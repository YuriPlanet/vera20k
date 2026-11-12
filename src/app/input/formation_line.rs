//! Formation line: hold the Formation Line key and drag with the left button
//! to draw a line; on release every selected unit moves to its own point on
//! it. With Shift held as well the drag pulls out a block, rows of units
//! behind the dragged front row. A VERA feature; gamemd has no such order
//! ([docs](../../../docs/formation-line.md)).
//!
//! Which unit takes which point: [`assign`] cuts a large group and the line
//! into matching pieces and solves each for the least total travel.
//!
//! This module owns the held key, the path being drawn and the matching. The
//! orders themselves are ordinary Moves built by
//! [`super::context_order::queue_formation_line`].

use winit::keyboard::KeyCode;

/// A new node joins the path once the cursor is this far from the last one,
/// in world pixels. It bounds the path's size, not its shape.
const MIN_NODE_SPACING: f32 = 4.0;

/// A drag shorter than this, in world pixels, is an ordinary click: half a
/// cell's width.
const MIN_LINE_LENGTH: f32 = 30.0;

/// One solve costs about the cube of its size, so a group is cut into
/// pieces, each solved alone. Pieces are as large as this many steps allow
/// over the whole group and no larger than [`MAX_PIECE`]: up to 128 units are
/// one solve, 2,000 are pieces of about 89 and 20,000 are pieces of 28.
const SOLVE_STEP_BUDGET: usize = 16_000_000;
const MAX_PIECE: usize = 128;
const MIN_PIECE: usize = 16;

/// A block fills every cell. Cells are diamonds this wide and this tall in
/// world pixels, so a front row can run eight ways and stay on cells: along
/// either side of the diamonds, or straight across or straight down the
/// screen, where the next cell is a whole width or height away.
const CELL_WIDTH: f32 = crate::map::terrain::TILE_WIDTH;
const CELL_HEIGHT: f32 = crate::map::terrain::TILE_HEIGHT;

/// A map cell as the matching sees it.
pub(crate) type Cell = (i32, i32);

/// The line being drawn: the cursor's trail in world pixels.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct FormationPath {
    nodes: Vec<(f32, f32)>,
    /// How many units were selected at the press: the dots drawn.
    units: usize,
    /// Shift was held at the press: the drag is a block's front row, from the
    /// first node to the last, and not a line.
    block: bool,
}

/// How far apart two world points are.
fn gap(from: (f32, f32), to: (f32, f32)) -> f32 {
    (to.0 - from.0).hypot(to.1 - from.1)
}

impl FormationPath {
    fn new(start: (f32, f32), units: usize, block: bool) -> Self {
        Self {
            nodes: vec![start],
            units,
            block,
        }
    }

    /// The cursor moved: it joins the trail once it is far enough from the
    /// trail's end.
    fn push(&mut self, point: (f32, f32)) {
        let far_enough = self.nodes.last().is_some_and(|&end| {
            let moved = gap(end, point);
            moved.is_finite() && moved >= MIN_NODE_SPACING
        });
        if far_enough {
            self.nodes.push(point);
        }
    }

    fn length(&self) -> f32 {
        self.nodes
            .windows(2)
            .map(|pair| gap(pair[0], pair[1]))
            .sum()
    }

    /// Long enough to be a formation and not a click.
    fn is_line(&self) -> bool {
        self.length() >= MIN_LINE_LENGTH
    }

    /// The release orders the group: a line long enough, or any block, as a
    /// block's bare click is its tightest shape.
    pub(crate) fn is_order(&self) -> bool {
        self.block || self.is_line()
    }

    /// One point per unit, in world pixels: along the line, or the block's
    /// rows.
    pub(crate) fn points(&self, count: usize) -> Vec<(f32, f32)> {
        if self.block {
            self.block_points(count)
        } else {
            self.spread(count)
        }
    }

    /// `count` points in a block whose front row is the drag, from the
    /// first node to the last: the drag's length is how many stand side by
    /// side, and the rest fill rows behind it, one in every cell. The front
    /// runs the nearest of the eight ways that stay on cells
    /// ([`CELL_WIDTH`]). Behind is to the right of the drag, so a drag from
    /// left to right fills towards the bottom of the screen. A drag shorter
    /// than a cell runs across the screen, about as wide as it is deep on
    /// the ground. A short last row sits in the middle.
    fn block_points(&self, count: usize) -> Vec<(f32, f32)> {
        let (Some(&corner), Some(&cursor)) = (self.nodes.first(), self.nodes.last()) else {
            return Vec::new();
        };
        if count == 0 {
            return Vec::new();
        }
        let (across, down) = (cursor.0 - corner.0, cursor.1 - corner.1);
        let length = across.hypot(down);
        let (half_width, half_height) = (CELL_WIDTH / 2.0, CELL_HEIGHT / 2.0);
        // One step along the front, each way it can run.
        let ways = [
            (CELL_WIDTH, 0.0),
            (-CELL_WIDTH, 0.0),
            (0.0, CELL_HEIGHT),
            (0.0, -CELL_HEIGHT),
            (half_width, half_height),
            (half_width, -half_height),
            (-half_width, half_height),
            (-half_width, -half_height),
        ];
        let mut along = ways[0];
        let mut columns = (((count as f32) / 2.0).sqrt().round() as usize).max(1);
        if length >= half_width {
            let lean = |way: (f32, f32)| (way.0 * across + way.1 * down) / way.0.hypot(way.1);
            for way in ways {
                if lean(way) > lean(along) {
                    along = way;
                }
            }
            columns = (length / along.0.hypot(along.1)) as usize + 1;
        }
        let columns = columns.clamp(1, count);
        let rows = count.div_ceil(columns);
        // The next cell behind: a quarter turn of the step on the ground.
        // Straight across or down the screen that is two rows away, and the
        // row between stands half a step along.
        let behind = (-2.0 * along.1, along.0 / 2.0);
        let brick = along.0 == 0.0 || along.1 == 0.0;
        (0..count)
            .map(|index| {
                let (row, column) = (index / columns, index % columns);
                let in_row = if row + 1 == rows {
                    count - row * columns
                } else {
                    columns
                };
                let mut steps = (column + (columns - in_row) / 2) as f32;
                let mut back = row as f32;
                if brick {
                    back /= 2.0;
                    if row % 2 == 1 {
                        steps += 0.5;
                    }
                }
                (
                    corner.0 + steps * along.0 + back * behind.0,
                    corner.1 + steps * along.1 + back * behind.1,
                )
            })
            .collect()
    }

    /// `count` points the same distance apart along the trail, the first on
    /// its start and the last on its end. One unit takes the end.
    ///
    /// Walks the trail once, piece by piece, dropping a point each time the
    /// distance owed since the last one has been covered.
    fn spread(&self, count: usize) -> Vec<(f32, f32)> {
        let (Some(&first), Some(&last)) = (self.nodes.first(), self.nodes.last()) else {
            return Vec::new();
        };
        match count {
            0 => return Vec::new(),
            1 => return vec![last],
            _ => {}
        }
        let apart = self.length() / (count - 1) as f32;
        let mut points = Vec::with_capacity(count);
        points.push(first);
        let mut owed = apart;
        for pair in self.nodes.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            let piece = gap(from, to);
            let mut walked = 0.0;
            while points.len() + 1 < count && piece - walked >= owed {
                walked += owed;
                let along = walked / piece;
                points.push((
                    from.0 + (to.0 - from.0) * along,
                    from.1 + (to.1 - from.1) * along,
                ));
                owed = apart;
            }
            owed -= piece - walked;
        }
        // Rounding can leave the walk a hair short of the last inner points.
        points.resize(count - 1, last);
        points.push(last);
        points
    }

    /// The dots to draw: one per selected unit. A block shows from the
    /// press, a line once it has a second node.
    pub(crate) fn dots(&self) -> Vec<(f32, f32)> {
        if self.units < 2 || (!self.block && self.nodes.len() < 2) {
            return Vec::new();
        }
        self.points(self.units)
    }
}

/// The held Formation Line key and the path drawn while it was down.
#[derive(Debug, Default)]
pub(crate) struct FormationLineState {
    held_key: Option<KeyCode>,
    path: Option<FormationPath>,
}

impl FormationLineState {
    pub(crate) fn press_key(&mut self, physical: KeyCode) {
        self.held_key = Some(physical);
    }

    /// The key came up. A line already being drawn stays until the button
    /// does: the press decided what the drag is.
    pub(crate) fn release_key(&mut self, physical: KeyCode) -> bool {
        if self.held_key == Some(physical) {
            self.held_key = None;
            return true;
        }
        false
    }

    pub(crate) fn clear_key(&mut self) {
        self.held_key = None;
    }

    pub(crate) fn key_held(&self) -> bool {
        self.held_key.is_some()
    }

    /// Start a line, or with `block` a block's front row, at the press point for
    /// `units` selected units.
    pub(crate) fn begin(&mut self, world: (f32, f32), units: usize, block: bool) {
        self.path = Some(FormationPath::new(world, units, block));
    }

    /// Is a line being drawn?
    pub(crate) fn drawing(&self) -> bool {
        self.path.is_some()
    }

    pub(crate) fn extend(&mut self, world: (f32, f32)) {
        if let Some(path) = self.path.as_mut() {
            path.push(world);
        }
    }

    /// End the drag and hand over what was drawn.
    pub(crate) fn finish(&mut self) -> Option<FormationPath> {
        self.path.take()
    }

    pub(crate) fn cancel(&mut self) {
        self.path = None;
    }

    pub(crate) fn path(&self) -> Option<&FormationPath> {
        self.path.as_ref()
    }
}

/// Give each unit one of `points`, which are in line order: `units[i]` is
/// `(id, cell)` and the result pairs every id with its point. Both lists
/// must be the same length; the shorter one bounds the result otherwise.
///
/// Split and solve: a group too large for one solve is cut in two with the
/// line ([`split_and_solve`]), again and again, and each piece gets the
/// assignment with the least total travel ([`assign_least_travel`]). Within
/// a piece no two straight routes cross, as a crossed pair always travels
/// further than the same pair uncrossed. Units beside a cut can get a
/// slightly longer trip than one solve over the whole group would give them.
pub(crate) fn assign(units: &[(u64, Cell)], points: &[Cell]) -> Vec<(u64, Cell)> {
    let count = units.len().min(points.len());
    let mut units = units[..count].to_vec();
    let mut orders = Vec::with_capacity(count);
    split_and_solve(&mut units, &points[..count], piece_size(count), &mut orders);
    orders
}

/// The largest piece the step budget allows for a group of `count`.
fn piece_size(count: usize) -> usize {
    let piece = ((SOLVE_STEP_BUDGET / count.max(1)) as f64).sqrt() as usize;
    piece.clamp(MIN_PIECE, MAX_PIECE)
}

/// Solve `units` against `points` when they fit one piece. Otherwise cut the
/// points in two halves in line order, give the first half the units that
/// lie furthest back along the direction from the first half's middle to the
/// second's, and do the same to each half. The cut is a straight line across
/// that direction, placed so each side has as many units as points.
fn split_and_solve(
    units: &mut [(u64, Cell)],
    points: &[Cell],
    piece: usize,
    orders: &mut Vec<(u64, Cell)>,
) {
    let count = units.len();
    if count <= piece.max(1) {
        orders.extend(assign_least_travel(units, points));
        return;
    }
    let half = count / 2;
    let (first_points, second_points) = points.split_at(half);
    let sum = |cells: &[Cell]| {
        cells.iter().fold((0i128, 0i128), |total, cell| {
            (total.0 + i128::from(cell.0), total.1 + i128::from(cell.1))
        })
    };
    let (first, second) = (sum(first_points), sum(second_points));
    let (first_count, second_count) = (first_points.len() as i128, second_points.len() as i128);
    // From the first half's middle to the second's, times both counts so the
    // two averages need no division.
    let axis = (
        second.0 * first_count - first.0 * second_count,
        second.1 * first_count - first.1 * second_count,
    );
    units.select_nth_unstable_by_key(half, |&(id, cell)| {
        (
            i128::from(cell.0) * axis.0 + i128::from(cell.1) * axis.1,
            id,
        )
    });
    let (first_units, second_units) = units.split_at_mut(half);
    split_and_solve(first_units, first_points, piece, orders);
    split_and_solve(second_units, second_points, piece, orders);
}

/// Travel cost of one pairing: the distance in 1/1024 cells.
fn travel(from: Cell, to: Cell) -> i64 {
    let dx = f64::from(to.0) - f64::from(from.0);
    let dy = f64::from(to.1) - f64::from(from.1);
    ((dx * dx + dy * dy).sqrt() * 1024.0).round() as i64
}

/// The assignment with the least total travel: the Hungarian algorithm with
/// row and column potentials, O(n^3).
fn assign_least_travel(units: &[(u64, Cell)], points: &[Cell]) -> Vec<(u64, Cell)> {
    let n = units.len().min(points.len());
    let costs: Vec<i64> = units[..n]
        .iter()
        .flat_map(|unit| points[..n].iter().map(move |&point| travel(unit.1, point)))
        .collect();
    // 1-based, with row and column 0 as the algorithm's sentinel.
    let mut row_potential = vec![0i64; n + 1];
    let mut column_potential = vec![0i64; n + 1];
    let mut column_row = vec![0usize; n + 1];
    let mut way = vec![0usize; n + 1];
    for row in 1..=n {
        column_row[0] = row;
        let mut column = 0;
        let mut least = vec![i64::MAX; n + 1];
        let mut used = vec![false; n + 1];
        loop {
            used[column] = true;
            let current_row = column_row[column];
            let mut delta = i64::MAX;
            let mut next = 0;
            for candidate in 1..=n {
                if used[candidate] {
                    continue;
                }
                let cost = costs[(current_row - 1) * n + candidate - 1]
                    - row_potential[current_row]
                    - column_potential[candidate];
                if cost < least[candidate] {
                    least[candidate] = cost;
                    way[candidate] = column;
                }
                if least[candidate] < delta {
                    delta = least[candidate];
                    next = candidate;
                }
            }
            for candidate in 0..=n {
                if used[candidate] {
                    row_potential[column_row[candidate]] += delta;
                    column_potential[candidate] -= delta;
                } else {
                    least[candidate] -= delta;
                }
            }
            column = next;
            if column_row[column] == 0 {
                break;
            }
        }
        while column != 0 {
            let previous = way[column];
            column_row[column] = column_row[previous];
            column = previous;
        }
    }
    let mut orders = vec![(0u64, (0, 0)); n];
    for column in 1..=n {
        let row = column_row[column];
        orders[row - 1] = (units[row - 1].0, points[column - 1]);
    }
    orders
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total_travel(units: &[(u64, Cell)], orders: &[(u64, Cell)]) -> i64 {
        orders
            .iter()
            .map(|&(id, point)| {
                let from = units.iter().find(|unit| unit.0 == id).unwrap().1;
                travel(from, point)
            })
            .sum()
    }

    #[test]
    fn a_block_fills_every_cell_behind_the_dragged_front_row() {
        // A bare click: two across the screen, rows half a cell's height
        // apart and every other one half a cell along, which is every cell.
        let click = FormationPath::new((0.0, 0.0), 6, true);
        assert!(click.is_order());
        assert_eq!(
            click.dots(),
            vec![
                (0.0, 0.0),
                (60.0, 0.0),
                (30.0, 15.0),
                (90.0, 15.0),
                (0.0, 30.0),
                (60.0, 30.0)
            ]
        );
        // A drag down a diamond's side, a little over four cells long: five
        // side by side on that side's cells, the sixth behind the middle.
        let mut dragged = FormationPath::new((0.0, 0.0), 6, true);
        dragged.push((130.0, 60.0));
        assert_eq!(
            dragged.points(6),
            vec![
                (0.0, 0.0),
                (30.0, 15.0),
                (60.0, 30.0),
                (90.0, 45.0),
                (120.0, 60.0),
                (30.0, 45.0)
            ]
        );
    }

    #[test]
    fn spread_puts_one_point_per_unit_evenly_along_a_bent_path() {
        let mut path = FormationPath::new((0.0, 0.0), 5, false);
        path.push((100.0, 0.0));
        path.push((100.0, 100.0));
        assert!(path.is_line());
        assert_eq!(
            path.spread(5),
            vec![
                (0.0, 0.0),
                (50.0, 0.0),
                (100.0, 0.0),
                (100.0, 50.0),
                (100.0, 100.0)
            ]
        );
        assert_eq!(path.spread(1), vec![(100.0, 100.0)]);

        let mut click = FormationPath::new((0.0, 0.0), 5, false);
        click.push((10.0, 0.0));
        assert!(!click.is_line(), "a short drag stays a click");
    }

    /// A row of units facing a line whose points are listed the other way
    /// round: each unit goes straight ahead, not to the point of its index.
    #[test]
    fn a_small_group_takes_the_points_with_the_least_total_travel() {
        let units: Vec<(u64, Cell)> = (0..6).map(|i| (100 + i as u64, (i * 3, 0))).collect();
        let points: Vec<Cell> = (0..6).rev().map(|i| (i * 3, 10)).collect();
        let straight: Vec<(u64, Cell)> = units.iter().map(|&(id, (x, _))| (id, (x, 10))).collect();

        let mut orders = assign(&units, &points);
        orders.sort();
        assert_eq!(orders, straight);
    }

    /// Two clusters, listed interleaved, before a line: cut into pieces of
    /// two, the western cluster still takes the line's western half, every
    /// point is taken once and the group travels no further than one solve
    /// over all of it.
    #[test]
    fn a_group_cut_into_pieces_splits_with_the_line_and_keeps_the_least_travel() {
        let units: Vec<(u64, Cell)> = (0..8)
            .map(|i| (i as u64, if i % 2 == 0 { (i, 0) } else { (40 + i, 0) }))
            .collect();
        let points: Vec<Cell> = (0..8).map(|i| (i * 6, 10)).collect();

        let mut orders = Vec::new();
        split_and_solve(&mut units.clone(), &points, 2, &mut orders);

        for &(id, point) in &orders {
            assert_eq!(id % 2 == 0, point.0 < 24, "unit {id} went to {point:?}");
        }
        let mut taken: Vec<Cell> = orders.iter().map(|order| order.1).collect();
        taken.sort();
        assert_eq!(taken, points);
        assert_eq!(
            total_travel(&units, &orders),
            total_travel(&units, &assign_least_travel(&units, &points))
        );
    }

    #[test]
    fn pieces_shrink_as_the_group_grows() {
        assert_eq!(piece_size(128), 128);
        assert_eq!(piece_size(20_000), 28);
        assert_eq!(piece_size(1_000_000), MIN_PIECE);
    }
}
