//! Lepton coordinate system — sub-cell spatial precision matching RA2's internal units.
//!
//! RA2 uses **leptons** as its fundamental spatial unit: 256 leptons = 1 cell.
//! Each isometric cell spans 256×256 leptons, with retail ground-height levels at 104 leptons.
//!
//! To avoid SimFixed (I16F16, max 32767) overflow on large maps, we store lepton
//! positions as **cell coordinate + sub-cell offset** rather than absolute leptons.
//! The sub-cell offset (`sub_x`, `sub_y`) ranges from 0 to 256 within the cell,
//! with 128 being the cell center.
//!
//! ## Dependency rules
//! - util/ has NO dependencies on other game modules.

use crate::util::fixed_math::{SIM_ZERO, SimFixed};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Number of leptons per cell (256). This is RA2's fundamental spatial unit ratio.
pub const LEPTONS_PER_CELL: SimFixed = SimFixed::lit("256");

/// Integer form of the leptons-per-cell ratio, for shift/divide coordinate
/// math. The single authority for the value 256 across the crate — modules
/// that want a local name or width (`i64`, hex) derive from this constant
/// instead of re-declaring the literal. A test pins it equal to the
/// `SimFixed` form above.
pub const LEPTONS_PER_CELL_I32: i32 = 256;

/// Lepton offset for the center of a cell (128). Default sub-cell position.
pub const CELL_CENTER_LEPTON: SimFixed = SimFixed::lit("128");

/// Integer form of the cell-centre offset — the authority for the value 128,
/// like `LEPTONS_PER_CELL_I32` for 256. A test pins the two forms equal.
pub const CELL_CENTER_LEPTON_I32: i32 = 128;

/// Tile width in pixels (60.0) divided by leptons per cell (256).
/// Pre-computed for efficient lepton → screen pixel conversion.
/// = 60.0 / 256.0 = 0.234375
#[cfg(test)]
pub(crate) const SCREEN_X_PER_LEPTON: f32 = 60.0 / 256.0;

/// Tile half-height in pixels (15.0) divided by leptons per cell (256).
/// Pre-computed for efficient lepton → screen pixel conversion.
/// = 30.0 / 256.0 = 0.1171875
#[cfg(test)]
pub(crate) const SCREEN_Y_PER_LEPTON: f32 = 30.0 / 256.0;

/// Screen-Y offset every VERA world layer carries relative to the original's
/// absolute tactical pixel.
///
/// The original starts a cell's tile bounding box on row `15*(rx+ry)`; VERA's
/// `map::terrain::iso_to_screen` starts it half a tile lower and the entity
/// projection here carries the identical term, so the *relation* between the
/// layers — the only thing a player can see — matches. Being constant across
/// every layer it is absorbed by the camera and is invisible.
///
/// It is load-bearing that both helpers carry it: pulling the tile helper down
/// to the original's absolute row without pulling this one down in the same
/// edit re-opens the half-tile bug this constant was added to close.
const WORLD_ROW_BIAS_PX: i32 = 15;

/// Raw-bit scale of the I16F16 sub-cell representation.
const SIM_FIXED_SCALE: i64 = 1 << 16;

// ---------------------------------------------------------------------------
// Infantry sub-cell lepton positions (RA2 canonical)
// ---------------------------------------------------------------------------

/// Sub-cell lepton offsets within a cell. RA2 defines 5 sub-cell positions (0–4).
/// Extracted from the original engine's runtime init.
///
/// - Sub-cell 0: center  (128, 128) — vehicles + default
/// - Sub-cell 1: top-left  ( 64,  64)
/// - Sub-cell 2: top-right (192,  64)
/// - Sub-cell 3: bottom-left  ( 64, 192) — used for infantry placement
/// - Sub-cell 4: bottom-right (192, 192) — used for infantry placement

/// Same values as `sim::cell_kernel::INFANTRY_SUBCELL_OFFSETS` (the
/// native-ordered integer table); change both or neither.
/// Sub-cell center position (sub-cell 0, and fallback for vehicles).
pub const SUBCELL_CENTER_X: SimFixed = SimFixed::lit("128");
pub const SUBCELL_CENTER_Y: SimFixed = SimFixed::lit("128");

/// Sub-cell 1: top-left within the cell diamond.
pub const SUBCELL_1_X: SimFixed = SimFixed::lit("64");
pub const SUBCELL_1_Y: SimFixed = SimFixed::lit("64");

/// Sub-cell 2: top-right within the cell diamond.
pub const SUBCELL_2_X: SimFixed = SimFixed::lit("192");
pub const SUBCELL_2_Y: SimFixed = SimFixed::lit("64");

/// Sub-cell 3: bottom-left (SW quadrant) within the cell diamond.
pub const SUBCELL_3_X: SimFixed = SimFixed::lit("64");
pub const SUBCELL_3_Y: SimFixed = SimFixed::lit("192");

/// Sub-cell 4: bottom-right (SE quadrant) within the cell diamond.
pub const SUBCELL_4_X: SimFixed = SimFixed::lit("192");
pub const SUBCELL_4_Y: SimFixed = SimFixed::lit("192");

// ---------------------------------------------------------------------------
// InRange (3D distance) constants
// ---------------------------------------------------------------------------

/// Active-retail 104-lepton level step. Cell, Foot, Techno/InRange, area-
/// damage, Anim, Bullet, Particle, Unit, and VXL own independently initialized
/// native globals, but every captured value is 104.
pub const LEPTONS_PER_LEVEL: i64 = 104;

/// Verified active-retail ground-height scalar. Bridge/deck height is a
/// caller-owned conditional addition and is never included here.
pub const GROUND_LEVEL_HEIGHT_LEPTONS: i32 = 104;

/// Sentinel weapon range meaning "always in range". When the configured
/// weapon range equals -512 leptons, InRange short-circuits to true regardless
/// of distance or other gates. Used by some special weapons.
pub const WEAPON_RANGE_ALWAYS_IN_RANGE_LEPTONS: i64 = -512;

/// Lepton threshold dividing low-flying from high-flying aircraft for InRange
/// gating. Aircraft below this altitude are treated as low-flying (target Z
/// snapped to ground for range checks); at or above, they're high-flying
/// (AirRangeBonus may apply).
///
/// The native ObjectClass predicate compares height against twice the
/// independently initialized 104-lepton flight-level scalar.
pub const HIGH_FLIGHT_THRESHOLD_LEPTONS: i64 = 2 * LEPTONS_PER_LEVEL;

/// Range/LOS cell-to-deck delta. Kept separate from entity OnBridge coordinate
/// selection even though both active retail values are 416 leptons.
pub const BRIDGE_HEIGHT_DELTA_LEPTONS: i64 = 416;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedGroundSlope(pub u8);

const fn ground_slope_records(g: i32) -> [(i32, i32, i32, i32, i32); 21] {
    [
        (0, 0, 0, 0, 0),
        (1, 0, 0, g, 0),
        (0, 1, 0, g, 0),
        (-1, 0, g, g, 0),
        (0, -1, g, g, 0),
        (1, 1, -g, g, 0),
        (-1, 1, 0, g, 0),
        (-1, -1, g, g, 0),
        (1, -1, 0, g, 0),
        (1, 1, 0, g, 0),
        (-1, 1, g, g, 0),
        (-1, -1, 2 * g, g, 0),
        (1, -1, g, g, 0),
        (1, 1, 0, 2 * g, 0),
        (-1, 1, g, 2 * g, 0),
        (-1, -1, 2 * g, 2 * g, 0),
        (1, -1, g, 2 * g, 0),
        (0, 0, 0, g / 2, g / 2),
        (0, 0, g, g / 2, -g / 2),
        (0, 0, 0, g / 2, g / 2),
        (0, 0, g, g / 2, -g / 2),
    ]
}

const GROUND_SLOPE_RECORDS: [(i32, i32, i32, i32, i32); 21] =
    ground_slope_records(GROUND_LEVEL_HEIGHT_LEPTONS);

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

/// Active-retail ground-only Cell surface calculation in world leptons.
///
/// gamemd-derived: `CellClass::ComputeGroundHeightAtCoord @ 0x0047B3A0`
/// sign-extends Level, applies the independently initialized 104-lepton Cell
/// scalar, evaluates low-byte X/Y against the 21-record slope table, clamps the
/// slope term before adding the base, and chops toward zero. Bridge height is
/// deliberately composed by callers such as `CellClass::GetTargetCoords`.
///
/// Residual kept outside this pure evaluator: the conditional placed-TMP
/// lifecycle that applies per-subtile `+0x28` to CellClass Level.
pub fn ground_height_leptons(
    level_byte: u8,
    slope: u8,
    world_x: i32,
    world_y: i32,
) -> Result<i32, UnsupportedGroundSlope> {
    ground_height_with_level_height(
        level_byte,
        slope,
        world_x,
        world_y,
        GROUND_LEVEL_HEIGHT_LEPTONS,
        &GROUND_SLOPE_RECORDS,
    )
}

fn ground_height_with_level_height(
    level_byte: u8,
    slope: u8,
    world_x: i32,
    world_y: i32,
    level_height: i32,
    records: &[(i32, i32, i32, i32, i32); 21],
) -> Result<i32, UnsupportedGroundSlope> {
    let signed_level = i32::from(level_byte as i8);
    let level_numerator = signed_level.wrapping_mul(level_height).wrapping_mul(256);
    let base = level_numerator.wrapping_add(128) / 256;
    if slope == 0 {
        return Ok(base);
    }
    let Some(&(coefficient_x, coefficient_y, bias_a, maximum, bias_b)) =
        records.get(slope as usize)
    else {
        return Err(UnsupportedGroundSlope(slope));
    };
    let local_x = (world_x as u32 & 0xff) as i32;
    let local_y = (world_y as u32 & 0xff) as i32;
    let slope_numerator = local_y
        .wrapping_mul(coefficient_y)
        .wrapping_add(local_x.wrapping_mul(coefficient_x))
        .wrapping_mul(level_height)
        .wrapping_add(bias_a.wrapping_add(bias_b).wrapping_mul(256))
        .clamp(0, maximum.wrapping_mul(256));
    Ok(base.wrapping_mul(256).wrapping_add(slope_numerator) / 256)
}

/// Get the lepton sub-cell offset for a given sub-cell index (0–4).
///
/// Returns `(sub_x, sub_y)` in lepton units (0..256 range).
/// Unknown indices default to cell center.
pub fn subcell_lepton_offset(sub_cell: Option<u8>) -> (SimFixed, SimFixed) {
    match sub_cell {
        Some(1) => (SUBCELL_1_X, SUBCELL_1_Y),
        Some(2) => (SUBCELL_2_X, SUBCELL_2_Y),
        Some(3) => (SUBCELL_3_X, SUBCELL_3_Y),
        Some(4) => (SUBCELL_4_X, SUBCELL_4_Y),
        _ => (SUBCELL_CENTER_X, SUBCELL_CENTER_Y), // 0 + fallback
    }
}

/// Convert a sub-cell lepton offset to a screen-space pixel offset from cell center.
///
/// The isometric projection maps lepton offsets from center (128, 128) to pixels:
///   dx_pixels = (sub_x - sub_y) * (TILE_WIDTH / 2) / 256
///   dy_pixels = (sub_x + sub_y - 256) * (TILE_HEIGHT / 2) / 256
///
/// This replaces hardcoded pixel offsets with lepton-derived values.
#[cfg(test)]
pub fn lepton_sub_to_screen_offset(sub_x: SimFixed, sub_y: SimFixed) -> (f32, f32) {
    let dx_lep: f32 = sub_x.to_num::<f32>() - CELL_CENTER_LEPTON.to_num::<f32>();
    let dy_lep: f32 = sub_y.to_num::<f32>() - CELL_CENTER_LEPTON.to_num::<f32>();
    // Isometric projection: offset from center
    let screen_dx: f32 = (dx_lep - dy_lep) * SCREEN_X_PER_LEPTON / 2.0;
    let screen_dy: f32 = (dx_lep + dy_lep) * SCREEN_Y_PER_LEPTON / 2.0;
    (screen_dx, screen_dy)
}

/// Project a signed absolute world X/Y coordinate using active YR's integer
/// operation order.
///
/// gamemd-derived: active YR `TacticalClass::CoordsToClient2` at `0x006D2140`
/// forms each 60/30-pixel numerator in signed integer arithmetic and divides
/// the complete result by 256, truncating toward zero.
pub(crate) fn project_absolute_lepton_xy(world_x_leptons: i32, world_y_leptons: i32) -> (i32, i32) {
    let world_x = i64::from(world_x_leptons);
    let world_y = i64::from(world_y_leptons);
    let screen_x_numerator = world_x * 60 / 2 + world_y * -60 / 2;
    let screen_y_numerator = world_x * 30 / 2 + world_y * 30 / 2;
    (
        (screen_x_numerator / 256) as i32,
        (screen_y_numerator / 256) as i32,
    )
}

/// Project an exact signed XYZ world coordinate into VERA's world-pixel frame.
pub(crate) fn absolute_leptons_to_screen(
    world_x_leptons: i32,
    world_y_leptons: i32,
    world_z_leptons: i32,
) -> (f32, f32) {
    let (screen_x, screen_y) = project_absolute_lepton_xy(world_x_leptons, world_y_leptons);
    let screen_y = screen_y + WORLD_ROW_BIAS_PX
        - crate::util::native_x87::adjust_for_z_standard(world_z_leptons);
    (screen_x as f32, screen_y as f32)
}

fn absolute_lepton_axis(cell: u16, sub_cell: SimFixed) -> i32 {
    let absolute_fixed = i64::from(cell) * 256 * SIM_FIXED_SCALE + i64::from(sub_cell.to_bits());
    (absolute_fixed / SIM_FIXED_SCALE) as i32
}

/// Where an entity standing at these lepton coordinates is drawn — **the entity
/// frame**, the projection of the object's own coordinate.
///
/// Cell and sub-cell inputs first become one signed absolute integer-lepton
/// coordinate. The native 60/30-pixel numerators are then divided by 256 with
/// signed truncation toward zero; only afterward is VERA's common +15 row bias
/// applied and exact `AdjustForZ` subtracted.
///
/// At the cell centre (`sub_x = sub_y = 128`) this lands on the **centre of the
/// cell's diamond**, which is half a tile *below* the row
/// `map::terrain::iso_to_screen` starts that cell's tile art on. That gap is
/// deliberate and is what the original does: it draws every class on the
/// object's own coordinate and starts tile art on the box top, so a unit's feet
/// meet the middle of the tile it stands on rather than its northern vertex.
///
/// `map::terrain::lepton_to_screen` is the absolute-lepton twin of this
/// function and produces the identical point; `matching_lepton_projections_agree`
/// pins that.
///
/// **Buildings are the one class that does not draw here.** The original gives
/// `BuildingClass` its own render-coordinate virtual which takes half a cell off
/// both axes, moving its anchor back onto the tile row of its north-west
/// footprint cell. That shift is owned by
/// `render::locomotor_visual::BUILDING_ART_LIFT_PX`, not by this function —
/// every other consumer here wants the plain projection.
pub(crate) fn lepton_to_screen_exact_z(
    rx: u16,
    ry: u16,
    sub_x: SimFixed,
    sub_y: SimFixed,
    world_z_leptons: i32,
) -> (f32, f32) {
    // Chop only after cell and sub-cell state form the complete absolute
    // coordinate. Chopping `sub_x`/`sub_y` first moves negative fractional
    // offsets across a native pixel boundary.
    let world_x = absolute_lepton_axis(rx, sub_x);
    let world_y = absolute_lepton_axis(ry, sub_y);
    absolute_leptons_to_screen(world_x, world_y, world_z_leptons)
}

pub fn lepton_to_screen(rx: u16, ry: u16, sub_x: SimFixed, sub_y: SimFixed, z: u8) -> (f32, f32) {
    lepton_to_screen_exact_z(
        rx,
        ry,
        sub_x,
        sub_y,
        i32::from(z as i8) * GROUND_LEVEL_HEIGHT_LEPTONS,
    )
}

/// Compute lepton direction vector and length for a cell-to-cell step.
///
/// `dx`, `dy` are the cell delta (each in {-1, 0, +1}).
/// Returns `(dir_x, dir_y, length)` where dir is in leptons and length is
/// 256 for cardinal moves, ~362 for diagonal moves.
/// Returns `(0, 0, 0)` if dx=dy=0.
pub fn cell_delta_to_lepton_dir(dx: i32, dy: i32) -> (SimFixed, SimFixed, SimFixed) {
    if dx == 0 && dy == 0 {
        return (SIM_ZERO, SIM_ZERO, SIM_ZERO);
    }
    let dir_x: SimFixed = SimFixed::from_num(dx * 256);
    let dir_y: SimFixed = SimFixed::from_num(dy * 256);
    // dx,dy ∈ {-1,0,+1} so length is exactly 256 (cardinal) or sqrt(2)*256 ≈ 362 (diagonal).
    // Compute directly to avoid I16F16 overflow from 256*256.
    let is_diagonal: bool = dx != 0 && dy != 0;
    let len: SimFixed = if is_diagonal {
        // sqrt(2) * 256 ≈ 362.038... — use SimFixed::lit for deterministic precision.
        SimFixed::lit("362.038")
    } else {
        SimFixed::from_num(256)
    };
    (dir_x, dir_y, len)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_and_fixed_leptons_per_cell_agree() {
        assert_eq!(LEPTONS_PER_CELL, SimFixed::from_num(LEPTONS_PER_CELL_I32));
        assert_eq!(
            CELL_CENTER_LEPTON,
            SimFixed::from_num(CELL_CENTER_LEPTON_I32)
        );
    }
    use crate::map::terrain;

    #[test]
    fn gsi_13_02_signed_projection_truncates_toward_zero_at_pixel_boundaries() {
        assert_eq!(project_absolute_lepton_xy(5, 0).0, 0);
        assert_eq!(project_absolute_lepton_xy(-5, 0).0, 0);
        assert_eq!(project_absolute_lepton_xy(9, 0).0, 1);
        assert_eq!(project_absolute_lepton_xy(-9, 0).0, -1);
    }

    #[test]
    fn gsi_13_02_absolute_subcell_projection_matches_retail_fixtures() {
        assert_eq!(
            lepton_to_screen(10, 10, SimFixed::from_num(133), SimFixed::from_num(128), 0,),
            (0.0, 330.0),
        );
        assert_eq!(
            lepton_to_screen(10, 10, SimFixed::from_num(123), SimFixed::from_num(128), 0,),
            (0.0, 329.0),
        );
    }

    #[test]
    fn gsi_13_02_util_and_terrain_share_the_exact_absolute_projector() {
        for (sub_x, sub_y) in [
            (SUBCELL_CENTER_X, SUBCELL_CENTER_Y),
            (SUBCELL_1_X, SUBCELL_1_Y),
            (SUBCELL_2_X, SUBCELL_2_Y),
            (SUBCELL_3_X, SUBCELL_3_Y),
            (SUBCELL_4_X, SUBCELL_4_Y),
        ] {
            let util = lepton_to_screen(10, 4, sub_x, sub_y, 2);
            let terrain = terrain::lepton_to_screen(glam::IVec3::new(
                absolute_lepton_axis(10, sub_x),
                absolute_lepton_axis(4, sub_y),
                2 * GROUND_LEVEL_HEIGHT_LEPTONS,
            ));
            assert_eq!(util, terrain, "canonical subcell ({sub_x}, {sub_y})");
        }

        let negative = lepton_to_screen(0, 0, SimFixed::from_num(-9), SimFixed::from_num(5), 0);
        assert_eq!(
            negative,
            terrain::lepton_to_screen(glam::IVec3::new(-9, 5, 0))
        );

        let crossing = lepton_to_screen(1, 0, SimFixed::lit("-0.5"), SIM_ZERO, 0);
        assert_eq!(crossing, (29.0, 29.0));
        assert_eq!(
            crossing,
            terrain::lepton_to_screen(glam::IVec3::new(255, 0, 0)),
            "the complete 255.5-lepton absolute coordinate chops to 255",
        );
    }

    #[test]
    fn gsi_04_03b_ground_height_matches_all_verified_slope_records() {
        let expected = [
            208, 234, 286, 286, 234, 208, 260, 208, 208, 312, 312, 312, 260, 312, 364, 312, 260,
            260, 260, 260, 260,
        ];
        for (slope, expected) in expected.into_iter().enumerate() {
            assert_eq!(
                ground_height_leptons(2, slope as u8, 64, 192),
                Ok(expected),
                "slope {slope}",
            );
        }
        assert_eq!(
            ground_height_leptons(0, 21, 0, 0),
            Err(UnsupportedGroundSlope(21))
        );
    }

    #[test]
    fn gsi_04_03b_ground_height_signed_level_and_ftol_chop() {
        assert_eq!(ground_height_leptons(0xff, 0, 0, 0), Ok(-103));
        assert_eq!(ground_height_leptons(0xff, 1, 1, 0), Ok(-102));
    }

    #[test]
    fn gsi_04_03_cellclass_ground_height_uses_the_104_lepton_surface_domain() {
        let center_contributions = [
            0, 52, 52, 52, 52, 0, 0, 0, 0, 104, 104, 104, 104, 104, 104, 104, 104, 52, 52, 52, 52,
        ];
        for (slope, contribution) in center_contributions.into_iter().enumerate() {
            assert_eq!(
                ground_height_leptons(2, slope as u8, 128, 128),
                Ok(208 + contribution),
                "active retail slope {slope}",
            );
        }
        assert_eq!(ground_height_leptons(0, 0, 128, 128), Ok(0));
        assert_eq!(ground_height_leptons(1, 0, 128, 128), Ok(104));
        assert_eq!(ground_height_leptons(2, 0, 128, 128), Ok(208));
        assert_eq!(
            ground_height_leptons(0xff, 0, 128, 128),
            Ok(-103),
            "0x0047B3A0 adds 0.5 then chops the signed base toward zero",
        );
        assert_eq!(
            ground_height_leptons(0, 21, 128, 128),
            Err(UnsupportedGroundSlope(21)),
        );
    }

    /// The raw level byte is sign-extended before its 104-lepton world Z goes
    /// through `AdjustForZ`; negative Z therefore retains native's asymmetric
    /// `+0.5` then truncate-toward-zero result.
    #[test]
    fn gsi_04_03b_lepton_projection_sign_extends_raw_level() {
        let (_, sy_zero) = lepton_to_screen(0, 0, CELL_CENTER_LEPTON, CELL_CENTER_LEPTON, 0);
        let (_, sy_minus_one) =
            lepton_to_screen(0, 0, CELL_CENTER_LEPTON, CELL_CENTER_LEPTON, 0xff);
        assert_eq!(sy_zero, 30.0);
        assert_eq!(sy_minus_one, 44.0);
        assert_eq!(sy_minus_one - sy_zero, 14.0);
    }

    /// An entity standing on a cell is drawn on that cell's **diamond centre**,
    /// half a tile east and half a tile south of where the tile art starts.
    ///
    /// This is the relation the original has and the one VERA got wrong: the
    /// entity projection used to return the tile's bounding-box top, so every
    /// unit, infantryman and aircraft was drawn half a tile north of the ground
    /// it stood on. Pinned as an exact half-tile on **both** axes so a future
    /// edit cannot quietly collapse the two frames back together.
    #[test]
    fn cell_centre_is_the_tile_diamond_centre() {
        for rx in [0u16, 5, 10, 50] {
            for ry in [0u16, 3, 10, 50] {
                for z in [0u8, 2, 4] {
                    let (corner_sx, corner_sy) = terrain::iso_to_screen(rx, ry, z);
                    let (actual_sx, actual_sy) =
                        lepton_to_screen(rx, ry, CELL_CENTER_LEPTON, CELL_CENTER_LEPTON, z);
                    assert!(
                        (actual_sx - (corner_sx + terrain::TILE_WIDTH / 2.0)).abs() < 0.01,
                        "X at ({rx}, {ry}, z={z}): expected {}, got {actual_sx}",
                        corner_sx + terrain::TILE_WIDTH / 2.0,
                    );
                    assert!(
                        (actual_sy - (corner_sy + terrain::TILE_HEIGHT / 2.0)).abs() < 0.01,
                        "Y at ({rx}, {ry}, z={z}): expected {}, got {actual_sy}",
                        corner_sy + terrain::TILE_HEIGHT / 2.0,
                    );
                }
            }
        }
    }

    /// The two public lepton projections accept different coordinate shapes —
    /// this one takes cell + sub-cell, `map::terrain`'s takes absolute leptons —
    /// and must reach the same shared integer transform at every sub-cell value.
    ///
    /// They disagreed by exactly half a tile before the entity anchor was fixed,
    /// which is why particles (on the terrain twin) and units (on this one) sat
    /// on different rows over the same ground. Pinned so they cannot part again.
    #[test]
    fn matching_lepton_projections_agree() {
        for (rx, ry) in [(0u16, 0u16), (10, 4), (7, 19), (63, 1)] {
            for sub_x in [0i32, 64, 128, 192, 255] {
                for sub_y in [0i32, 64, 128, 192, 255] {
                    let (ax, ay) = lepton_to_screen(
                        rx,
                        ry,
                        SimFixed::from_num(sub_x),
                        SimFixed::from_num(sub_y),
                        0,
                    );
                    let (bx, by) = terrain::lepton_to_screen(glam::IVec3::new(
                        i32::from(rx) * 256 + sub_x,
                        i32::from(ry) * 256 + sub_y,
                        0,
                    ));
                    assert!(
                        (ax - bx).abs() < 0.01 && (ay - by).abs() < 0.01,
                        "cell ({rx},{ry}) sub ({sub_x},{sub_y}): util {:?} vs terrain {:?}",
                        (ax, ay),
                        (bx, by),
                    );
                }
            }
        }
    }

    #[test]
    fn subcell_3_offsets_bottom_left() {
        let (dx, _dy) = lepton_sub_to_screen_offset(SUBCELL_3_X, SUBCELL_3_Y);
        // Sub-cell 3 (64, 192): dx_lep = -64, dy_lep = +64.
        // Isometric: screen_dx = -15 (left of center), screen_dy = 0.
        assert!(dx < 0.0, "Sub-cell 3 should be left of center: dx={}", dx);
    }

    #[test]
    fn subcell_4_offsets_bottom_right() {
        let (dx, _dy) = lepton_sub_to_screen_offset(SUBCELL_4_X, SUBCELL_4_Y);
        // Sub-cell 4 (192, 192): dx_lep = +64, dy_lep = +64.
        // Isometric: screen_dx = 0, screen_dy = +7.5 (below center).
        assert!(
            dx.abs() < 1.0,
            "Sub-cell 4 should be near center X: dx={}",
            dx
        );
    }

    #[test]
    fn subcell_center_has_zero_offset() {
        let (dx, dy) = lepton_sub_to_screen_offset(SUBCELL_CENTER_X, SUBCELL_CENTER_Y);
        assert!(
            dx.abs() < 0.001,
            "Center sub-cell X offset should be ~0: {}",
            dx
        );
        assert!(
            dy.abs() < 0.001,
            "Center sub-cell Y offset should be ~0: {}",
            dy
        );
    }

    #[test]
    fn subcell_lookup_returns_correct_positions() {
        assert_eq!(
            subcell_lepton_offset(None),
            (SUBCELL_CENTER_X, SUBCELL_CENTER_Y)
        );
        assert_eq!(
            subcell_lepton_offset(Some(0)),
            (SUBCELL_CENTER_X, SUBCELL_CENTER_Y)
        );
        assert_eq!(subcell_lepton_offset(Some(1)), (SUBCELL_1_X, SUBCELL_1_Y));
        assert_eq!(subcell_lepton_offset(Some(2)), (SUBCELL_2_X, SUBCELL_2_Y));
        assert_eq!(subcell_lepton_offset(Some(3)), (SUBCELL_3_X, SUBCELL_3_Y));
        assert_eq!(subcell_lepton_offset(Some(4)), (SUBCELL_4_X, SUBCELL_4_Y));
    }

    #[test]
    fn leptons_per_cell_is_256() {
        assert_eq!(LEPTONS_PER_CELL.to_num::<i32>(), 256);
    }

    #[test]
    fn cell_delta_to_lepton_dir_cardinal() {
        let (dx, dy, len) = cell_delta_to_lepton_dir(1, 0);
        assert_eq!(dx.to_num::<i32>(), 256);
        assert_eq!(dy.to_num::<i32>(), 0);
        assert_eq!(len.to_num::<i32>(), 256);
    }

    #[test]
    fn cell_delta_to_lepton_dir_diagonal() {
        let (dx, dy, len) = cell_delta_to_lepton_dir(1, 1);
        assert_eq!(dx.to_num::<i32>(), 256);
        assert_eq!(dy.to_num::<i32>(), 256);
        // sqrt(256² + 256²) ≈ 362
        let len_i: i32 = len.to_num();
        assert!(
            len_i >= 361 && len_i <= 363,
            "diagonal len should be ~362, got {}",
            len_i
        );
    }

    #[test]
    fn cell_delta_to_lepton_dir_zero() {
        let (dx, dy, len) = cell_delta_to_lepton_dir(0, 0);
        assert_eq!(dx.to_num::<i32>(), 0);
        assert_eq!(dy.to_num::<i32>(), 0);
        assert_eq!(len.to_num::<i32>(), 0);
    }
}
