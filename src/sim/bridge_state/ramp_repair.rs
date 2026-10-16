//! Native573540/570050 ramp recovery and568E40/569760 span reconstruction.
//! Evidence: tools/spatial_oracle/bridge_repair.{py,json,meta.json}.
//! Callbacks remain synchronous; the host owns live terrain, constructors and
//! zone graph operations. A returned display extent is absent on native's
//! count30 abort, which leaves the caller's rectangle uninitialized.
use super::publication::CellCoord;
use crate::map::bridge_rim_tiles::HighBridgeRimTiles;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    High,
    Low,
}

pub(crate) type Rect = [i32; 4];

/// The cell reads of [`hut_anchor`].
pub(crate) trait HutCells {
    type Cell: Copy;
    type Error;
    fn lookup(&mut self, requested: CellCoord) -> Self::Cell;
    fn coord(&self, cell: Self::Cell) -> CellCoord;
    fn flags(&self, cell: Self::Cell) -> u32;
    fn anchor(&self, cell: Self::Cell) -> Result<Self::Cell, Self::Error>;
}

pub(crate) trait RepairHost: HutCells {
    fn tiles(&self, family: Family) -> HighBridgeRimTiles;
    fn tile(&self, cell: Self::Cell) -> i32;
    fn subtile(&self, cell: Self::Cell) -> u8;
    fn level(&self, cell: Self::Cell) -> u8;
    fn write_level(&mut self, cell: Self::Cell, level: u8) -> Result<(), Self::Error>;
    fn overlay(&self, cell: Self::Cell) -> i32;
    fn search_in_bounds(&self, requested: CellCoord, family: Family) -> bool;
    /// Native allocation test does not update the shared dummy on a miss.
    fn allocated(&self, requested: CellCoord) -> bool;
    fn ordinary_repair(&mut self, requested: CellCoord, family: Family) -> Result<(), Self::Error>;
    fn pavement_clear(&mut self, requested: CellCoord);
    fn replace(&mut self, requested: CellCoord, tile: i32) -> Result<(), Self::Error>;
    /// Original56DB70's return is the connectivity result after its record
    /// activation/edge additions, not a generic terrain-changed flag.
    fn validate(&mut self, requested: CellCoord) -> Result<bool, Self::Error>;
    /// Complete constructor5FC380, including admission, Mark and lifecycle.
    /// Last argument is native frame=-1, not house/owner.
    fn construct(
        &mut self,
        requested: CellCoord,
        overlay: u8,
        frame: i32,
    ) -> Result<(), Self::Error>;
    fn connectivity(&mut self) -> Result<(), Self::Error>;
    fn rebuild(&mut self, cells: &[CellCoord]) -> Result<(), Self::Error>;
    fn project(&mut self, requested: CellCoord, level: i8) -> [i32; 2];
    fn dirty_screen(&mut self, rect: Option<Rect>);
}

use super::rim::step;

fn relative<H: RepairHost>(host: &H, cell: H::Cell, keys: HighBridgeRimTiles) -> i32 {
    host.tile(cell).wrapping_sub(keys.base).wrapping_add(1)
}

fn raise_three<H: RepairHost>(
    host: &mut H,
    center: CellCoord,
    direction: u8,
) -> Result<(), H::Error> {
    let sides = if direction == 2 { [0, 4] } else { [2, 6] };
    for point in [center, step(center, sides[0]), step(center, sides[1])] {
        let cell = host.lookup(point);
        host.write_level(cell, host.level(cell).wrapping_add(4))?;
    }
    Ok(())
}

fn append_footprint(cells: &mut Vec<CellCoord>, center: CellCoord, direction: u8) {
    for across in 0..2i16 {
        for along in -2..3i16 {
            let (x, y) = if direction == 2 {
                (across, along)
            } else {
                (along, across)
            };
            cells.push((center.0.wrapping_add(x), center.1.wrapping_add(y)));
        }
    }
}

/// Native span primitive. The forward scan mutates terrain even when distance30
/// later aborts. Reconstruction starts at the retained input and excludes the
/// terminating endpoint; constructor callbacks can change following cells.
pub(crate) fn restore_span<H: RepairHost>(
    host: &mut H,
    family: Family,
    start: H::Cell,
    direction: u8,
    screen: bool,
) -> Result<Option<Rect>, H::Error> {
    let keys = host.tiles(family);
    let mut cell = host.lookup(host.coord(start));
    let mut count = 1;
    let mut connectivity = false;
    let mut rebuild = Vec::new();
    loop {
        let requested = step(host.coord(cell), direction);
        cell = host.lookup(requested);
        let old = relative(host, cell, keys);
        let sub = host.subtile(cell);
        let axis = usize::from(direction == 4);
        let (end, target_sub) = if direction == 2 {
            (keys.bottom_right, 4)
        } else {
            (keys.bottom_left, 2)
        };
        if matches!(direction, 2 | 4) && sub == target_sub {
            if end.contains(&old) {
                host.pavement_clear(requested);
                connectivity |= host.validate(step(requested, direction.wrapping_add(4)))?;
                break;
            }
            if (0..5).any(|v| old == keys.middle[axis].wrapping_add(v)) {
                host.replace(
                    requested,
                    keys.base.wrapping_add(keys.middle[axis]).wrapping_sub(1),
                )?;
                connectivity |= host.validate(requested)?;
                if old == keys.middle[axis].wrapping_add(4) {
                    raise_three(host, requested, direction)?;
                    append_footprint(&mut rebuild, requested, direction);
                }
            }
        }
        count += 1;
        if count == 30 {
            return Ok(None);
        }
    }
    cell = host.lookup(host.coord(start));
    let first = screen.then(|| host.project(host.coord(start), host.level(start) as i8));
    let mut next_requested = host.coord(cell);
    for _ in 0..count {
        let current = relative(host, cell, keys);
        let sub = host.subtile(cell);
        if !((current == keys.middle[0] && sub & 1 == 0) || (current == keys.middle[1] && sub <= 4))
        {
            let overlay = match (family, direction == 2) {
                (Family::High, true) => 24,
                (Family::High, false) => 25,
                (Family::Low, true) => 237,
                (Family::Low, false) => 238,
            };
            host.construct(host.coord(cell), overlay, -1)?;
        }
        next_requested = step(host.coord(cell), direction);
        cell = host.lookup(next_requested);
    }
    let rect = if let Some(first) = first {
        let last = host.lookup(next_requested);
        let end = host.project(next_requested, host.level(last) as i8);
        Some([
            first[0].min(end[0]).wrapping_sub(64),
            first[1].min(end[1]).wrapping_sub(64),
            first[0]
                .wrapping_sub(end[0])
                .wrapping_abs()
                .wrapping_add(128),
            first[1]
                .wrapping_sub(end[1])
                .wrapping_abs()
                .wrapping_add(128),
        ])
    } else {
        None
    };
    if connectivity {
        host.connectivity()?;
    }
    if !rebuild.is_empty() {
        host.rebuild(&rebuild)?;
    }
    Ok(rect)
}

/// The starter and anchor search both bridge-repair-hut tails run once their
/// overlay scan finds nothing: the repair here, and the death fallback of
/// `0x00574000` / `0x00574C20` (`0x005740D8..0x00574362`). The starter is the
/// input cell, else the first cell out to three steps in directions 0..7
/// whose flags carry 0x100 or 0x400. A 0x100 starter anchors on itself when
/// it also has 0x80, else on its +0x2C anchor. A pure 0x400 starter walks in
/// direction 4 (0x800) or 2 over at most three more 0x400 cells; the anchor
/// lies two steps back from the first cell without 0x400. Returns the anchor
/// and the walk direction, 6 for a 0x800 starter and 0 otherwise; `None`
/// where native returns (no starter, or a fourth 0x400 cell, `0x005742FE`).
pub(crate) fn hut_anchor<H: HutCells>(
    host: &mut H,
    input: CellCoord,
) -> Result<Option<(CellCoord, u8)>, H::Error> {
    let mut selected = host.lookup(input);
    if host.flags(selected) & 0x500 == 0 {
        'rays: for direction in 0..8 {
            let mut requested = input;
            for _ in 0..3 {
                requested = step(requested, direction);
                selected = host.lookup(requested);
                if host.flags(selected) & 0x500 != 0 {
                    break 'rays;
                }
            }
        }
    }
    let flags = host.flags(selected);
    if flags & 0x500 == 0 {
        return Ok(None);
    }
    let anchor = if flags & 0x100 == 0 {
        let forward = if flags & 0x800 == 0 { 2 } else { 4 };
        let mut point = host.coord(selected);
        let mut walked = 0;
        loop {
            point = step(point, forward);
            let next = host.lookup(point);
            if host.flags(next) & 0x400 == 0 {
                break;
            }
            walked += 1;
            if walked > 3 {
                return Ok(None);
            }
        }
        step(
            step(point, forward.wrapping_add(4)),
            forward.wrapping_add(4),
        )
    } else if flags & 0x80 == 0 {
        host.coord(host.anchor(selected)?)
    } else {
        host.coord(selected)
    };
    let walk = if host.flags(selected) & 0x800 == 0 {
        0
    } else {
        6
    };
    Ok(Some((anchor, walk)))
}

/// Family entry's overlay scan is x-major; engineer's earlier family scan is
/// y-major. Keeping both passes distinct is observable with competing strips.
pub(crate) fn repair<H: RepairHost>(
    host: &mut H,
    input: CellCoord,
    family: Family,
) -> Result<(), H::Error> {
    for x in -2..3i16 {
        for y in -2..3i16 {
            let requested = (input.0.wrapping_add(x), input.1.wrapping_add(y));
            let cell = host.lookup(requested);
            let overlay = host.overlay(cell);
            if super::ordinary::member(overlay, family) {
                return host.ordinary_repair(requested, family);
            }
        }
    }
    let Some((mut requested, reverse)) = hut_anchor(host, input)? else {
        return Ok(());
    };
    let keys = host.tiles(family);
    while host.search_in_bounds(requested, family) {
        if host.allocated(requested) {
            let cell = host.lookup(requested);
            let old = relative(host, cell, keys);
            let sub = host.subtile(cell);
            let endpoint = if keys.top_left.contains(&old) && sub == 8 {
                Some(2)
            } else if keys.top_right.contains(&old) && sub == 12 {
                Some(4)
            } else {
                None
            };
            if let Some(direction) = endpoint {
                host.pavement_clear(requested);
                let start = host.lookup(requested);
                let rect = restore_span(host, family, start, direction, true)?;
                host.dirty_screen(rect);
                return Ok(());
            }
            let middle = if sub == 5 && (0..5).any(|v| old == keys.middle[0].wrapping_add(v)) {
                Some((0, 2))
            } else if sub == 7 && (0..5).any(|v| old == keys.middle[1].wrapping_add(v)) {
                Some((1, 4))
            } else {
                None
            };
            if let Some((axis, direction)) = middle {
                let mut connectivity = false;
                let mut rebuild = Vec::new();
                if old == keys.middle[axis].wrapping_add(4) {
                    host.replace(
                        requested,
                        keys.base.wrapping_add(keys.middle[axis]).wrapping_sub(1),
                    )?;
                    let center = step(requested, direction + 4);
                    raise_three(host, center, direction)?;
                    connectivity = host.validate(center)?;
                    append_footprint(&mut rebuild, center, direction);
                }
                repair(
                    host,
                    step(step(requested, direction + 4), direction + 4),
                    family,
                )?;
                let start = host.lookup(requested);
                let rect = restore_span(host, family, start, direction, true)?;
                host.dirty_screen(rect);
                if connectivity {
                    host.connectivity()?;
                }
                if !rebuild.is_empty() {
                    host.rebuild(&rebuild)?;
                }
                return Ok(());
            }
        }
        requested = step(requested, reverse);
    }
    Ok(())
}

#[cfg(test)]
#[path = "ramp_repair_tests.rs"]
mod tests;
