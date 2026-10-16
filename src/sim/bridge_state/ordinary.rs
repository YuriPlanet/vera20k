//! Shared scalar cells of ordinary overlay bridges. The concrete205..232 and
//! wooden74..101 families can both sit at ground level; family is not height.
use super::publication::CellCoord;
use super::ramp_repair::{Family, Rect};

pub(crate) trait OrdinaryBridgeHost {
    type Cell: Copy;
    type Error;
    fn lookup(&mut self, coord: CellCoord) -> Self::Cell;
    fn coord(&self, cell: Self::Cell) -> CellCoord;
    fn overlay(&self, cell: Self::Cell) -> i32;
    fn write_overlay(&mut self, cell: Self::Cell, overlay: u8);
    fn redraw(&mut self, cell: Self::Cell);
    fn radar(&mut self, coord: CellCoord);
    fn recalc(&mut self, cell: Self::Cell) -> Result<(), Self::Error>;
    fn occupants(&mut self, cell: Self::Cell, damage_mode: u8) -> Result<(), Self::Error>;
    fn connectivity(&mut self) -> Result<(), Self::Error>;
    fn rebuild_rectangle(&mut self, rect: Rect) -> Result<(), Self::Error>;
}

pub(crate) fn member(overlay: i32, family: Family) -> bool {
    match family {
        Family::Low => (74..=101).contains(&overlay),
        Family::High => (205..=232).contains(&overlay),
    }
}

/// A standing ordinary bridge overlay: the family short of its two collapsed
/// caps (`0x64`/`0x65`, `0xE7`/`0xE8`). ApplyDamageToCell (`0x005871C8`)
/// and Apply_area_damage's direct blocks (`0x0048A217`, `0x0048A26D`) admit
/// these bands; the radar (GetRadarColor `0x0047C060`) and the parasite
/// release-cell test read the same bands.
pub fn standing(overlay: i32, family: Family) -> bool {
    match family {
        Family::Low => (0x4A..=0x63).contains(&overlay),
        Family::High => (0xCD..=0xE6).contains(&overlay),
    }
}

/// Native ordinary selector classification, shared by damage, repair and hut
/// callers. These are overlay axes; the longitudinal walk is perpendicular.
pub(crate) fn axis(overlay: i32, family: Family) -> Option<super::Axis> {
    member(overlay, family).then(|| {
        if north_south(overlay, family) {
            super::Axis::NS
        } else {
            super::Axis::EW
        }
    })
}

pub(super) fn north_south(overlay: i32, family: Family) -> bool {
    match family {
        Family::Low => matches!(overlay, 74..=82 | 92..=95 | 100),
        Family::High => matches!(overlay, 205..=213 | 223..=226 | 231),
    }
}

pub(super) fn offset(point: CellCoord, x: i16, y: i16) -> CellCoord {
    (point.0.wrapping_add(x), point.1.wrapping_add(y))
}

/// Original57BAA0/57CCF0/57F440 select the middle of the three-cell width using
/// live overlay membership. Reads retain native shared-dummy lookup effects.
pub(super) fn centered<H: OrdinaryBridgeHost>(
    host: &mut H,
    input: CellCoord,
    family: Family,
) -> Option<(CellCoord, bool)> {
    let selected = host.lookup(input);
    let overlay = host.overlay(selected);
    if !member(overlay, family) {
        return None;
    }
    let ns = north_south(overlay, family);
    let across = if ns { (0, -1) } else { (-1, 0) };
    let before = offset(input, across.0, across.1);
    let previous = host.lookup(before);
    let point = if !member(host.overlay(previous), family) {
        offset(input, -across.0, -across.1)
    } else {
        let previous = host.lookup(offset(before, across.0, across.1));
        if member(host.overlay(previous), family) {
            before
        } else {
            input
        }
    };
    Some((point, ns))
}

#[cfg(test)]
#[path = "ordinary_test_host.rs"]
pub(super) mod test_host;
