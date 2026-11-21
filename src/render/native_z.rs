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
//! Evidence (gamemd instructions):
//! - `Standard_SHP_blitter @ 0x004373B0` (`0x004374BB..0x004375A7`): the two
//!   seeding paths below, then the per-row step applied after each row
//!   (`0x00437921..0x0043793f`).
//! - `Extended_SHP_blitter @ 0x00437A10`: ordinary gradients without a
//!   second shape; with one, a constant seed minus the per-pixel signed
//!   z-shape byte. Top-seeded gradients keep their quantisation; only the
//!   bottom-seeded building gradient bypasses it (`0x00437BAB..0x00437C72`).
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

/// `g_ZBuffer + 0x24`: the row seed a new Z buffer starts from
/// (`0x007BCA2C`); distinct from empty stored Z. [`ZSeed`] moves it as the
/// view scrolls.
pub const DEFAULT_Z: i32 = 0x8000;

/// gamemd's row seed from frame to frame. The Z buffer keeps its pixels while
/// the view scrolls, so the seed scrolls with them and a world row keeps its
/// Z. The quantised walks (`0x00437527..0x00437585`, `0x00437BBA`,
/// `0x00437C45`: Moderate and Vertical step 3) round from the seed, so a
/// sprite's occlusion line on a cliff or ramp holds still while the view
/// scrolls, where a seed fixed to the screen moves it by up to 2 px.
///
/// - `TacticalClass_Draw @ 0x006D3D10` takes the view's move since its last
///   draw (`0x006D3D55..0x006D3D80`). A move as wide or as tall as the view
///   (`0x006D3D8A..0x006D3DAE`), or a complete redraw asked for since
///   (`GScreenClass::Flag_To_Redraw(1) @ 0x004F42F0`, read at `0x006D3DBB`),
///   zeroes it (`0x006D3DC6`).
/// - `ZBufferClass::Pan @ 0x007BCB50` (`0x006D401B`) takes what is left: the
///   seed falls by the rows the view moved down (`0x007BCD14`).
/// - A radar click puts it back to [`DEFAULT_Z`] (`0x00653EBA`), and so does
///   the new Z buffer a change of the view's size makes
///   (`Set_View_Dimensions`, `0x004A8A48`).
///
/// RESIDUAL: of the complete redraws, the camera jumps' are modelled
/// ([`ZSeed::complete_redraw`]). The others, such as a lighting change
/// (`0x0053ADE7`) or a lightning storm (`0x0053A711`), keep the seed only if
/// they fall on a frame the view scrolls; VERA pans it, which moves the
/// quantised phase by that frame's rows mod 3.
#[derive(Debug, Default)]
pub struct ZSeed {
    /// The view the last frame drew: left, top, width and height in world
    /// pixels.
    view: Option<[i32; 4]>,
    /// Rows the seed has panned: it is `DEFAULT_Z - pan`.
    pan: i32,
    /// A complete redraw is due, so the next frame does not pan.
    redraw: bool,
}

impl ZSeed {
    /// The rows panned for a frame drawing `view` (left, top, width and height
    /// in world pixels; the top is the Z buffer's row 0).
    pub fn frame(&mut self, view: [i32; 4]) -> i32 {
        let [left, top, width, height] = view;
        let redraw = std::mem::take(&mut self.redraw);
        match self.view {
            Some([last_left, last_top, last_width, last_height])
                if [last_width, last_height] == [width, height] =>
            {
                let [dx, dy] = [left.wrapping_sub(last_left), top.wrapping_sub(last_top)];
                if !redraw && dx.wrapping_abs() < width && dy.wrapping_abs() < height {
                    self.pan = self.pan.wrapping_add(dy);
                }
            }
            Some(_) => self.pan = 0,
            None => {}
        }
        self.view = Some(view);
        self.pan
    }

    /// Rows the seed has panned as of the last frame.
    pub fn pan(&self) -> i32 {
        self.pan
    }

    /// The seed as of the last frame.
    pub fn seed(&self) -> i32 {
        DEFAULT_Z.wrapping_sub(self.pan)
    }

    /// `GScreenClass::Flag_To_Redraw(1)`: the next frame redraws the whole
    /// view and does not pan.
    pub fn complete_redraw(&mut self) {
        self.redraw = true;
    }

    /// A radar click: a complete redraw from [`DEFAULT_Z`]
    /// (`0x00653EA5..0x00653EBA`).
    pub fn reset(&mut self) {
        self.pan = 0;
        self.redraw = true;
    }
}

/// Lossless GPU storage of the native u16 Z-buffer word. The original leaves
/// compare their signed candidate first, then write its low16 bits. Hardware
/// comparisons alone only establish that behavior for in-range candidates.
/// See native_z.wgsl.
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
/// It rides with [`Z_GRADIENT_VOXEL_VIEW_CLIP_FLAG`]. All body parts retain
/// one native composite rectangle; its last 16 rows use gradient 2/FootZ,
/// while preceding rows use gradient 0/FootZ-5. Shadows do not carry it.
pub const Z_GRADIENT_VOXEL_BRIDGE_SPLIT_FLAG: u32 = 0x400;

/// A voxel body's final composite reaches the screen through Bit_Blit
/// (`0x0073B140` and TechnoClass::Render `0x00706ED0`, both via `0x004AF2A0`),
/// which clips its destination rectangle to the tactical view before seeding
/// (`0x00437461`, then `0x004374BB..0x004375A7` read the clipped rectangle).
/// A bottom-seeded walk then depends on the clipped bottom row. A turreted
/// SHP Unit's body and turret reach the screen through the same composite
/// (`0x0073C7AC..0x0073CDF9`), so the SHP shader takes this bit too, but
/// only without [`Z_GRADIENT_LINEAR_SHAPE_FLAG`], whose channel bits share
/// it ([`unit_composite`]). A frame drawn straight to the screen carries
/// [`Z_GRADIENT_RAW_FRAME_FLAG`] instead.
pub const Z_GRADIENT_VOXEL_VIEW_CLIP_FLAG: u32 = 0x1000;

/// Whether a packed gradient is a Unit final composite's
/// ([`pack_voxel_z_gradient`]), whose blit `fx_params.w` clips to the view.
pub const fn unit_composite(packed_gradient: u32) -> bool {
    packed_gradient & (Z_GRADIENT_LINEAR_SHAPE_FLAG | Z_GRADIENT_VOXEL_VIEW_CLIP_FLAG)
        == Z_GRADIENT_VOXEL_VIEW_CLIP_FLAG
}

/// A raw SHP frame (format bit 0x02 clear). CC_Draw_Shape hands it to
/// Bit_Blit (`0x004AF126 -> 0x004373B0`), which clips the rectangle to the
/// view (`0x00437461`) and seeds either gradient kind from the clipped one
/// (`0x0043751A`, `0x0043754A`). The RLE walker seeds a top-seeded entry
/// from the clipped top too (`0x00437BAB`), but entry 2 from the whole
/// rectangle (`0x00437C39`): [`shp_seed_rows`]. Only a gradient-2 frame
/// needs the flag; `fx_params.w` holds the view's height
/// (`render::draw_state::DrawState`).
pub const Z_GRADIENT_RAW_FRAME_FLAG: u32 = 0x4000;

/// The flag an SHP frame's format adds to its gradient word; `extended` is
/// format bit 0x02.
pub const fn shp_frame_flag(extended: bool) -> u32 {
    if extended {
        0
    } else {
        Z_GRADIENT_RAW_FRAME_FLAG
    }
}

/// The world rows `[top, bottom)` an SHP blit over rows `rows` seeds its Z
/// walk from, in a view over rows `view` (`CameraUniform::view_rows`): a
/// top-seeded entry from the top clipped to the view, a raw frame from both
/// ends clipped ([`Z_GRADIENT_RAW_FRAME_FLAG`]), the RLE walker's entry 2
/// from the whole rectangle. Mirrors native_z.wgsl `shp_seed_rows`.
pub fn shp_seed_rows(rows: [i32; 2], packed_gradient: u32, view: [i32; 2]) -> [i32; 2] {
    let raw = packed_gradient & Z_GRADIENT_RAW_FRAME_FLAG != 0;
    let mut rows = rows;
    if raw || ZGradient::from_index(packed_gradient).entry().top_seeded {
        rows[0] = rows[0].max(view[0]);
    }
    if raw {
        rows[1] = rows[1].min(view[1]);
    }
    rows
}

/// Encoded overlay depth file: geometry was clipped on the CPU and the
/// optional surface reads signed raw bytes by linear address. Bits12..13
/// select one of four RGBA channels; high16 stores the native row stride.
pub const Z_GRADIENT_LINEAR_SHAPE_FLAG: u32 = 0x800;

pub fn pack_overlay_z_gradient(gradient: ZGradient, slope: u8, stride: u16) -> u32 {
    debug_assert!((1..=4).contains(&slope));
    pack_z_gradient(gradient, true)
        | Z_GRADIENT_LINEAR_SHAPE_FLAG
        | (u32::from(slope - 1) << 12)
        | (u32::from(stride) << 16)
}

/// A Unit body's final composite (`0x0073B140`), voxel or turreted SHP:
/// clipped to the view, and split when `bridge_split`.
pub const fn pack_voxel_z_gradient(gradient: ZGradient, bridge_split: bool) -> u32 {
    gradient as u32
        | Z_GRADIENT_VOXEL_VIEW_CLIP_FLAG
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
/// each Standard_SHP_blitter call (0x0073B2FE, 0x00437461), split or not.
/// This does not choose the gameplay/type predicate or move the parent's
/// painter position.
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
    }
    if packed_gradient & (Z_GRADIENT_VOXEL_BRIDGE_SPLIT_FLAG | Z_GRADIENT_VOXEL_VIEW_CLIP_FLAG) != 0
    {
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

/// Whether an SHP blit over `rect` ([world top, height]) can reach a negative
/// signed candidate, whatever rectangle a z-shape or view clip cuts it to and
/// whatever its z-shape bytes. Z rows count from `z_origin_row`. Conservative:
/// each row's candidate is at least its own row's unstepped seed
/// `(DEFAULT_Z - screen_row) & 0xFFFF` plus `z_adjust`, less the rect's height
/// in steps, three of quantisation and 127 of z-shape; a seed that wraps
/// inside the rect puts small seeds beside large ones.
pub(crate) fn shp_candidate_may_be_negative(
    rect: [i32; 2],
    z_adjust: i32,
    z_origin_row: i32,
) -> bool {
    let [top, height] = rect.map(i64::from);
    // The bottom-seeded entry adds one (`sprite_seed_z`).
    let first = i64::from(DEFAULT_Z) - (top - i64::from(z_origin_row)) + 1;
    let last = first - height - 1;
    first.div_euclid(0x1_0000) != last.div_euclid(0x1_0000)
        || last.rem_euclid(0x1_0000) + i64::from(z_adjust) - height - 3 - 127 < 0
}

/// Conservative signed candidate envelope for a posed voxel body. Within
/// each native region the existing row kernel is monotone, so its two ends
/// suffice. Split and clip boundaries may seed independent regions.
/// None means that no valid bounded range was established, not safe hardware Z.
/// Z rows count from `z_origin_row` (`CameraUniform::native_z_origin_row`).
pub(crate) fn voxel_depth_range(
    rect: [i32; 2],
    packed_gradient: u32,
    z_adjust: i32,
    z_origin_row: i32,
    clip: [i32; 2],
) -> Option<[i32; 2]> {
    let [top, height] = rect;
    if height <= 0 {
        return None;
    }
    let bottom = top.checked_add(height)?;
    // Moderate gradient row accumulation multiplies by two.
    height.checked_mul(2)?;
    // Guard malformed/overflowing presentation inputs before the i32 kernel.
    z_adjust.checked_add(i32::from(u16::MAX))?;
    z_adjust.checked_sub(5)?.checked_sub(height)?;
    let split = bottom.checked_sub(16)?;
    let probes = [
        top,
        bottom - 1,
        split.checked_sub(1)?,
        split,
        clip[0],
        clip[1].checked_sub(1)?,
    ];
    let count = if packed_gradient & Z_GRADIENT_VOXEL_BRIDGE_SPLIT_FLAG != 0 {
        probes.len()
    } else if packed_gradient & Z_GRADIENT_VOXEL_VIEW_CLIP_FLAG != 0 {
        // One region; the clip may move its top and bottom.
        probes.len()
    } else {
        1
    };
    let mut range: Option<[i32; 2]> = None;
    for &world_row in &probes[..count] {
        let Some(region) = voxel_depth_region(rect, packed_gradient, z_adjust, world_row, clip)
        else {
            continue;
        };
        let screen_top = region.top.checked_sub(z_origin_row)?;
        // sprite_seed_z computes DEFAULT_Z-height-screen_top before its u16 cast.
        DEFAULT_Z
            .checked_sub(region.height)?
            .checked_sub(screen_top)?
            .checked_add(1)?;
        for row in [0, region.height - 1] {
            let value = sprite_row_z(
                region.gradient,
                screen_top,
                region.height,
                region.z_adjust,
                row,
            );
            range = Some(match range {
                Some([min, max]) => [min.min(value), max.max(value)],
                None => [value, value],
            });
        }
    }
    range
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

/// Constant seed of an extended SHP blit with a second shape.
/// `437BAB..437BD9` keeps top-gradient quantisation; `437C39..437C72`
/// bypasses it for the bottom gradient. `437E67..437EA7` advances shape rows
/// without stepping Z. Consumed original controls: ore_shape_clip.json.
/// The per-pixel leaf then subtracts the signed shape byte. Raw SHP frames
/// use the standard walker and do not consume the second shape.
#[cfg(test)]
pub fn zshape_seed_z(gradient: ZGradient, screen_top: i32, height: i32, z_adjust: i32) -> i32 {
    if gradient.entry().top_seeded {
        sprite_seed_z(gradient, screen_top, height, z_adjust).0
    } else {
        ((DEFAULT_Z - height - screen_top + 1) & 0xFFFF) + z_adjust
    }
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
/// `NormalZAdjust`, anim `ZAdjust`, ...).
pub fn ground_anchored_z_adjust(height_level: i32, class_term: i32) -> i32 {
    class_term - HEIGHT_LEVEL_PX * height_level
}

/// BUILDNGZ.SHA canvas point that lands on a building's draw point before the
/// foundation shift (`BuildingClass_DrawBody 0x0043D6EF..0x0043D77A`).
pub const ZSHAPE_ANCHOR: (i32, i32) = (0xC6, 0x1BE);

/// The same point for a remembered building under fog of war:
/// FoggedObjectClass::DrawAll adds (0x90, 0xAC) to ZShapePointMove
/// (`0x004D1DBD`, `0x004D1DCE`) and passes it from the same draw point.
pub const FOGGED_ZSHAPE_ANCHOR: (i32, i32) = (0x90, 0xAC);

/// `BuildingClass_DrawBody @ 0x0043D787`: a foundation width of 8 or more
/// draws without a z-shape (`CMP EAX, 8 / JL`).
pub const ZSHAPE_MAX_FOUNDATION_WIDTH: u16 = 7;

/// World-pixel origin of the z-shape frame for one building.
///
/// The body pushes `anchor + ZShapePointMove - CellToPixel((W-1)*256,
/// (H-1)*256)` as the z-shape offset, `anchor` being [`ZSHAPE_ANCHOR`] or
/// [`FOGGED_ZSHAPE_ANCHOR`]; the extended blitter reads pixel `p` of the
/// z-shape's frame for screen pixel `draw_point - offset - frame + p`, where
/// `frame` is that frame's offset in its canvas: CC_Draw_Shape subtracts it
/// (`0x004AF08E..0x004AF0CB`) where a canvas placement would add it.
/// `draw_point` is the point `CC_Draw_Shape` centres the body on (VERA's
/// building anchor), in world pixels.
pub fn zshape_origin(
    anchor: (i32, i32),
    draw_point: (i32, i32),
    foundation: (u16, u16),
    point_move: (i32, i32),
    frame: (i32, i32),
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
        anchor.0 + point_move.0 - far_corner.0,
        anchor.1 + point_move.1 - far_corner.1,
    );
    (
        draw_point.0 - offset.0 - frame.0,
        draw_point.1 - offset.1 - frame.1,
    )
}

/// The source columns the view's left edge, at world column `view_left`,
/// clips off a body's rect from world column `left` (WGSL `round` ties to
/// even). The RLE ZReadWrite leaf (`0x004990E0`) skips them a token at a
/// time; [`zshape_skip_overshoot`] gives what that leaves behind per row.
pub fn zshape_lead_skip(view_left: i32, left: f32) -> f32 {
    view_left
        .saturating_sub(left.round_ties_even() as i32)
        .max(0) as f32
}

/// The source columns the RLE ZReadWrite leaf (`0x004990E0`) leaves a body's
/// z-shape pointer behind by in one frame row (`row`, its palette indices),
/// skipped `skip` columns in from the left ([`zshape_lead_skip`]). The leaf
/// skips a token at a time (`0x00499141..0x00499154`); a skip that ends inside
/// a transparent run moves dest, Z and A on to the run's end
/// (`0x0049915D..0x0049918F`) but not the z-shape pointer (ECX), which drawn
/// pixels and later runs advance (`0x00499234`, `0x004991C7`): the rest of
/// the row reads its shape that many columns back. A transparent span is
/// taken as runs of 255 from its start, as retail frames encode it. The skip
/// table carries the result per row (`render::zshape_skips`).
///
/// RESIDUAL: a mod frame that splits its spans otherwise overshoots
/// differently, and ore on a slope (the linear shape, its quad already clipped
/// on the CPU) keeps no overshoot: where the view's left edge cuts them, their
/// depth can sit up to a run's worth of shape columns off.
pub fn zshape_skip_overshoot(row: &[u8], skip: usize) -> u8 {
    let width = row.len();
    if skip == 0 || skip >= width || row[skip - 1] != 0 {
        return 0;
    }
    // The transparent span the skip ends in, and the run of it the skip's
    // last token covers.
    let start = row[..skip - 1]
        .iter()
        .rposition(|&index| index != 0)
        .map_or(0, |drawn| drawn + 1);
    let run_end = (start + ((skip - 1 - start) / 255 + 1) * 255).min(width);
    let end = row[skip..run_end]
        .iter()
        .position(|&index| index != 0)
        .map_or(run_end, |drawn| skip + drawn);
    (end - skip) as u8
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

#[cfg(all(test, tests_render_a_r))]
mod tests {
    /// The two cases of depth_gpu_tests
    /// `building_zshape_trails_a_left_edge_skip_that_ends_inside_a_run`, as
    /// the table computes them, and the skips that leave nothing behind.
    #[test]
    fn a_skip_ending_inside_a_transparent_run_trails_to_the_runs_end() {
        use super::zshape_skip_overshoot;
        // A 4-column run cut 2 in: the pointer trails by 2 to column 4.
        assert_eq!(zshape_skip_overshoot(&[0, 0, 0, 0, 33, 33, 33, 33], 2), 2);
        // A 300-column run encodes as 255 + 45: the skip ends 245 short of
        // the first token's end.
        let mut row = vec![0; 320];
        row[300..].fill(33);
        assert_eq!(zshape_skip_overshoot(&row, 10), 245);
        // A drawn pixel before the skip starts the span and its runs there.
        row[4] = 33;
        assert_eq!(zshape_skip_overshoot(&row, 10), 250);
        // A skip that ends on a drawn pixel, or outside the frame, trails nothing.
        assert_eq!(zshape_skip_overshoot(&[33, 0, 0, 33], 1), 0);
        assert_eq!(zshape_skip_overshoot(&[0, 0, 33], 3), 0);
        assert_eq!(zshape_skip_overshoot(&[0, 0, 33], 0), 0);
    }

    #[test]
    fn voxel_candidate_envelope_covers_existing_regions_and_shader_clip_boundaries() {
        for rect in [[43, 15], [43, 48], [-10, 64]] {
            for gradient in [
                pack_voxel_z_gradient(ZGradient::Flat, false),
                pack_voxel_z_gradient(ZGradient::Vertical, false),
                pack_voxel_z_gradient(ZGradient::Vertical, true),
            ] {
                for clip in [[0, 96], [48, 62], [-5, 10], [200, 300]] {
                    for camera in [0, -32725] {
                        let range = voxel_depth_range(rect, gradient, -65, camera, clip);
                        for world_row in rect[0]..rect[0] + rect[1] {
                            if let Some(region) =
                                voxel_depth_region(rect, gradient, -65, world_row, clip)
                            {
                                let value = sprite_row_z(
                                    region.gradient,
                                    region.top - camera,
                                    region.height,
                                    region.z_adjust,
                                    world_row - region.top,
                                );
                                let [min, max] = range.expect("an admitted region has an envelope");
                                assert!(
                                    min <= value && value <= max,
                                    "{rect:?}/{gradient}/{clip:?}/{world_row}: {range:?} misses{value}"
                                );
                            }
                        }
                    }
                }
            }
        }
        assert!(voxel_depth_range([43, 15], 0, -65, 0, [0, 96]).unwrap()[0] >= 0);
        assert!(voxel_depth_range([43, 15], 0, -65, -32725, [-32725, -32629]).unwrap()[1] < 0);
        assert_eq!(voxel_depth_range([i32::MAX, 15], 0, -65, 0, [0, 96]), None);
    }

    /// TacticalClass_Draw 0x006D3D8A..0x006D3DC6 and Pan 0x007BCD14: small
    /// moves pan the seed; a move as tall as the view, a complete redraw or
    /// a radar click does not, and a new view size starts it over.
    #[test]
    fn the_seed_pans_with_small_moves_alone() {
        let mut seed = ZSeed::default();
        let view = |left, top| [left, top, 800, 600];
        assert_eq!(seed.frame(view(0, 1000)), 0);
        assert_eq!(seed.frame(view(30, 1010)), 10);
        assert_eq!(seed.frame(view(30, 1609)), 609);
        assert_eq!(seed.frame(view(30, 2209)), 609, "a whole view's move");
        assert_eq!(seed.frame(view(-770, 2210)), 609, "a whole view's width");
        seed.complete_redraw();
        assert_eq!(seed.frame(view(-770, 2215)), 609);
        assert_eq!(seed.frame(view(-770, 2205)), 599);
        assert_eq!(seed.seed(), DEFAULT_Z - 599);
        seed.reset();
        assert_eq!(seed.frame(view(-770, 2200)), 0);
        assert_eq!(seed.frame(view(-770, 2201)), 1);
        assert_eq!(seed.frame([-770, 2202, 800, 599]), 0, "a new view size");
    }

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
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/terrain_draw_oracle/fixtures/rows.json",
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
        // An unsplit body is one Bit_Blit, clipped to the view before its
        // seed (0x00437461); a shadow's plain gradient is not.
        let ordinary = voxel_depth_region(
            [-4, 32],
            pack_voxel_z_gradient(ZGradient::Moderate, false),
            7,
            6,
            [0, 24],
        )
        .unwrap();
        assert_eq!(ordinary.top, 0);
        assert_eq!(ordinary.height, 24);
        assert_eq!(ordinary.gradient, ZGradient::Moderate);
        assert_eq!(ordinary.z_adjust, 7);
        let shadow = voxel_depth_region([-4, 32], ZGradient::Moderate as u32, 7, 6, [0, 24]);
        assert_eq!(
            shadow.map(|region| [region.top, region.height]),
            Some([-4, 32])
        );
    }

    #[test]
    fn building_shape_uses_unquantised_bottom_seed_and_raw_frames_only_clip() {
        // Regression derived from the 437C39..437C72 arithmetic, not a
        // captured parity golden. A neutral shape must have one Z at all rows.
        assert_eq!(
            zshape_seed_z(ZGradient::Vertical, 100, 30, -2),
            DEFAULT_Z - 131
        );
        assert_ne!(
            zshape_seed_z(ZGradient::Vertical, 100, 30, -2),
            sprite_row_z(ZGradient::Vertical, 100, 30, -2, 0),
        );
        assert_eq!(
            zshape_seed_z(ZGradient::Vertical, 85, 30, -17),
            zshape_seed_z(ZGradient::Vertical, 100, 30, -2)
        );
        assert_eq!(pack_building_z_gradient(true, true), 0x302);
        assert_eq!(pack_building_z_gradient(true, false), 0x202);
        assert_eq!(pack_building_z_gradient(false, true), 2);
    }

    #[test]
    fn shape_seeds_match_original_gradient_branches() {
        let native = crate::test_fixture::json("tools/spatial_oracle/ore_shape_clip.json");
        for row in native["shape_seeds"].as_array().unwrap() {
            let value = |key: &str| row[key].as_i64().unwrap() as i32;
            assert_eq!(
                zshape_seed_z(
                    ZGradient::from_index(value("gradient") as u32),
                    value("screen_top"),
                    value("height"),
                    value("z_adjust")
                ),
                value("seed"),
                "{row}"
            );
        }
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
            zshape_origin(ZSHAPE_ANCHOR, (1000, 500), (1, 1), (0, 0), (0, 0)),
            (1000 - 0xC6, 500 - 0x1BE)
        );
        // 3x3: far corner (2,2) cells -> (0, 60) px, so the canvas drops 60 rows.
        assert_eq!(
            zshape_origin(ZSHAPE_ANCHOR, (1000, 500), (3, 3), (0, 0), (0, 0)),
            (1000 - 0xC6, 500 - 0x1BE + 60)
        );
        // 4x2 (GAWEAP-like) with ZShapePointMove=30,15.
        let (x, y) = zshape_origin(ZSHAPE_ANCHOR, (1000, 500), (4, 2), (30, 15), (0, 0));
        assert_eq!((x, y), (1000 - (0xC6 + 30 - 60), 500 - (0x1BE + 15 - 60)));
        // A frame set into its canvas moves the other way.
        assert_eq!(
            zshape_origin(ZSHAPE_ANCHOR, (1000, 500), (1, 1), (0, 0), (5, -3)),
            (1000 - 0xC6 - 5, 500 - 0x1BE + 3)
        );
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
