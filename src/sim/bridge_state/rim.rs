//! Native bridge rim cleanup control flow: concrete 576770 -> 576200 and
//! wooden 571050 -> 570AE0. Each wooden body is instruction-identical to its
//! concrete twin except for the tileset base (0xABAD1C, not 0xAA0E28) and the
//! selector's search bound ([`RimBounds`]); 570AE0 calls the NWSE setter
//! 47E470, byte-identical to 47E040.
//!
//! The host owns live cells and executes group writes/callbacks synchronously.
//! No precomputed mutation list may stand in for callback-time state here.
//! Evidence: HIGH_BRIDGE_RIM_REFRESH_ALGORITHM_GHIDRA_REPORT.md and
//! tools/spatial_oracle/bridge_rim. The current stock corpus covers one axis;
//! additional selector branches require their own comparisons.

use crate::map::bridge_rim_tiles::HighBridgeRimTiles;

#[cfg(all(test, tests_sim_a_b))]
#[path = "rim_tests.rs"]
mod tests;

pub(crate) type RimCoord = (i16, i16);

#[derive(Debug, Clone, Copy)]
pub(crate) struct RimCell {
    pub coord: RimCoord,
    pub flags: u32,
    pub tile: i32,
    pub subtile: u8,
    /// Native +2C target's current +24 coordinate, required only when the
    /// structural bit is set without the anchor-self bit.
    pub anchor: Option<RimCoord>,
}

pub(crate) trait HighBridgeRimHost {
    /// Stable real-cell identity or the one shared dummy identity.
    type Cell: Copy;
    fn tiles(&self) -> HighBridgeRimTiles;
    fn bounds(&self) -> RimBounds;
    /// Same fixed-stride lookup and retained dummy effects as Get_CellClass.
    fn cell(&mut self, coord: RimCoord) -> Self::Cell;
    /// Read a retained object without another Get_CellClass/dummy stamp.
    fn read(&self, cell: Self::Cell) -> RimCell;
    /// The selector separately probes Map+13C after its Size-diamond guard.
    fn allocated(&self, coord: RimCoord) -> bool;
    /// Execute full 47E040, including each slot's writes BEFORE its fallout.
    fn clear_group(&mut self, anchor: Self::Cell, direction: u8);
    /// The caller's +11E=0/+44=-1 stores, followed by its extra radar mark.
    fn clear_overlay_and_mark_radar(&mut self, cell: Self::Cell);
    /// 575EE0 remains observable even when this refresh makes no field writes.
    fn notify_span(&mut self, first: RimCoord, end: RimCoord);
    /// Dispatch event31 if the retained cell's current Tag pointer is nonnull.
    fn notify_cell(&mut self, cell: Self::Cell);
    /// Presentation invalidation for the searched span. It must not own cells.
    fn mark_span(&mut self, start: RimCoord, end: RimCoord);
    /// Begin a fresh native sentinel rectangle before this selector call.
    fn reset_span_marks(&mut self);
    /// Publish only when the accumulated rectangle differs from the sentinel.
    fn mark_screen(&mut self);
}

/// The selector's search bound, tested before each cursor's lookup.
#[derive(Debug, Clone, Copy)]
pub(crate) enum RimBounds {
    /// 576770: the Size diamond, from MapClass Size width and height.
    Diamond { width: i32, height: i32 },
    /// 571050: the inclusive MapClass+0x124..+0x130 rectangle
    /// (0x005711D8..0x00571227 and the step test at 0x00571384).
    Rect(crate::sim::scenario_bootstrap::NativeStartBounds),
}

impl RimBounds {
    fn contains(self, cursor: RimCoord) -> bool {
        match self {
            Self::Diamond { width, height } => {
                let (x, y) = (i32::from(cursor.0), i32::from(cursor.1));
                !(x + y <= width || width <= x - y || width <= y - x || width + height * 2 < x + y)
            }
            Self::Rect(bounds) => bounds.contains(cursor),
        }
    }
}

pub(crate) fn step(coord: RimCoord, direction: u8) -> RimCoord {
    let delta = crate::util::direction::DIRECTION_DELTAS[usize::from(direction & 7)];
    (
        coord.0.wrapping_add(delta.0 as i16),
        coord.1.wrapping_add(delta.1 as i16),
    )
}

/// Original 575EE0: ascending endpoint-exclusive span, each row in the order
/// center, +1 perpendicular, -1 perpendicular, -2 perpendicular. The host must
/// perform every lookup even without a CellTag: it may move the shared dummy.
pub(crate) fn notify_span_cells(host: &mut impl HighBridgeRimHost, first: RimCoord, end: RimCoord) {
    visit_span_cells(first, end, |coord| {
        let cell = host.cell(coord);
        host.notify_cell(cell);
        host.read(cell).coord
    });
}

/// Shared575EE0 traversal. Each visit returns the retained receiver's live
/// coordinate after its callback, including shared-dummy lookup mutations.
pub(crate) fn visit_span_cells(
    first: RimCoord,
    end: RimCoord,
    mut visit: impl FnMut(RimCoord) -> RimCoord,
) {
    let horizontal = first.1 == end.1;
    let reversed = if horizontal {
        end.0 < first.0
    } else {
        end.1 < first.1
    };
    let (mut cursor, last) = if reversed { (end, first) } else { (first, end) };
    while cursor != last {
        let positive = if horizontal { 4 } else { 2 };
        let negative = (positive + 4) & 7;
        for coord in [cursor, step(cursor, positive)] {
            visit(coord);
        }
        let third = visit(step(cursor, negative));
        // 576073/5761A3 use the third retained CellClass +24 AFTER its Tag
        // callback. A real fixed-stride alias or a moved dummy matters here.
        visit(step(third, negative));
        cursor = step(cursor, if horizontal { 2 } else { 4 });
    }
}

/// Original 576770/571050 selector. Direction-order neighbor reads and anchor
/// resolution precede the independent theater scan; bridge roles are not input.
pub(crate) fn update_adjacent(host: &mut impl HighBridgeRimHost, input: RimCoord) {
    let Some(neighbor) = (0..8).find_map(|direction| {
        let cell = host.cell(step(input, direction));
        (host.read(cell).flags & 0x500 != 0).then_some(cell)
    }) else {
        return;
    };

    let initial = host.read(neighbor);
    let mut cursor;
    if initial.flags & 0x100 == 0 {
        let forward = if initial.flags & 0x800 != 0 { 4 } else { 2 };
        cursor = initial.coord;
        let mut destroyed_count = 0;
        loop {
            cursor = step(cursor, forward);
            let cell = host.cell(cursor);
            if host.read(cell).flags & 0x400 == 0 {
                break;
            }
            destroyed_count += 1;
            if destroyed_count > 3 {
                return;
            }
        }
        let backward = (forward + 4) & 7;
        cursor = step(step(cursor, backward), backward);
    } else if initial.flags & 0x80 == 0 {
        // A null/unrepresented native anchor cannot establish a valid target.
        let Some(anchor) = initial.anchor else {
            return;
        };
        cursor = anchor;
    } else {
        cursor = initial.coord;
    }

    let forward = if host.read(neighbor).flags & 0x800 != 0 {
        6
    } else {
        0
    };
    host.reset_span_marks();
    let bounds = host.bounds();
    let tiles = host.tiles();
    loop {
        if !bounds.contains(cursor) {
            return;
        }
        if host.allocated(cursor) {
            let cell = host.cell(cursor);
            let fields = host.read(cell);
            if let Some(direction) = tiles.start_direction(fields.tile, fields.subtile) {
                if update_edge(host, cursor, direction) {
                    host.mark_screen();
                }
                return;
            }
        }
        cursor = step(cursor, forward);
    }
}

/// Original 576200/570AE0. Tail recursion restarts the complete search at the initial
/// coordinate after every group clear. An iterative restart preserves that
/// order without placing a native-length bound on Rust's call stack.
pub(crate) fn update_edge(
    host: &mut impl HighBridgeRimHost,
    start: RimCoord,
    direction: u8,
) -> bool {
    let mut changed = false;
    'restart: loop {
        let tiles = host.tiles();
        let mut cell = host.cell(start);
        let first = step(host.read(cell).coord, direction);
        let mut found = None;
        // Candidate distances1..29; distance30 is the original failure sentinel.
        for distance in 1..30 {
            let requested = step(host.read(cell).coord, direction);
            cell = host.cell(requested);
            let fields = host.read(cell);
            if tiles.is_end(fields.tile, fields.subtile, direction) {
                found = Some((requested, distance));
                break;
            }
        }
        let Some((end, distance)) = found else {
            return changed;
        };
        cell = host.cell(start);
        host.mark_span(start, end);
        let mut previous_clear = false;
        let mut last_anchor = (-1, -1);
        let mut notified = false;
        for _ in 0..distance {
            let fields = host.read(cell);
            let anchor = fields.flags & 0x80 != 0;
            if anchor && previous_clear {
                last_anchor = fields.coord;
            }
            if !anchor && !previous_clear && last_anchor != (-1, -1) {
                let back = step(fields.coord, direction.wrapping_sub(4) & 7);
                // This lookup is a native call before the setter itself.
                let target = host.cell(back);
                host.clear_group(target, if direction == 2 { 0 } else { 6 });
                host.clear_overlay_and_mark_radar(target);
                changed = true;
                continue 'restart;
            }
            previous_clear = !anchor;
            if previous_clear && !notified {
                host.notify_span(first, end);
                notified = true;
            }
            // Notifications may move the dummy. Resume the retained object.
            cell = host.cell(step(host.read(cell).coord, direction));
        }
        return changed;
    }
}
