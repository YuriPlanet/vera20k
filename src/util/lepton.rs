//! Lepton coordinate system — sub-cell spatial precision matching RA2's internal units.
//!
//! RA2 uses **leptons** as its fundamental spatial unit: 256 leptons = 1 cell.
//! Each isometric cell spans 256×256 leptons, with retail ground-height levels at 104 leptons.
//!
//! To avoid SimFixed (I16F16, max 32767) overflow on large maps, we store lepton
//! positions as **cell coordinate + sub-cell offset** rather than absolute leptons.
//! Map sub-cell offsets use 128 for the cell center. Native signed coordinate
//! packets truncate toward zero, retaining a negative sub-cell remainder for
//! negative world positions.
//!
//! ## Dependency rules
//! - util/ has NO dependencies on other game modules.

use crate::util::fixed_math::SimFixed;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Leptons per cell (256), RA2's fundamental spatial unit ratio, for
/// shift/divide coordinate math. The single authority for the value 256
/// across the crate — modules that want a local name or width (`i64`, hex)
/// derive from this constant instead of re-declaring the literal.
pub const LEPTONS_PER_CELL_I32: i32 = 256;

/// Lepton offset for the center of a cell (128). Default sub-cell position.
pub const CELL_CENTER_LEPTON: SimFixed = SimFixed::lit("128");

/// Integer form of the cell-centre offset — the authority for the value 128,
/// like `LEPTONS_PER_CELL_I32` for 256. A test pins the two forms equal.
pub const CELL_CENTER_LEPTON_I32: i32 = 128;

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
pub(crate) const WORLD_ROW_BIAS_PX: i32 = 15;

/// Raw-bit scale of the I16F16 sub-cell representation.
const SIM_FIXED_SCALE: i64 = 1 << 16;

// ---------------------------------------------------------------------------
// Infantry sub-cell lepton positions (RA2 canonical)
// ---------------------------------------------------------------------------

/// Sub-cell lepton offsets within a cell, by spot (0–4), from the cell's
/// north corner. Extracted from the original engine's runtime init.
///
/// - Sub-cell 0: center  (128, 128) — vehicles + default
/// - Sub-cell 1: top-left  ( 64,  64)
/// - Sub-cell 2: top-right (192,  64)
/// - Sub-cell 3: bottom-left  ( 64, 192) — used for infantry placement
/// - Sub-cell 4: bottom-right (192, 192) — used for infantry placement
pub const SUBCELL_OFFSETS: [(i32, i32); 5] =
    [(128, 128), (64, 64), (192, 64), (64, 192), (192, 192)];

// ---------------------------------------------------------------------------
// InRange (3D distance) constants
// ---------------------------------------------------------------------------

/// Active-retail 104-lepton level step. Cell, Foot, Techno/InRange, area-
/// damage, Anim, Bullet, Particle, Unit, and VXL own independently initialized
/// native globals, but every captured value is 104.
pub const LEPTONS_PER_LEVEL: i64 = 104;

/// Verified active-retail ground-height scalar. Bridge/deck height is a
/// caller-owned conditional addition and is never included here.
pub const GROUND_LEVEL_HEIGHT_LEPTONS: i32 = LEPTONS_PER_LEVEL as i32;

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

/// Signed lepton→cell, truncating toward zero: gamemd's `CDQ; AND EDX,0xFF;
/// ADD; SAR 8` idiom (e.g. `ObjectClass::Get_Cell_Packed` `0x0041BEA0` and
/// the Unit readiness, ore-scan and Foot path copies inlined at `0x004D404A`
/// and `0x004DCF3E`). `v / 256` agrees; `div_euclid(256)` and a bare `>> 8`
/// floor negative leptons instead.
pub const fn lepton_to_cell(v: i32) -> i32 {
    (v + ((v >> 31) & 0xFF)) >> 8
}

/// [`lepton_to_cell`] stored into a 16-bit `CellStruct` field.
pub const fn lepton_to_cell_packed(v: i32) -> i16 {
    lepton_to_cell(v) as i16
}

/// Signed native cell word and the remaining integer leptons. AreaDamage
///489309..489330 and Object41BEA0 use the shared truncating conversion above.
/// Consumed native query controls: `tools/spatial_oracle/map_queries.json`.
/// The remainder is signed too: -257 is cell -1 plus -1 lepton, not cell0+255.
/// The packed word intentionally wraps outside the native signed-cell domain;
/// callers retaining an arbitrary absolute coordinate must keep that coordinate.
pub(crate) fn split_lepton_axis(leptons: i32) -> (u16, SimFixed) {
    let cell = lepton_to_cell(leptons);
    (
        cell as i16 as u16,
        SimFixed::from_num(leptons - cell * LEPTONS_PER_CELL_I32),
    )
}

/// Levels a high bridge deck sits above the cell that carries it.
///
/// Native code spells this 4 two ways: as an immediate (`GetEffectiveHeight`
/// adds `((flags >> 7) & 1) * 4` for an anchor cell) and as the lepton deck
/// global below divided by the level step. Both are fixed, never INI-read.
pub const BRIDGE_DECK_HEIGHT_LEVELS: i32 = 4;

/// Leptons a high bridge deck sits above the ground height of its cell.
///
/// gamemd keeps separate globals for this value, each initialized once to four
/// times a 104-lepton level step: Foot `[0x00AC13BC]` (stored by
/// `0x005F37C0..0x005F3890`, `LEA ECX,[EAX*4]` at `0x005F3866`, added by
/// `ObjectClass::SetHeight 0x005F5FA0`), Bounce `[0x0089C76C]` (initializer
/// `0x00439610`, read by `0x00439A10`/`0x00439B00`), Jumpjet `[0x00ABC5DC]`,
/// the parasite release deck `[0x00AC497C]`, Anim `[0x0089A1B4]`
/// (initializer `0x00421E20`, the same `4 * level + 0.5` truncation, read by
/// the MakeInfantry mark and clear at `0x0042629B`/`0x00426328`), Rocket
/// `[0x00B04E6C]` (one of the initializers at `0x008147C4`, read by the
/// impact predictor `0x006620F0`; 416 when those initializers run under
/// Unicorn), and VoxelAnim `[0x00B1D1BC]` (initializer `0x00749310`, again
/// `4 * level + 0.5`, read by `VoxelAnimClass::DrawIt`'s shadow at
/// `0x00749C2D`; 416 when its initializers run in order under Unicorn). None
/// is INI-read and every captured value is 416, so one constant serves them
/// all, as [`LEPTONS_PER_LEVEL`] does for their level steps.
pub const BRIDGE_DECK_HEIGHT_LEPTONS: i32 = BRIDGE_DECK_HEIGHT_LEVELS * GROUND_LEVEL_HEIGHT_LEPTONS;

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

/// Significand of the pixel-height scale `[0xB0CDD8]`: the double its
/// initializers `0x006D1830`, `0x006D18C0` and `0x006D1BF0` leave (bits
/// `0x401BDFB59E463B4E`, about 6.968) is this times `2^-50`.
const PIXEL_HEIGHT_SCALE_SIGNIFICAND: i128 = 0x1B_DFB5_9E46_3B4E;

/// `0x006D2120`: a height in screen pixels as leptons,
/// `ftol((pixels - 0.5) * [0xB0CDD8])`. Computed exactly as
/// `(2 * pixels - 1) * significand * 2^-51` truncated toward zero, with
/// `__ftol @ 0x007C5F00`'s 64-bit result cut to its low dword. Every half SHP
/// height (`-16384..=16383`) equals native execution under the control words
/// `0x0E7F`, `0x027F` and `0x037F`, and sample counts across the whole
/// `i32` range match too (`tools/superweapon_oracle.json`
/// `storm_pixel_heights`). Native callers: `LightningStorm::CreateCloudBolt`
/// (`0x0053A176`) and the Walk and Mech Move_To bridge lifts (`0x0075AD52`,
/// `0x005B0105`), which ask for 60 pixels.
pub const fn native_pixel_height_leptons(pixels: i32) -> i32 {
    let product = (2 * pixels as i128 - 1) * PIXEL_HEIGHT_SCALE_SIGNIFICAND;
    (product / (1 << 51)) as i64 as i32
}

/// A spot's [`SUBCELL_OFFSETS`] entry in whole leptons. Unknown spots
/// default to the cell center.
pub fn subcell_offset(spot: u8) -> (i32, i32) {
    SUBCELL_OFFSETS
        .get(usize::from(spot))
        .copied()
        .unwrap_or(SUBCELL_OFFSETS[0])
}

/// Get the lepton sub-cell offset for a given sub-cell index (0–4).
///
/// Returns `(sub_x, sub_y)` in lepton units (0..256 range).
/// No sub-cell or an unknown index defaults to cell center.
pub fn subcell_lepton_offset(sub_cell: Option<u8>) -> (SimFixed, SimFixed) {
    let (x, y) = subcell_offset(sub_cell.unwrap_or(0));
    (SimFixed::from_num(x), SimFixed::from_num(y))
}

/// Project a signed absolute world X/Y coordinate using active YR's integer
/// operation order.
///
/// gamemd-derived: active YR `TacticalClass::CoordsToClient2` at `0x006D2140`
/// forms each 60/30-pixel numerator in signed integer arithmetic and divides
/// the complete result by 256, truncating toward zero.
pub fn project_absolute_lepton_xy(world_x_leptons: i32, world_y_leptons: i32) -> (i32, i32) {
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
pub fn absolute_leptons_to_screen(
    world_x_leptons: i32,
    world_y_leptons: i32,
    world_z_leptons: i32,
) -> (f32, f32) {
    let (screen_x, screen_y) = project_absolute_lepton_xy(world_x_leptons, world_y_leptons);
    let screen_y = screen_y + WORLD_ROW_BIAS_PX
        - crate::util::native_x87::adjust_for_z_standard(world_z_leptons);
    (screen_x as f32, screen_y as f32)
}

/// Compose a native signed CellStruct word and subcell offset before chopping
/// fractional leptons. This is the shared inverse for packed cell packets;
/// the original absolute coordinate remains authoritative when one is retained.
pub(crate) fn absolute_lepton_axis(cell: u16, sub_cell: SimFixed) -> i32 {
    let absolute_fixed =
        i64::from(cell as i16) * 256 * SIM_FIXED_SCALE + i64::from(sub_cell.to_bits());
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
/// [`absolute_leptons_to_screen`] is the absolute-lepton twin of this
/// function and produces the identical point; `matching_lepton_projections_agree`
/// pins that.
///
/// **Buildings are the one class that does not draw here.** The original gives
/// `BuildingClass` its own render-coordinate virtual which takes half a cell off
/// both axes, moving its anchor back onto the tile row of its north-west
/// footprint cell. That shift is owned by
/// `render::locomotor_visual::BUILDING_ART_LIFT_PX`, not by this function —
/// every other consumer here wants the plain projection.
pub fn lepton_to_screen_exact_z(
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::fixed_math::SIM_ZERO;

    #[test]
    fn lepton_to_cell_truncates_toward_zero_and_packs_to_16_bits() {
        for (leptons, cell) in [
            (256, 1),
            (-256, -1),
            (255, 0),
            (-1, 0),
            (-255, 0),
            (-257, -1),
            (384, 1),
            (-384, -1),
        ] {
            assert_eq!(lepton_to_cell(leptons), cell, "{leptons}");
        }
        assert_eq!(lepton_to_cell_packed(0x0100_0000), 0);
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
        for spot in 0..SUBCELL_OFFSETS.len() as u8 {
            let (sub_x, sub_y) = subcell_lepton_offset(Some(spot));
            let util = lepton_to_screen(10, 4, sub_x, sub_y, 2);
            let absolute = absolute_leptons_to_screen(
                absolute_lepton_axis(10, sub_x),
                absolute_lepton_axis(4, sub_y),
                2 * GROUND_LEVEL_HEIGHT_LEPTONS,
            );
            assert_eq!(util, absolute, "canonical subcell ({sub_x}, {sub_y})");
        }

        let negative = lepton_to_screen(0, 0, SimFixed::from_num(-9), SimFixed::from_num(5), 0);
        assert_eq!(negative, absolute_leptons_to_screen(-9, 5, 0));

        let crossing = lepton_to_screen(1, 0, SimFixed::lit("-0.5"), SIM_ZERO, 0);
        assert_eq!(crossing, (29.0, 29.0));
        assert_eq!(
            crossing,
            absolute_leptons_to_screen(255, 0, 0),
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
                    let (bx, by) = absolute_leptons_to_screen(
                        i32::from(rx) * 256 + sub_x,
                        i32::from(ry) * 256 + sub_y,
                        0,
                    );
                    assert!(
                        (ax - bx).abs() < 0.01 && (ay - by).abs() < 0.01,
                        "cell ({rx},{ry}) sub ({sub_x},{sub_y}): util {:?} vs absolute {:?}",
                        (ax, ay),
                        (bx, by),
                    );
                }
            }
        }
    }

    #[test]
    fn subcell_lookup_returns_correct_positions() {
        let lit = |x: &str, y: &str| (SimFixed::lit(x), SimFixed::lit(y));
        assert_eq!(subcell_lepton_offset(None), lit("128", "128"));
        assert_eq!(subcell_lepton_offset(Some(0)), lit("128", "128"));
        assert_eq!(subcell_lepton_offset(Some(1)), lit("64", "64"));
        assert_eq!(subcell_lepton_offset(Some(2)), lit("192", "64"));
        assert_eq!(subcell_lepton_offset(Some(3)), lit("64", "192"));
        assert_eq!(subcell_lepton_offset(Some(4)), lit("192", "192"));
        assert_eq!(subcell_lepton_offset(Some(5)), lit("128", "128"));
        assert_eq!(subcell_offset(3), (64, 192));
        assert_eq!(subcell_offset(255), (128, 128));
    }

    #[test]
    fn absolute_leptons_to_screen_sub_cell_offset_is_iso_subdivided() {
        // Sub-cell offset of (128, 0) — native truncates the complete Y
        // numerator from 7.5 to 7 before VERA's common +15 row bias.
        let (sx, sy) = absolute_leptons_to_screen(128, 0, 0);
        assert_eq!((sx, sy), (15.0, 22.0));
    }

    #[test]
    fn adjust_for_z_lepton_projection_matches_retail_fixtures_and_boundary() {
        let (_, baseline) = absolute_leptons_to_screen(0, 0, 0);
        for (z, expected_lift) in [
            (104, 15.0),
            (208, 30.0),
            (256, 37.0),
            (727, 104.0),
            (728, 105.0),
            (1_500, 216.0),
            (-400, -56.0),
        ] {
            let (_, projected) = absolute_leptons_to_screen(0, 0, z);
            assert_eq!(baseline - projected, expected_lift, "z={z}");
        }
    }
}
