//! Live bridge state machines and CellClass setter publication.
//!
//! Native576BA0/571490 and 47E040/47E470; evidence: bridge_body_publication
//! native corpus and HIGH_BRIDGE_RIM_REFRESH_ALGORITHM_GHIDRA_REPORT.md. Scalar writes must not
//! dispatch extra callbacks. The host keeps world authorities resident while
//! fallout, perpendicular helpers, rim and zone work execute synchronously.
//! The production host is world/bridge_publication.rs; its existing tile/rim
//! callback projections remain explicitly outside this core's parity claim.

use super::ramp_repair::Family;
use super::{Axis, Phase};

#[cfg(test)]
#[path = "publication_tests.rs"]
mod tests;

pub(crate) type CellCoord = (i16, i16);

pub(crate) trait BridgePublicationHost {
    /// Stable allocation identity; a shared dummy remains the same identity
    /// when another lookup changes its coordinate.
    type Cell: Copy;
    fn lookup(&mut self, coord: CellCoord) -> Self::Cell;
    fn coord(&self, cell: Self::Cell) -> CellCoord;
    fn flags(&self, cell: Self::Cell) -> u32;
    fn state(&self, cell: Self::Cell) -> u8;
    fn write_flags(&mut self, cell: Self::Cell, flags: u32);
    fn write_state(&mut self, cell: Self::Cell, state: u8);
    /// Literal native +2C pointer, distinct from a derived self relation.
    fn write_anchor(&mut self, cell: Self::Cell, anchor: Option<Self::Cell>);
    /// Literal +44=-1, without an implicit Recalc or zone callback.
    fn clear_overlay(&mut self, cell: Self::Cell);
    fn fallout(&mut self, cell: Self::Cell);
    fn radar(&mut self, cell: Self::Cell);
    /// The family's UpdateRamp_* helper; the twins differ only in base.
    fn perpendicular(
        &mut self,
        coord: CellCoord,
        axis: Axis,
        phase: Phase,
        direction: u8,
        family: Family,
    );
    /// UpdateAdjacentBridges 576770 or its wooden twin 571050.
    fn rim(&mut self, coord: CellCoord, family: Family);
    /// InvalidateBridgeZones 56DAE0 at `query`, and 56C510 when it asks.
    fn zones(&mut self, query: CellCoord);
    /// Current Cell+38 tile, +11A subtile and signed +11B level.
    fn tile(&self, cell: Self::Cell) -> i32;
    fn subtile(&self, cell: Self::Cell) -> u8;
    fn level(&self, cell: Self::Cell) -> i8;
    /// FloodFillIsoTileType 56EB80 `(coord, tile, -1, level, 0)`.
    fn flood(&mut self, coord: CellCoord, tile: i32, level: i32);
    /// RecalcCellsAndRebuildZones 586990 over the caller's vector.
    fn recalc_zones(&mut self, cells: &[CellCoord]);
    /// The family's tileset base and BridgeMiddle1/2 theater keys; absent
    /// without an active theater.
    fn middles(&self, family: Family) -> Option<(i32, [i32; 2])>;
}

fn step(coord: CellCoord, direction: u8) -> CellCoord {
    let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[usize::from(direction & 7)];
    (
        coord.0.wrapping_add(dx as i16),
        coord.1.wrapping_add(dy as i16),
    )
}

fn finish_slot<H: BridgePublicationHost>(host: &mut H, cell: H::Cell, direction: u8, set: bool) {
    host.write_state(
        cell,
        if set {
            crate::map::bridge_facts::bridge_setter_state(direction)
        } else {
            0
        },
    );
    if !set {
        host.fallout(cell);
    }
    host.radar(cell);
}

/// Original47E040's body/ramp callers supply direction0 or6 and state0/1.
/// Every following lookup uses current +24 after the preceding callback.
pub(crate) fn set_bridge_direction<H: BridgePublicationHost>(
    host: &mut H,
    anchor: H::Cell,
    direction: u8,
    set: bool,
) {
    debug_assert!(matches!(direction, 0 | 6));
    let intact = u32::from(set);
    let structural = intact << 8;
    let transition = intact << 9;
    let forward = intact << 12;
    let extra = intact << 16;
    let destroyed = u32::from(!set) << 10;
    let direction_zero = u32::from(direction == 0) << 11;

    // The first +11E store precedes the anchor flag word, even for destruction.
    host.write_state(
        anchor,
        crate::map::bridge_facts::bridge_setter_state(direction),
    );
    host.write_flags(
        anchor,
        (host.flags(anchor) & 0xfffe_e07f)
            | structural
            | transition
            | forward
            | extra
            | destroyed
            | (intact << 7)
            | direction_zero,
    );
    finish_slot(host, anchor, direction, set);

    let mut cell = host.lookup(step(host.coord(anchor), direction));
    host.write_anchor(cell, set.then_some(anchor));
    host.write_flags(
        cell,
        ((host.flags(cell) & 0xfffe_e8ff) | structural | transition | forward | extra | destroyed)
            & 0xffff_f7ff
            | direction_zero,
    );
    finish_slot(host, cell, direction, set);

    cell = host.lookup(step(host.coord(cell), direction));
    host.write_anchor(cell, set.then_some(anchor));
    host.write_flags(
        cell,
        ((host.flags(cell) & 0xfffe_e8ff) | structural | forward | extra | destroyed) & 0xffff_f7ff
            | direction_zero,
    );
    finish_slot(host, cell, direction, set);

    cell = host.lookup(step(host.coord(cell), direction));
    host.write_flags(cell, (host.flags(cell) & 0xffff_efff) | forward);

    // Opposite is relative to the retained anchor's current coordinate, not
    // the original input coordinate or the third forward lookup.
    cell = host.lookup(step(host.coord(anchor), direction.wrapping_sub(4) & 7));
    host.write_anchor(cell, set.then_some(anchor));
    host.write_flags(
        cell,
        ((host.flags(cell) & 0xffff_f8ff) | structural | transition | destroyed) & 0xfffe_e7ff
            | extra
            | direction_zero,
    );
    finish_slot(host, cell, direction, set);
    if direction == 6 {
        cell = host.lookup(step(host.coord(cell), 2));
        host.write_anchor(cell, set.then_some(anchor));
        host.write_flags(cell, (host.flags(cell) & 0xfffe_ffff) | extra);
    }
}

/// Original576BA0's structural-body branch, after its caller has resolved the
/// stable self/+2C anchor. State/axis is selected once before any callback.
/// Returns native0 for damage/no transition and1 for collapse.
pub(crate) fn advance_body_at_anchor<H: BridgePublicationHost>(
    host: &mut H,
    input: CellCoord,
    anchor: H::Cell,
    family: Family,
) -> bool {
    let state = host.state(anchor);
    let (axis, first_direction, second_direction, setter_direction) = if state <= 8 {
        (Axis::NS, 2, 6, 0)
    } else {
        (Axis::EW, 4, 0, 6)
    };
    let (first, second, collapse) = match state {
        0..=5 | 9..=14 => {
            host.write_state(anchor, if state <= 8 { 6 } else { 15 });
            (Some(Phase::DamageA), Some(Phase::DamageB), false)
        }
        6 | 15 => (Some(Phase::CollapseA), Some(Phase::CollapseB), true),
        7 | 17 => (Some(Phase::CollapseA), None, true),
        8 | 16 => (None, Some(Phase::CollapseB), true),
        _ => return false,
    };
    if let Some(phase) = first {
        host.perpendicular(host.coord(anchor), axis, phase, first_direction, family);
    }
    if let Some(phase) = second {
        host.perpendicular(host.coord(anchor), axis, phase, second_direction, family);
    }
    if collapse {
        set_bridge_direction(host, anchor, setter_direction, false);
        host.write_state(anchor, 0);
        host.clear_overlay(anchor);
        host.rim(input, family);
        host.zones(host.coord(anchor));
    }
    collapse
}

/// The non-structural half of 576BA0/571490, entered when the input cell's
/// tile is one of the family's BridgeMiddle1/2 variants. The input's
/// variant selects the branch; the walk reaches the anchor row, whose tiles
/// the flood replaces. Returns native1 only for a concrete collapse: the
/// wooden driver falls through to its0 return (0x00571990, 0x00571EA0).
pub(crate) fn advance_bridgehead<H: BridgePublicationHost>(
    host: &mut H,
    input: CellCoord,
    family: Family,
) -> bool {
    let Some((base, middles)) = host.middles(family) else {
        return false;
    };
    let cell = host.lookup(input);
    let relative = host.tile(cell).wrapping_sub(base).wrapping_add(1);
    let variant_of = |middle: i32| (0..4).find(|&v| relative == middle.wrapping_add(v));
    let mut subtile = host.subtile(cell);
    // NS: the second column absorbs; EW: the second row does.
    let (axis, variant) = if let Some(variant) = variant_of(middles[0]) {
        if subtile & 1 != 0 {
            return false;
        }
        (Axis::NS, variant)
    } else if let Some(variant) = variant_of(middles[1]) {
        if subtile > 4 {
            return false;
        }
        (Axis::EW, variant)
    } else {
        return false;
    };
    let middle = middles[usize::from(axis == Axis::EW)];
    // Walk to subtile 4 (NS: N from above, S from below) or 2 (EW: W, E).
    // Native has no bound; a full i16 wrap is Rust's only guard.
    let (target, down, up) = match axis {
        Axis::NS => (4, 0, 4),
        Axis::EW => (2, 6, 2),
    };
    let mut walk = input;
    for _ in 0..=0x10000 {
        if subtile == target {
            break;
        }
        walk = step(walk, if subtile >= target { down } else { up });
        let cell = host.lookup(walk);
        subtile = host.subtile(cell);
    }
    if subtile != target {
        return false;
    }
    let anchor = host.lookup(walk);
    let (first_direction, second_direction) = match axis {
        Axis::NS => (2, 6),
        Axis::EW => (4, 0),
    };
    if variant < 3 {
        host.flood(
            host.coord(anchor),
            base.wrapping_add(middle).wrapping_add(2),
            -1,
        );
        // The wooden EW damage calls the concrete helpers 572B80/572C90
        // (0x00571AB0, 0x00571AC5); every other branch keeps its family.
        let ramps = if axis == Axis::EW {
            Family::High
        } else {
            family
        };
        host.perpendicular(
            host.coord(anchor),
            axis,
            Phase::DamageA,
            first_direction,
            ramps,
        );
        host.perpendicular(
            host.coord(anchor),
            axis,
            Phase::DamageB,
            second_direction,
            ramps,
        );
        return false;
    }

    // The three BlowUpBridge cells run along the span from the walk
    // coordinate. Native shifts them, the zone query and the flood level's
    // source by one column/row when the re-looked-up anchor is NS-odd or has
    // EW subtile >= 5; the walk above only exits at subtile 4 (NS) or 2 (EW),
    // so that shift never fires and is not ported.
    let (across, along) = match axis {
        Axis::NS => ((-1, 0), (0, 1)),
        Axis::EW => ((0, -1), (1, 0)),
    };
    let offset = |coord: CellCoord, (dx, dy): (i16, i16), n: i16| {
        (
            coord.0.wrapping_add(dx.wrapping_mul(n)),
            coord.1.wrapping_add(dy.wrapping_mul(n)),
        )
    };
    for n in [-1, 0, 1] {
        let target = host.lookup(offset(walk, along, n));
        host.fallout(target);
    }
    let center = host.coord(anchor);
    let level = host.level(anchor);
    host.flood(
        host.coord(anchor),
        base.wrapping_add(middle).wrapping_add(3),
        i32::from(level) - 4,
    );
    host.perpendicular(
        host.coord(anchor),
        axis,
        Phase::CollapseA,
        first_direction,
        family,
    );
    host.perpendicular(
        host.coord(anchor),
        axis,
        Phase::CollapseB,
        second_direction,
        family,
    );
    // NS: W then E; EW: N then S.
    let (rim_a, rim_b) = match axis {
        Axis::NS => (6, 2),
        Axis::EW => (0, 4),
    };
    host.rim(step(host.coord(anchor), rim_a), family);
    host.rim(step(host.coord(anchor), rim_b), family);
    host.zones(center);
    // 0x42FCB0-built vector: two lines across the span, five cells along it.
    let mut cells = Vec::with_capacity(10);
    for line in 0..2 {
        for n in -2..3 {
            cells.push(offset(offset(center, across, -line), along, n));
        }
    }
    host.recalc_zones(&cells);
    family == Family::High
}
