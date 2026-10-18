//! Substrate for carving a cliff stair: the per-shape tile tables and the
//! precheck every carve runs before it touches a cell.
//!
//! The carve routines themselves live in `carve`. What is here is the part
//! they all share, and the part that decides whether a carve happens at all:
//!
//! - [`RampRecord`] — the level-step and slope tables for the four straight
//!   ramp shapes, read out of the executable.
//! - [`rect_is_carveable`] — the go/no-go test. Its verdict is what makes the
//!   caller's hundred-attempt retry loop affordable, because a refusal costs
//!   nothing: no cell written, no random draw taken.
//!
//! Reached through `carve_driver`, which runs on the two island map types.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::preview::Playfield;
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;

use super::connector::RampShape;

/// Cells along a ramp's stepped edge. The tables are all this wide.
pub const RAMP_STEPS: usize = 5;

/// The tile and level tables for one ramp shape.
///
/// `level_steps` is added to the region's own level and then dropped by 4, so
/// `[3, 2, 1, 0, 0]` walks a cell down one terrain step at a time and then
/// holds — the stair itself.
///
/// The two slope rows are variants chosen by the sign of the run between the
/// carve's endpoints, not by any random draw. A slope value `v` writes `v` to
/// the cell's slope byte and picks the tile `ramp_base + v - 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RampRecord {
    pub level_steps: [i8; RAMP_STEPS],
    pub slope_descending: [u8; RAMP_STEPS],
    pub slope_ascending: [u8; RAMP_STEPS],
}

impl RampRecord {
    /// The slope row for a run of `delta`, which is the endpoint difference
    /// along the shape's own axis. Positive picks the ascending row.
    pub fn slopes(&self, delta: i32) -> &[u8; RAMP_STEPS] {
        if delta > 0 {
            &self.slope_ascending
        } else {
            &self.slope_descending
        }
    }
}

/// Every straight shape's level table is the same descending stair.
const LEVEL_STAIR: [i8; RAMP_STEPS] = [3, 2, 1, 0, 0];

/// Tables for the four straight ramp shapes, in the executable's own order.
///
/// Only the straight shapes have tables — the three corner routines stamp
/// their geometry differently and read none of this. The four slope triples
/// are the four cardinal ramp orientations: each row runs
/// `[head, body, body, body, tail]`.
const RAMP_RECORDS: [RampRecord; 4] = [
    // ClearSouth
    RampRecord {
        level_steps: LEVEL_STAIR,
        slope_descending: [11, 15, 15, 15, 7],
        slope_ascending: [12, 16, 16, 16, 8],
    },
    // ClearEast
    RampRecord {
        level_steps: LEVEL_STAIR,
        slope_descending: [10, 14, 14, 14, 6],
        slope_ascending: [11, 15, 15, 15, 7],
    },
    // ClearNorth
    RampRecord {
        level_steps: LEVEL_STAIR,
        slope_descending: [10, 14, 14, 14, 6],
        slope_ascending: [9, 13, 13, 13, 5],
    },
    // ClearWest
    RampRecord {
        level_steps: LEVEL_STAIR,
        slope_descending: [9, 13, 13, 13, 5],
        slope_ascending: [12, 16, 16, 16, 8],
    },
];

/// The table for a straight shape.
pub fn ramp_record(shape: RampShape) -> &'static RampRecord {
    let index = match shape {
        RampShape::ClearSouth => 0,
        RampShape::ClearEast => 1,
        RampShape::ClearNorth => 2,
        RampShape::ClearWest => 3,
    };
    &RAMP_RECORDS[index]
}

/// The tile a slope value selects, relative to the theater's ramp base.
pub fn ramp_tile(ramp_base: i32, slope: u8) -> i32 {
    ramp_base + i32::from(slope) - 1
}

/// May a ramp be carved through this rect?
///
/// Two stages, and the whole thing bails on the first refusal:
///
/// 1. The four corners must be on the map.
/// 2. Every cell in the rect must be bare ground **and** belong either to the
///    carving region or to the lower one the ramp is reaching down to.
///
/// That second ownership term is why a ramp can cross the boundary between two
/// plateaus at all. Note the contrast with the ring-window quota test in
/// `connector`, which is handed the same lower-region id and ignores it — the
/// two look alike and mean different things.
///
/// The sweep itself is not diamond-guarded: the corner probes are what
/// establish the rect is on the map, exactly as in the original.
pub fn rect_is_carveable(
    grid: &mut RmgGrid,
    scratch: &RmgScratch,
    ids: &TileIds,
    playfield: &Playfield,
    rect: (i32, i32, i32, i32),
    region: i32,
    lower_region: i32,
) -> bool {
    let (rx, ry, w, h) = rect;
    let corners = [
        (rx, ry),
        (rx + w - 1, ry),
        (rx, ry + h - 1),
        (rx + w - 1, ry + h - 1),
    ];
    // The playfield, not the map diamond — they are not the same rectangle.
    // The playfield is inset by the map's local-size margins, so a rect out in
    // the border band passes the diamond and fails here, which is the whole
    // point: ramps do not get carved into the unplayable frame.
    //
    // Elevation-aware, because the carve asks for the raised form. A cell off
    // the grid has no level or slope to read, so it is refused outright rather
    // than probed at zero.
    for &(x, y) in &corners {
        let Ok(cx) = u16::try_from(x) else {
            return false;
        };
        let Ok(cy) = u16::try_from(y) else {
            return false;
        };
        let Some(cell) = grid.get(x, y) else {
            return false;
        };
        if !playfield.contains_raised(cx, cy, cell.level as i8, cell.slope) {
            return false;
        }
    }

    for row in 0..h {
        for col in 0..w {
            let (x, y) = (rx + col, ry + row);
            let owner = scratch.get(x, y).region;
            if owner != region && owner != lower_region {
                return false;
            }
            if !ids.is_clear(grid.cell_native(x, y).tile) {
                return false;
            }
        }
    }
    true
}
