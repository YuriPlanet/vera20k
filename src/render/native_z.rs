//! Native Z-buffer row arithmetic for the tactical depth pipelines.
//!
//! gamemd keeps one 16-bit Z per screen pixel (`g_ZBuffer @ 0x00887644`,
//! lower = nearer, `DefaultZ = 0x8000`). Every draw seeds a per-row Z from its
//! screen position and a class-supplied adjustment, walks rows with one of
//! three gradient entries, and tests (buildings and tiles also write) each
//! pixel. CPU helpers retain native row arithmetic; tactical shaders encode
//! native u16 stores losslessly as word/65535, independently of map bounds.
//! `depth_for_row` and `depth_for_native_z` still produce the legacy world/sort
//! scalar consumed by compatibility Batch paths. Their existing clamps and
//! fractional ordering are not an exact native u16 source. TREE reads a mixed
//! compatibility destination by rounding its value on the native axis.
//!
//! Evidence (read-only Ghidra, 2026-09-07, `docs/research/ZBUFFER_DEPTH_SYSTEM.md`
//! sections 1, 2, 4):
//! - `Standard_SHP_blitter @ 0x004373B0` (`0x004374BB..0x004375A7`): the two
//!   seeding paths below, then the per-row step applied after each row
//!   (`0x00437921..0x0043793f`).
//! - `Extended_SHP_blitter @ 0x00437A10`: ordinary gradients without a
//!   second shape; with one, constant raw bottom seed (no gradient), minus
//!   the per-pixel signed z-shape byte in the leaf (`0x004990E0`).
//! - `TMP_TileBlitter @ 0x00547CF0`: `DAT_00AA1104 = (u16)(DefaultZ + YOrigin
//!   - y - tileH) - tileH * heightLevel / 2`, per pixel `base + zdata <= zbuf`.
//! - `TechnoClass_DrawSHP @ 0x00705E00`: CC stack slot 7 = a7 - 2
//!   (`0x00706430`); units and infantry on the ground add
//!   `FootClass::GetZAdjustment @ 0x004DAFC0` (which starts from
//!   `-AdjustForZ(Location.Z)`), airborne ones subtract `AdjustForZ(Location.Z)`
//!   (`0x00705FF3..0x00706048`); aircraft likewise (`0x00705FB9`). Buildings
//!   pass `NormalZAdjust - AdjustForZ(Location.Z)` themselves
//!   (`BuildingClass_DrawBody 0x0043D836..0x0043D84D`).
//! - Gradient table `g_ZGradientTable @ 0x00817710`.

/// `g_ZBuffer + 0x24`: row seed constant; distinct from empty stored Z.
pub const DEFAULT_Z: i32 = 0x8000;

/// Lossless GPU storage of the native u16 Z-buffer word. The original leaves
/// compare their signed candidate first, then write its low16 bits. Hardware
/// comparisons alone only establish that behavior for in-range candidates.
/// See TERRAIN_STATIC_BODY_SHADOW_NATIVE_2026_09_09.md and native_z.wgsl.
pub const STORED_DEPTH_CLEAR: f32 = 1.0;

/// Original ctor 007BCA08 and active dirty clear 007BCFB0 store 0xffff;
/// 007BCA2C separately initializes the 0x8000 row seed. Empty pixels admit
/// candidates 32768..65534 too, while equality at 65535 still rejects.
pub const EMPTY_Z: u16 = u16::MAX;

#[cfg(test)]
pub fn stored_depth(z: u16) -> f32 {
    f32::from(z) / f32::from(u16::MAX)
}

#[cfg(test)]
pub fn stored_z(depth: f32) -> u16 {
    (depth * f32::from(u16::MAX)).round() as u16
}

/// `TechnoClass_DrawSHP` pushes `a7 - 2` as the CC_Draw_Shape Z term
/// (`0x00706430`, `0x00706505`, `0x007065D4`). Every SHP object draw carries it.
pub const SHP_DRAW_Z_ADJUST_PX: i32 = -2;

/// `BuildingClass_DrawBody @ 0x0043D9C9` pushes `-1 - AdjustForZ(Z)` as the
/// bib's a7; the bib body then carries `-1 - 2` on top of the lift cancel.
pub const BIB_Z_ADJUST_PX: i32 = -1;

/// Terrain height levels project 15 screen rows apart. Raw object Z instead
/// uses `native_x87::adjust_for_z_standard` (the active 104-lepton ground
/// scalar and native rounding/728 correction); 256 leptons is one XY cell,
/// not one native height level.
pub const HEIGHT_LEVEL_PX: i32 = crate::map::terrain::HEIGHT_STEP as i32;

/// `IsometricTileTypeClass` tile height (`piVar10[3]` at `0x00547D8B`), the
/// `tileH` term of the tile base Z. Retail RA2/YR templates are 60x30.
pub const TILE_HEIGHT_ROWS: i32 = crate::map::terrain::TILE_HEIGHT as i32;

/// One per-scanline Z-gradient entry (`g_ZGradientTable @ 0x00817710`, six
/// `i32` each). Only the fields the blitters read are modelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZGradientEntry {
    /// `field[1]`: quantisation divisor of the seeded Z on the field[5]=1 path.
    pub quantise: i32,
    /// `field[2]`: accumulator increment per row.
    pub increment: i32,
    /// `field[3]`: accumulator threshold; one Z step per crossing.
    pub threshold: i32,
    /// `field[4]`: signed Z step.
    pub step_dir: i32,
    /// `field[5]`: 1 = seed from the top row; 0 = seed from the bottom row
    /// (the building path that folds the sprite height in).
    pub top_seeded: bool,
}

/// Which gradient entry a draw walks with. The discriminant is the native
/// table index and the value stored in `SpriteInstance::z_gradient`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ZGradient {
    /// Entry 0 `{1,1,1,1,-1,1}`: Z decreases one unit per row (ground-flat art).
    Flat = 0,
    /// Entry 1 `{2,3,2,3,-1,1}`: Z decreases two units every three rows.
    Moderate = 1,
    /// Entry 2 `{1,3,1,3,+1,0}`: seeded at the bottom row, Z increases one
    /// unit every three rows going down. Buildings, units, infantry, anims.
    Vertical = 2,
}

/// Bytes at `0x00817710`, re-read 2026-09-07 (critic confirmation).
pub const GRADIENT_TABLE: [ZGradientEntry; 3] = [
    ZGradientEntry {
        quantise: 1,
        increment: 1,
        threshold: 1,
        step_dir: -1,
        top_seeded: true,
    },
    ZGradientEntry {
        quantise: 3,
        increment: 2,
        threshold: 3,
        step_dir: -1,
        top_seeded: true,
    },
    ZGradientEntry {
        quantise: 3,
        increment: 1,
        threshold: 3,
        step_dir: 1,
        top_seeded: false,
    },
];

impl ZGradient {
    pub const fn entry(self) -> ZGradientEntry {
        GRADIENT_TABLE[self as usize]
    }

    pub const fn from_index(index: u32) -> Self {
        match index & 0xFF {
            1 => Self::Moderate,
            2 => Self::Vertical,
            _ => Self::Flat,
        }
    }
}

/// Bit in `SpriteInstance::z_gradient` that enables the BUILDNGZ z-shape
/// lookup at `zshape_origin`.
pub const Z_GRADIENT_ZSHAPE_FLAG: u32 = 0x100;

/// Clip against the second shape even for raw frames. CC_Draw_Shape clips
/// before its format-bit dispatch; only extended frames consume shape values.
pub const Z_GRADIENT_ZSHAPE_CLIP_FLAG: u32 = 0x200;

/// Unit final-composite split (`0x0073B140`, Unit vtable +0x55C).
/// Only the voxel sprite shader consumes this bit. All body parts retain
/// one native composite rectangle; its last 16 rows use gradient 2/FootZ,
/// while preceding rows use gradient 0/FootZ-5. Shadows do not carry it.
pub const Z_GRADIENT_VOXEL_BRIDGE_SPLIT_FLAG: u32 = 0x400;

pub const fn pack_voxel_z_gradient(gradient: ZGradient, bridge_split: bool) -> u32 {
    gradient as u32
        | if bridge_split {
            Z_GRADIENT_VOXEL_BRIDGE_SPLIT_FLAG
        } else {
            0
        }
}

/// One independently clipped native unit-composite blit rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoxelDepthRegion {
    pub top: i32,
    pub height: i32,
    pub gradient: ZGradient,
    pub z_adjust: i32,
}

/// CPU description of the voxel shader's per-fragment region selection.
/// `rect` is [world top, height], `clip` is [world top, exclusive bottom].
/// Eligibility uses the original height; clipping happens afterwards in
/// each Standard_SHP_blitter call (0x0073B2FE, 0x00437461). This does not
/// choose the gameplay/type predicate or move the parent's painter position.
#[cfg(test)]
pub fn voxel_depth_region(
    rect: [i32; 2],
    packed_gradient: u32,
    z_adjust: i32,
    world_row: i32,
    clip: [i32; 2],
) -> Option<VoxelDepthRegion> {
    let [mut top, mut height] = rect;
    let mut gradient = ZGradient::from_index(packed_gradient);
    let mut z_adjust = z_adjust;
    if packed_gradient & Z_GRADIENT_VOXEL_BRIDGE_SPLIT_FLAG != 0 {
        if height > 16 {
            let boundary = top + height - 16;
            if world_row < boundary {
                height -= 16;
                gradient = ZGradient::Flat;
                z_adjust -= 5;
            } else {
                top = boundary;
                height = 16;
                gradient = ZGradient::Vertical;
            }
        }
        let bottom = (top + height).min(clip[1]);
        top = top.max(clip[0]);
        height = bottom - top;
    }
    (height > 0 && world_row >= top && world_row < top + height).then_some(VoxelDepthRegion {
        top,
        height,
        gradient,
        z_adjust,
    })
}

pub const fn pack_building_z_gradient(zshape: bool, extended: bool) -> u32 {
    pack_z_gradient(ZGradient::Vertical, zshape && extended)
        | if zshape {
            Z_GRADIENT_ZSHAPE_CLIP_FLAG
        } else {
            0
        }
}

/// Pack a gradient entry and the z-shape enable into `SpriteInstance::z_gradient`.
pub const fn pack_z_gradient(gradient: ZGradient, zshape: bool) -> u32 {
    (gradient as u32) | if zshape { Z_GRADIENT_ZSHAPE_FLAG } else { 0 }
}

/// Seeded Z and initial accumulator for the first row of an SHP/VXL blit.
///
/// `screen_top` is the blit rect's top row in Z-buffer coordinates
/// (`YOrigin` taken as 0, matching a `-win` retail window whose tactical
/// area starts at the top of the Z surface), `height` the rect height and
/// `z_adjust` the CC_Draw_Shape Z term.
pub fn sprite_seed_z(
    gradient: ZGradient,
    screen_top: i32,
    height: i32,
    z_adjust: i32,
) -> (i32, i32) {
    let entry = gradient.entry();
    if entry.top_seeded {
        // `0x00437566..0x00437585`: (u16)(DefaultZ + YOrigin - y) + z, quantised.
        let raw = ((DEFAULT_Z - screen_top) & 0xFFFF) + z_adjust;
        ((raw / entry.quantise) * entry.quantise, 0)
    } else {
        // `0x004374FA..0x00437558`: bottom-row seed folding the height in.
        let step = entry.threshold / entry.increment;
        let raw = ((DEFAULT_Z - height - screen_top + 1) & 0xFFFF) + z_adjust;
        let mut z = (raw / step) * step - height / step;
        let mut accum = entry.threshold - height % step;
        if accum == entry.threshold {
            accum = 0;
            z += entry.step_dir;
        }
        (z, accum)
    }
}

/// Z of row `row` (0 = top) of a blit seeded by [`sprite_seed_z`]. The walker
/// steps after each row (`0x00437921`), so row `k` has absorbed `k` increments.
#[cfg(test)]
pub fn sprite_row_z(
    gradient: ZGradient,
    screen_top: i32,
    height: i32,
    z_adjust: i32,
    row: i32,
) -> i32 {
    let entry = gradient.entry();
    let (seed, accum) = sprite_seed_z(gradient, screen_top, height, z_adjust);
    let steps = (accum + row.max(0) * entry.increment) / entry.threshold;
    seed + entry.step_dir * steps
}

/// Constant seed of an extended SHP blit with a second shape (BUILDNGZ).
/// `Extended_SHP_blitter @ 0x00437C39..0x00437C72` bypasses gradient
/// quantisation; `0x00437E67..0x00437EA7` advances shape rows without stepping Z.
/// The per-pixel leaf then subtracts the signed shape byte. Raw SHP frames
/// use the standard walker and do not consume the second shape.
#[cfg(test)]
pub fn zshape_seed_z(screen_top: i32, height: i32, z_adjust: i32) -> i32 {
    ((DEFAULT_Z - height - screen_top + 1) & 0xFFFF) + z_adjust
}

/// Tile base Z (`DAT_00AA1104`). `screen_top` is the diamond top row on
/// screen (already lifted by the height level), `height_level` the cell's
/// `+0x11B` level. Per pixel the tile adds its TMP Z-data byte and draws when
/// `base + zdata <= zbuf`.
#[cfg(test)]
pub fn tile_base_z(screen_top: i32, height_level: i32) -> i32 {
    ((DEFAULT_Z - screen_top - TILE_HEIGHT_ROWS) & 0xFFFF) - (TILE_HEIGHT_ROWS * height_level) / 2
}

/// The Z term a draw supplies relative to its *drawn* (lifted) rows so that the
/// result anchors on the ground-projected row: every native class folds
/// `-AdjustForZ(Location.Z)` into its adjustment, cancelling the screen lift.
/// `class_term` is the remaining class constant (`SHP_DRAW_Z_ADJUST_PX`,
/// `NormalZAdjust`, bib `-1`, anim `ZAdjust`, ...).
pub fn ground_anchored_z_adjust(height_level: i32, class_term: i32) -> i32 {
    class_term - HEIGHT_LEVEL_PX * height_level
}

/// BUILDNGZ.SHA canvas point that lands on a building's draw point before the
/// foundation shift (`BuildingClass_DrawBody 0x0043D6EF..0x0043D77A`).
pub const ZSHAPE_ANCHOR: (i32, i32) = (0xC6, 0x1BE);

/// `BuildingClass_DrawBody @ 0x0043D787`: a foundation width of 8 or more
/// draws without a z-shape (`CMP EAX, 8 / JL`).
pub const ZSHAPE_MAX_FOUNDATION_WIDTH: u16 = 7;

/// World-pixel origin of the z-shape canvas for one building.
///
/// The body pushes `(0xC6, 0x1BE) + ZShapePointMove - CellToPixel((W-1)*256,
/// (H-1)*256)` as the z-shape offset; the extended blitter reads z-shape pixel
/// `p` for screen pixel `draw_point - offset + p`. `draw_point` is the point
/// `CC_Draw_Shape` centres the body on (VERA's building anchor), in world
/// pixels.
pub fn zshape_origin(
    draw_point: (i32, i32),
    foundation: (u16, u16),
    point_move: (i32, i32),
) -> (i32, i32) {
    let w = i32::from(foundation.0.max(1)) - 1;
    let h = i32::from(foundation.1.max(1)) - 1;
    // `TacticalClass::CellToPixel` of whole-cell leptons: x = (dx - dy) * 30,
    // y = (dx + dy) * 15 (60x30 cells, 256 leptons per cell).
    let far_corner = (
        (w - h) * (crate::map::terrain::TILE_WIDTH as i32 / 2),
        (w + h) * HEIGHT_LEVEL_PX,
    );
    let offset = (
        ZSHAPE_ANCHOR.0 + point_move.0 - far_corner.0,
        ZSHAPE_ANCHOR.1 + point_move.1 - far_corner.1,
    );
    (draw_point.0 - offset.0, draw_point.1 - offset.1)
}

/// `FUN_0045E8F0`: every non-zero BUILDNGZ byte is remapped by `-0x41` and
/// later read as a signed char.
pub fn zshape_remap(byte: u8) -> i8 {
    if byte == 0 {
        0
    } else {
        byte.wrapping_sub(0x41) as i8
    }
}

/// Bias applied when the remapped z-shape is stored in an `R8Unorm` texture,
/// so the shader recovers `i32(texel * 255 + 0.5) - ZSHAPE_TEXEL_BIAS`.
pub const ZSHAPE_TEXEL_BIAS: i32 = 128;

/// Normalised depth for a native ground row on VERA's depth axis.
pub fn depth_for_row(row: f32, origin_y: f32, world_height: f32) -> f32 {
    (1.0 - (row - origin_y) / world_height.max(1.0)).clamp(0.001, 0.999)
}

/// Normalised depth of an absolute native Z at camera row `camera_y`.
#[cfg(test)]
pub fn depth_for_native_z(z: i32, camera_y: i32, origin_y: f32, world_height: f32) -> f32 {
    depth_for_row((DEFAULT_Z - z + camera_y) as f32, origin_y, world_height)
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_stored_depth_preserves_every_word_and_distinguishes_empty_from_seed() {
        assert_eq!(stored_z(STORED_DEPTH_CLEAR), EMPTY_Z);
        assert_ne!(i32::from(EMPTY_Z), DEFAULT_Z);
        for z in 0..=u16::MAX {
            assert_eq!(stored_z(stored_depth(z)), z);
        }
    }

    #[test]
    fn original_extended_row_walk_matches_all_clipped_fixture_rows() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/terrain_draw_oracle/fixtures/rows.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 406);
        let mut checked = 0;
        for c in cases {
            let value = |index: usize| c[index].as_i64().unwrap() as i32;
            let gradient = ZGradient::from_index(value(0) as u32);
            for (offset, expected) in c[6].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    sprite_row_z(
                        gradient,
                        value(1),
                        value(2),
                        value(3),
                        value(4) + offset as i32
                    ) as u32,
                    expected.as_u64().unwrap() as u32,
                    "case={c}, offset={offset}"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 13854);
    }

    use super::*;

    #[test]
    fn voxel_split_selects_original_boundary_then_clips_each_seed_rectangle() {
        let flag = pack_voxel_z_gradient(ZGradient::Moderate, true);
        let upper = voxel_depth_region([-4, 32], flag, 7, 6, [0, 24]).unwrap();
        assert_eq!(
            upper,
            VoxelDepthRegion {
                top: 0,
                height: 12,
                gradient: ZGradient::Flat,
                z_adjust: 2
            }
        );
        let lower = voxel_depth_region([-4, 32], flag, 7, 12, [0, 24]).unwrap();
        assert_eq!(
            lower,
            VoxelDepthRegion {
                top: 12,
                height: 12,
                gradient: ZGradient::Vertical,
                z_adjust: 7
            }
        );
        assert!(voxel_depth_region([-4, 32], flag, 7, 24, [0, 24]).is_none());
        // The branch threshold uses the original 16/17 heights, not the
        // remaining visible height after clipping the first row away.
        assert_eq!(
            voxel_depth_region([-1, 16], flag, 7, 0, [0, 24])
                .unwrap()
                .gradient,
            ZGradient::Moderate
        );
        assert_eq!(
            voxel_depth_region([-1, 17], flag, 7, 0, [0, 24])
                .unwrap()
                .gradient,
            ZGradient::Vertical
        );
        let ordinary = voxel_depth_region(
            [-4, 32],
            pack_voxel_z_gradient(ZGradient::Moderate, false),
            7,
            6,
            [0, 24],
        )
        .unwrap();
        assert_eq!(ordinary.top, -4);
        assert_eq!(ordinary.height, 32);
        assert_eq!(ordinary.gradient, ZGradient::Moderate);
        assert_eq!(ordinary.z_adjust, 7);
    }

    #[test]
    fn gradient_table_matches_the_bytes_at_0x00817710() {
        let t = GRADIENT_TABLE;
        assert_eq!(
            (
                t[0].increment,
                t[0].threshold,
                t[0].step_dir,
                t[0].top_seeded
            ),
            (1, 1, -1, true)
        );
        assert_eq!(
            (
                t[1].increment,
                t[1].threshold,
                t[1].step_dir,
                t[1].top_seeded
            ),
            (2, 3, -1, true)
        );
        assert_eq!(
            (
                t[2].increment,
                t[2].threshold,
                t[2].step_dir,
                t[2].top_seeded
            ),
            (1, 3, 1, false)
        );
    }

    #[test]
    fn flat_gradient_walks_one_unit_nearer_per_row_from_the_top() {
        // Top row Z is the ground Z of that screen row; each row down is nearer.
        let top = DEFAULT_Z - 100;
        assert_eq!(sprite_row_z(ZGradient::Flat, 100, 20, 0, 0), top);
        assert_eq!(sprite_row_z(ZGradient::Flat, 100, 20, 0, 7), top - 7);
        assert_eq!(sprite_row_z(ZGradient::Flat, 100, 20, -2, 0), top - 2);
    }

    #[test]
    fn moderate_gradient_quantises_the_seed_and_steps_two_per_three_rows() {
        let seed = ((DEFAULT_Z - 101) / 3) * 3;
        assert_eq!(sprite_row_z(ZGradient::Moderate, 101, 30, 0, 0), seed);
        assert_eq!(sprite_row_z(ZGradient::Moderate, 101, 30, 0, 1), seed);
        assert_eq!(sprite_row_z(ZGradient::Moderate, 101, 30, 0, 2), seed - 1);
        assert_eq!(sprite_row_z(ZGradient::Moderate, 101, 30, 0, 3), seed - 2);
        assert_eq!(sprite_row_z(ZGradient::Moderate, 101, 30, 0, 6), seed - 4);
    }

    #[test]
    fn vertical_gradient_anchors_on_the_bottom_row_and_climbs_nearer_upward() {
        // Height 30 (divisible by 3): accum starts at 3 -> reset to 0 and seed +1.
        let raw = DEFAULT_Z - 30 - 100 + 1 - 2;
        let seed = (raw / 3) * 3 - 10 + 1;
        assert_eq!(sprite_seed_z(ZGradient::Vertical, 100, 30, -2), (seed, 0));
        assert_eq!(sprite_row_z(ZGradient::Vertical, 100, 30, -2, 0), seed);
        assert_eq!(sprite_row_z(ZGradient::Vertical, 100, 30, -2, 2), seed);
        assert_eq!(sprite_row_z(ZGradient::Vertical, 100, 30, -2, 3), seed + 1);
        assert_eq!(sprite_row_z(ZGradient::Vertical, 100, 30, -2, 29), seed + 9);
        // The bottom row sits within one quantum of the ground Z of that row.
        let bottom_ground = DEFAULT_Z - (100 + 29);
        assert!((sprite_row_z(ZGradient::Vertical, 100, 30, 0, 29) - bottom_ground).abs() <= 3);
    }

    #[test]
    fn vertical_gradient_height_not_divisible_by_three_keeps_the_partial_accumulator() {
        // Height 31: accum = 3 - 1 = 2, so the first step lands after one row.
        let raw = DEFAULT_Z - 31 - 100 + 1;
        let seed = (raw / 3) * 3 - 10;
        assert_eq!(sprite_seed_z(ZGradient::Vertical, 100, 31, 0), (seed, 2));
        assert_eq!(sprite_row_z(ZGradient::Vertical, 100, 31, 0, 0), seed);
        assert_eq!(sprite_row_z(ZGradient::Vertical, 100, 31, 0, 1), seed + 1);
        assert_eq!(sprite_row_z(ZGradient::Vertical, 100, 31, 0, 3), seed + 1);
        assert_eq!(sprite_row_z(ZGradient::Vertical, 100, 31, 0, 4), seed + 2);
    }

    #[test]
    fn building_shape_uses_unquantised_bottom_seed_and_raw_frames_only_clip() {
        // Regression derived from the 437C39..437C72 arithmetic, not a
        // captured parity golden. A neutral shape must have one Z at all rows.
        assert_eq!(zshape_seed_z(100, 30, -2), DEFAULT_Z - 131);
        assert_ne!(
            zshape_seed_z(100, 30, -2),
            sprite_row_z(ZGradient::Vertical, 100, 30, -2, 0),
        );
        assert_eq!(zshape_seed_z(85, 30, -17), zshape_seed_z(100, 30, -2));
        assert_eq!(pack_building_z_gradient(true, true), 0x302);
        assert_eq!(pack_building_z_gradient(true, false), 0x202);
        assert_eq!(pack_building_z_gradient(false, true), 2);
    }

    #[test]
    fn tile_base_z_projects_an_elevated_tile_onto_its_ground_row() {
        // A level-2 tile is drawn 30 rows higher; its base Z is the ground
        // tile's base plus 0.5 per level (30*2/2 = 30 back, 30 up = wash).
        let ground = tile_base_z(400, 0);
        let lifted = tile_base_z(400 - 30, 2);
        assert_eq!(ground, DEFAULT_Z - 400 - 30);
        assert_eq!(lifted, ground);
        // Odd levels truncate the half unit toward zero.
        assert_eq!(tile_base_z(400 - 15, 1), DEFAULT_Z - 385 - 30 - 15);
    }

    #[test]
    fn flat_tile_zdata_makes_lower_rows_nearer() {
        // Retail CLEAR01.TEM Z-data runs 29 at the top row to 1..3 at the
        // bottom, so `base + zdata` decreases down the tile: nearer.
        let base = tile_base_z(200, 0);
        assert!(base + 29 > base + 1);
    }

    #[test]
    fn ground_anchored_adjust_cancels_the_height_lift() {
        assert_eq!(ground_anchored_z_adjust(0, SHP_DRAW_Z_ADJUST_PX), -2);
        assert_eq!(ground_anchored_z_adjust(3, SHP_DRAW_Z_ADJUST_PX), -2 - 45);
        // Drawn 45 rows higher and adjusted by -45: the same Z as on the ground.
        let ground = sprite_row_z(ZGradient::Vertical, 300, 30, -2, 29);
        let lifted = sprite_row_z(ZGradient::Vertical, 300 - 45, 30, -2 - 45, 29);
        assert_eq!(ground, lifted);
    }

    #[test]
    fn zshape_origin_shifts_down_and_across_with_the_foundation() {
        // 1x1: the canvas anchor lands on the draw point.
        assert_eq!(
            zshape_origin((1000, 500), (1, 1), (0, 0)),
            (1000 - 0xC6, 500 - 0x1BE)
        );
        // 3x3: far corner (2,2) cells -> (0, 60) px, so the canvas drops 60 rows.
        assert_eq!(
            zshape_origin((1000, 500), (3, 3), (0, 0)),
            (1000 - 0xC6, 500 - 0x1BE + 60)
        );
        // 4x2 (GAWEAP-like) with ZShapePointMove=30,15.
        let (x, y) = zshape_origin((1000, 500), (4, 2), (30, 15));
        assert_eq!((x, y), (1000 - (0xC6 + 30 - 60), 500 - (0x1BE + 15 - 60)));
    }

    #[test]
    fn zshape_remap_is_signed_minus_0x41_with_zero_kept() {
        assert_eq!(zshape_remap(0), 0);
        assert_eq!(zshape_remap(0x41), 0);
        assert_eq!(zshape_remap(0x40), -1);
        assert_eq!(zshape_remap(0x01), -64);
        assert_eq!(zshape_remap(0xC0), 127);
        assert_eq!(zshape_remap(0xC1), -128);
    }

    #[test]
    fn depth_axis_is_one_world_row_per_native_z_unit() {
        let wh: f32 = 3000.0;
        let a = depth_for_native_z(DEFAULT_Z - 500, 0, 0.0, wh);
        let b = depth_for_native_z(DEFAULT_Z - 501, 0, 0.0, wh);
        // Lower Z (nearer) is the smaller depth, by exactly one row.
        assert!(b < a);
        assert!((a - b - 1.0 / wh).abs() < 1e-6);
        // Scrolling the camera does not change the depth of a fixed world row.
        let fixed_row_z = |cam: i32| DEFAULT_Z - (700 - cam);
        assert_eq!(
            depth_for_native_z(fixed_row_z(0), 0, 0.0, wh),
            depth_for_native_z(fixed_row_z(120), 120, 0.0, wh)
        );
    }
}
