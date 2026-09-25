//! Pad geometry — lepton→cell conversion for docking pad cells.
//!
//! Single source of truth for converting a building's (origin + foundation) +
//! a pad's lepton offset into the cell where the docked unit parks, for
//! aircraft docking descent (`sim::docking::aircraft_dock`). The refinery pad
//! is the fixed NW+(3,1) cell the DOCKING receiver sends
//! (`radio::receive::dock_pad_cell`).
//!
//! Offsets are interpreted as **building-center-relative**, not origin-
//! relative: the original game computes a pad coordinate by adding the
//! `DockingOffset%d` lepton vector to the building's geometric center
//! (`origin_lepton + ((W-1)*128, (H-1)*128)`), and we replicate that here.
//! Treating the offset as origin-relative would shift multi-pad airfields one
//! cell northwest of their retail position — a visible parity drift.
//!
//! ## Dependency rules
//! - Part of `sim/` — depends only on `rules/`.
//! - `sim/` NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::rules::object_type::DockPad;

/// Convert a building's (origin + foundation) + a pad's lepton offset into
/// the pad's cell coordinates.
///
/// - `origin` is the building's top-left cell (`(rx, ry)`).
/// - `foundation` is `(width, height)` in cells, used to compute the
///   geometric center.
/// - `pad.lepton_offset` is the center-relative offset in leptons
///   (256 leptons = 1 cell).
///
/// The `+128` half-cell rounding ensures lepton coordinates near cell
/// boundaries snap to the visually correct cell (e.g. lepton 128 → cell 1,
/// not cell 0).
pub fn pad_cell_for(origin: (u16, u16), foundation: (u16, u16), pad: &DockPad) -> (u16, u16) {
    let (rx, ry) = origin;
    let (w, h) = foundation;
    // Geometric-center offset (in leptons) from the origin cell's top-left.
    let center_off_x = (w as i32 - 1) * 128;
    let center_off_y = (h as i32 - 1) * 128;
    let (dx, dy, _dz) = pad.lepton_offset;
    let cx = (center_off_x + dx + 128).div_euclid(256);
    let cy = (center_off_y + dy + 128).div_euclid(256);
    (
        (rx as i32 + cx).max(0) as u16,
        (ry as i32 + cy).max(0) as u16,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(x: i32, y: i32, z: i32) -> DockPad {
        DockPad {
            lepton_offset: (x, y, z),
        }
    }

    #[test]
    fn one_by_one_zero_offset_returns_origin() {
        // 1x1 building: geometric center == origin cell, so offset (0,0) = origin.
        assert_eq!(pad_cell_for((10, 10), (1, 1), &pad(0, 0, 0)), (10, 10));
    }

    #[test]
    fn one_by_one_positive_offset_one_cell() {
        // 1x1 building: center == origin. +256 leptons = +1 cell.
        assert_eq!(pad_cell_for((10, 10), (1, 1), &pad(256, 0, 0)), (11, 10));
        assert_eq!(pad_cell_for((10, 10), (1, 1), &pad(0, 256, 0)), (10, 11));
    }

    #[test]
    fn negative_offset_is_clamped_to_zero() {
        // 1x1 at (1,1) with offset (-512, 0) would land at (-1, 1); clamp to (0, 1).
        assert_eq!(pad_cell_for((1, 1), (1, 1), &pad(-512, 0, 0)), (0, 1));
    }

    #[test]
    fn gaairc_four_pads_match_center_relative_math() {
        // GAAIRC: Foundation=3x2, art.ini offsets verified live 2026-05-11.
        // Building center is at (W-1)*128, (H-1)*128 = (256, 128) leptons from origin.
        let origin = (20, 20);
        let foundation = (3, 2);
        // DockingOffset0=0,-128,0:
        //   center_off=(256, 128), pad_off=(0, -128), total=(256+0+128, 128-128+128)=(384, 128)
        //   cell offset = (1, 0). pad cell = (21, 20).
        assert_eq!(pad_cell_for(origin, foundation, &pad(0, -128, 0)), (21, 20));
        // DockingOffset1=0,128,0 → (384, 384), cell offset (1, 1), pad cell (21, 21).
        assert_eq!(pad_cell_for(origin, foundation, &pad(0, 128, 0)), (21, 21));
        // DockingOffset2=256,-128,0 → (640, 128), cell offset (2, 0), pad cell (22, 20).
        assert_eq!(
            pad_cell_for(origin, foundation, &pad(256, -128, 0)),
            (22, 20)
        );
        // DockingOffset3=256,128,0 → (640, 384), cell offset (2, 1), pad cell (22, 21).
        assert_eq!(
            pad_cell_for(origin, foundation, &pad(256, 128, 0)),
            (22, 21)
        );
    }

    #[test]
    fn nadept_single_pad_4x3_depot() {
        // NADEPT: Foundation=4x3, art.ini DockingOffset0=128,0,0.
        // Center offset = ((4-1)*128, (3-1)*128) = (384, 256).
        // Total = (384+128+128, 256+0+128) = (640, 384).
        // Cell offset = (640/256, 384/256) = (2, 1). pad cell = (origin+2, origin+1).
        assert_eq!(pad_cell_for((30, 30), (4, 3), &pad(128, 0, 0)), (32, 31));
    }

    #[test]
    fn z_coord_does_not_affect_cell() {
        // Z is for rendering altitude only; cell is X/Y based.
        assert_eq!(pad_cell_for((10, 10), (1, 1), &pad(0, 0, 999)), (10, 10));
    }
}
