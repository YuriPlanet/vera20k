//! Exact ordinary, unmirrored VXL draw preparation and encoded-span raster.
//!
//! Original 7540F0/754510/756590 own corners, shared center, crop and integer
//! strides. This owner is separate from voxel shadow/model-to-world geometry.
//!
//! A `UseBuffer=yes` TechnoType's body, turret and barrel each draw with
//! 0xB43180 set (`0x00706ED0` sets it at `0x00706F12` and clears it at
//! `0x0070724F`): 754510 rasterizes the sorted sections from the last, and
//! 756590 adds Z words and dispatches the depth-tested leaves 757980/757BF0
//! (leaf table `0x00846840`).
//!
//! RESIDUAL (UseBuffer state across draws). VERA always draws the ordinary
//! style: each draw starts from a clear depth map and the flag follows the
//! drawn type. Native keeps the colour buffer (0xB2FF78), depth map, the
//! previous draw's crop rect (0xB2FB60..6C, stored by 754510 at
//! `0x007546A1` and by 7542F0) and 0xB43180 across every draw. Each draw
//! starts with 753E00: its first call (0x8467E0) clears both buffers whole;
//! later calls clear the colour buffer over the previous rect, inclusive
//! (whole when that rect leaves the map), and the depth map over the same
//! rect only while the flag is set (`0x00753EC5`). The uncached part draw
//! 706ED0 sets the flag for a UseBuffer type before its clear and clears it
//! after; the uncached shadow draw 707280 sets it before its clear
//! (`0x007072FC`) and again after (`0x0070744B`), and never clears it. While
//! it is set every caller rasterizes depth-tested, and 706640 stores what a
//! miss rendered. Reproducing this needs a runtime native-order software
//! voxel pass with native's cache lifetime (related: issue #828); VERA bakes
//! its atlas at load and its pose cache in presentation order.
//!
//! Retail UseBuffer types are HIND, SCHD, SCHP and SHAD: VehicleTypes with
//! the Jumpjet locomotor, SpeedType=Hover and no BalloonHover, so the
//! Aircraft shadow caller (`0x004148A0`) never runs for them. Their shadow
//! goes Unit `0x0073C202..0x0073C5C4` -> `0x004DB0D0` (Jumpjet
//! Is_To_Have_Shadow `0x0055ABE0` returns 1) -> `0x00706BD0`. The Unit key
//! goes -1 only for BalloonHover (+0xD6A) or SpeedType Track/Wheel (+0x67C
//! 1/2) (`0x0073C43F..0x0073C472`), none of the four, but 706BD0 drops the
//! key for `DisableShadowCache=` (+0xDBF, ART read `0x0071598B`, stored at
//! `0x007159A6`; tested at `0x00706C33`). Retail ART sets it, with
//! `DisableVoxelCache=` (+0xDBE, `0x00706806`), on SHAD, PDPLANE and SPYP.
//!
//! Flag leak (per frame): each drawn SHAD (Night Hawk Transport) renders its
//! body through 706ED0 every frame (set, draw, clear), then, after the body
//! in `0x0073B470`, its shadow through 707280, which leaves the flag set. It
//! stays set until the next SHAD body (next frame while one is drawn) or a
//! HIND/SCHD/SCHP part cache miss; with no SHAD drawn any more, until such a
//! miss. Meanwhile every voxel draw is depth-tested (two-pixel voxels,
//! nearest wins, reversed section order): voxel bullets (`0x0046824A` calls
//! 46B0C0 with no cache), voxel anim debris (`0x00749B70`), PDPLANE and
//! SPYP bodies, key -1 crash and rocket bodies, and every other type's body,
//! turret or barrel cache miss, whose image is then reused. Shadow stencils
//! ignore the flag. Cached images last until `0x005F99E0` frees every
//! type's caches and resets the 4,000,000-byte pool, called only when a
//! store finds the pool full (`0x00706AE0`, `0x00706E38`). Which draws fall
//! between a frame's last SHAD shadow and the next SHAD body depends on draw
//! order (not traced); the look is not captured against retail.
//!
//! Stale depth (rare, not measured): a HIND/SCHD/SCHP part miss (clears the
//! flag), a shadow cache hit, smaller flag-off draws, then a UseBuffer draw
//! keeps the miss's depth outside the smaller rect and drops the new draw's
//! voxels there that are not nearer. SHAD alone never leaves stale depth:
//! its shadow sets the flag before clearing the body's rect.
//!
//! A store at depth address 0xFFFF also writes one byte past both 64 KiB
//! buffers, which is dropped here.

use super::{HvaFile, Mat4, Vec3, VxlFile};
use crate::util::native_affine::{ftol, load, matrix_product, store, transform_point};
use crate::util::native_x87::X87Chop53 as Fpu;

// Original 8468C0: start, X edge, Y edge, Z edge, start-X, start-Y,
// column-X step, column-Y step. Rows 4..7 dispatch backward encoded Z.
const ORIENTATIONS: [[i32; 8]; 8] = [
    [0, 3, 1, 4, 1, 1, -1, -1],
    [1, 2, 0, 5, 1, 0, -1, 1],
    [2, 1, 3, 6, 0, 0, 1, 1],
    [3, 0, 2, 7, 0, 1, 1, -1],
    [4, 7, 5, 0, 1, 1, -1, -1],
    [5, 6, 4, 1, 1, 0, -1, 1],
    [6, 5, 7, 2, 0, 0, 1, 1],
    [7, 4, 6, 3, 0, 1, 1, -1],
];

#[derive(Debug, Clone)]
pub(super) struct RasterParams {
    pub column_steps: [i32; 3],
    pub origin_xy: [u16; 2],
    pub axis_xy: [[i16; 2]; 3],
    /// UseBuffer only: the origin's Z word (P+0x1C) and each grid axis's Z
    /// step (P+0x22/+0x28/+0x2E). 756590 computes the steps only under
    /// 0xB43180 (`0x007567B0`); the ordinary leaves read neither.
    pub origin_z: u16,
    pub axis_z: [i16; 3],
    pub sizes: [u8; 3],
    pub reverse: bool,
}

#[derive(Debug)]
pub(super) struct Section {
    #[cfg(test)]
    pub matrix: Mat4,
    #[cfg(test)]
    pub hva_matrix: Option<Mat4>,
    pub corners: [Vec3; 8],
    #[cfg(test)]
    pub minimum: Vec3,
    pub maximum: Vec3,
    pub corner_index: usize,
    pub params: RasterParams,
}

#[derive(Debug)]
pub(super) struct Geometry {
    pub sections: Vec<Section>,
    /// 754510's exchange-sorted section list.
    pub paint_order: Vec<usize>,
    /// 0xB43180: the draw rasterizes through the depth-tested leaves.
    pub use_buffer: bool,
    /// Native world offset X/Y, fixed-buffer crop X/Y, width/height.
    pub rect: [i32; 6],
}

pub(super) fn prepare_geometry(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    frame: u32,
    draw_matrix: Mat4,
    use_buffer: bool,
) -> Option<Geometry> {
    let scale = vxl.limbs.first()?.scale;
    let mut sections = Vec::with_capacity(vxl.limbs.len());
    let mut minimum = Vec3::splat(10000.0);
    let mut maximum = Vec3::splat(-10000.0);
    for (index, limb) in vxl.limbs.iter().enumerate() {
        let sizes = [limb.size_x, limb.size_y, limb.size_z];
        if sizes.contains(&0) {
            return None;
        }
        let hva_matrix = if let Some(hva) = hva {
            let mut raw = *hva.get_transform(frame, index as u32)?;
            // 5F823E scales the entire HVA with limb zero's scale once.
            for column in [3, 7, 11] {
                raw[column] = store(Fpu::mul(load(raw[column])?, load(scale)?))?;
            }
            Some(super::hva_to_mat4(&raw, 1.0))
        } else {
            None
        };
        let model = match hva_matrix {
            Some(hva) => matrix_product(draw_matrix, hva)?,
            None => draw_matrix, // 706F64 bypasses HVA; tailer transform is not applied.
        };
        let matrix = matrix_product(Mat4::IDENTITY, model)?;
        let [lo_x, lo_y, lo_z, hi_x, hi_y, hi_z] = limb.bounds;
        let raw_corners = [
            Vec3::new(hi_x, hi_y, lo_z),
            Vec3::new(hi_x, lo_y, lo_z),
            Vec3::new(lo_x, lo_y, lo_z),
            Vec3::new(lo_x, hi_y, lo_z),
            Vec3::new(hi_x, hi_y, hi_z),
            Vec3::new(hi_x, lo_y, hi_z),
            Vec3::new(lo_x, lo_y, hi_z),
            Vec3::new(lo_x, hi_y, hi_z),
        ];
        let mut corners = [Vec3::ZERO; 8];
        let mut lo = Vec3::splat(10000.0);
        let mut hi = Vec3::splat(-10000.0);
        let mut corner_index = 0;
        for (i, corner) in raw_corners.into_iter().enumerate() {
            let mut point = transform_point(matrix, corner)?;
            point.y = -point.y;
            if point.z < lo.z {
                corner_index = i;
            }
            for axis in 0..3 {
                // Original comparisons replace only strict extrema; equal
                // values preserve the first stored bits, including signed 0.
                if point[axis] < lo[axis] {
                    lo[axis] = point[axis];
                }
                if point[axis] > hi[axis] {
                    hi[axis] = point[axis];
                }
            }
            corners[i] = point;
        }
        for axis in 0..3 {
            if lo[axis] < minimum[axis] {
                minimum[axis] = lo[axis];
            }
            if hi[axis] > maximum[axis] {
                maximum[axis] = hi[axis];
            }
        }
        sections.push(Section {
            #[cfg(test)]
            matrix,
            #[cfg(test)]
            hva_matrix,
            corners,
            #[cfg(test)]
            minimum: lo,
            maximum: hi,
            corner_index,
            params: RasterParams {
                column_steps: [0; 3],
                origin_xy: [0; 2],
                axis_xy: [[0; 2]; 3],
                origin_z: 0,
                axis_z: [0; 3],
                sizes,
                reverse: corner_index >= 4,
            },
        });
    }
    let mut center = Vec3::ZERO;
    let mut rect = [0i32; 6];
    for axis in 0..3 {
        center[axis] = store(Fpu::mul(
            Fpu::add(load(minimum[axis])?, load(maximum[axis])?),
            load(0.5)?,
        ))?;
        if axis < 2 {
            let extent = ftol(Fpu::sub(load(maximum[axis])?, load(minimum[axis])?))?;
            rect[axis + 4] = extent.checked_add(8)?;
            rect[axis + 2] = 124 - extent / 2;
            rect[axis] = ftol(load(center[axis])?)? - rect[axis + 4] / 2;
        }
    }
    for section in &mut sections {
        let table = ORIENTATIONS[section.corner_index];
        let origin = section.corners[table[0] as usize];
        let [nx, ny, _nz] = section.params.sizes.map(i32::from);
        section.params.column_steps = [
            table[4] * (nx - 1) + table[5] * (ny - 1) * nx,
            table[6],
            table[7] * nx,
        ];
        // Z only feeds the UseBuffer leaves; ordinary draws skip it.
        for axis in 0..if use_buffer { 3 } else { 2 } {
            let centered = Fpu::sub(
                Fpu::add(load(origin[axis])?, load(128.0)?),
                load(center[axis])?,
            );
            let origin_word = ftol(Fpu::mul(centered, load(256.0)?))? as u16;
            match axis {
                2 => section.params.origin_z = origin_word,
                _ => section.params.origin_xy[axis] = origin_word,
            }
            for grid_axis in 0..3 {
                let edge = section.corners[table[grid_axis + 1] as usize];
                let difference = Fpu::sub(load(edge[axis])?, load(origin[axis])?);
                let step = Fpu::div(
                    difference,
                    Fpu::load_i32(i32::from(section.params.sizes[grid_axis])),
                )
                .ok()?;
                let step = ftol(Fpu::mul(step, load(256.0)?))? as i16;
                match axis {
                    2 => section.params.axis_z[grid_axis] = step,
                    _ => section.params.axis_xy[grid_axis][axis] = step,
                }
            }
        }
    }
    let mut paint_order: Vec<usize> = (0..sections.len()).collect();
    // 7545B1 pairwise exchange, deliberately not a stable depth sort.
    for i in 0..paint_order.len() {
        for j in i + 1..paint_order.len() {
            if sections[paint_order[i]].maximum.z > sections[paint_order[j]].maximum.z {
                paint_order.swap(i, j);
            }
        }
    }
    Some(Geometry {
        sections,
        paint_order,
        use_buffer,
        rect,
    })
}

impl Geometry {
    /// The order 754510 rasterizes the sorted list in: from the last section
    /// under UseBuffer (`0x007545F3`), from the first otherwise.
    pub fn raster_order(&self) -> impl Iterator<Item = usize> + '_ {
        let last = self.paint_order.len().wrapping_sub(1);
        (0..self.paint_order.len())
            .map(move |step| self.paint_order[if self.use_buffer { last - step } else { step }])
    }
}

/// One literal visibility-map store. Order, including stores of zero, decides
/// the winner; palette values and VERA's auxiliary depth never decide it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PaintWrite {
    pub address: u16,
    pub color: u8,
    /// VERA-internal auxiliary depth for existing independently baked part
    /// composition. The native opaque leaf writes no per-pixel Z value.
    pub depth: f32,
}

pub(crate) struct PreparedDraw {
    pub writes: Vec<PaintWrite>,
    pub rect: [i32; 6],
    /// UseBuffer's byte depth map (0xB1D5E0) after the draw.
    #[cfg(test)]
    pub depth_map: Option<Vec<u8>>,
}

fn pair(x: i32, y: i32) -> u32 {
    u32::from(x as u16) | (u32::from(y as u16) << 16)
}

/// One encoded column's raster point.
#[derive(Clone, Copy)]
enum Cursor {
    /// 7DF9C0/7DFAE0: X in the low and Y in the high word of one dword, so
    /// a carry out of X enters Y.
    Packed(u32),
    /// 757980/757BF0: separate X, Y and Z 8.8 words, with no carry between.
    Split([u16; 3]),
}

impl Cursor {
    /// The visibility-map byte: the high bytes of Y and X.
    fn address(self) -> u16 {
        match self {
            Self::Packed(point) => (((point >> 8) & 0xff) | ((point >> 16) & 0xff00)) as u16,
            Self::Split([x, y, _]) => (y & 0xff00) | (x >> 8),
        }
    }
}

fn column_offset(body: &[u8], table: usize, column: usize) -> Option<i32> {
    let offset = table.checked_add(column.checked_mul(4)?)?;
    Some(i32::from_le_bytes(
        body.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// Execute 7DF9C0/7DFAE0's integer addressing over the original run stream.
/// Every read is bounded for malformed external assets; successful native
/// columns consume exactly size_z cells. Every run advances its byte pointer
/// even when skip/count are zero; checked reads bound such streams without
/// rejecting native-accepted empty runs based on an invented run-count cap.
///
/// With a depth map the section runs UseBuffer's 757980 (forward) or 757BF0
/// (backward) instead: a voxel draws only when its Z word's high byte is
/// strictly above the map's, and then stores that byte and its colour at its
/// pixel and the next one.
fn paint_section(
    limb: &super::VxlLimb,
    section: &Section,
    pages: &[u8; 256],
    vpl: Option<&super::VplFile>,
    writes: &mut Vec<PaintWrite>,
    mut depth_map: Option<&mut [u8]>,
) -> Option<()> {
    let spans = limb.native_spans.as_ref()?;
    let body = &spans.body;
    let params = &section.params;
    let [nx, ny, nz] = params.sizes.map(usize::from);
    let origin = pair(
        i32::from(params.origin_xy[0]),
        i32::from(params.origin_xy[1]),
    );
    let strides = params
        .axis_xy
        .map(|[x, y]| pair(i32::from(x), i32::from(y)));
    let [dx, dy] = params.axis_xy[2].map(i32::from);
    let split_origin = [params.origin_xy[0], params.origin_xy[1], params.origin_z];
    let split_axes: [[u16; 3]; 3] = std::array::from_fn(|grid| {
        let [x, y] = params.axis_xy[grid];
        [x as u16, y as u16, params.axis_z[grid] as u16]
    });
    // Skips step each word by count * Z-axis step, wrapping: 757980 builds
    // the X/Y products as a table (0xB45590) and multiplies Z by IMUL.
    // RESIDUAL: a skip of size_z or more reads a table entry that call did
    // not write; that needs an all-empty column without a -1 offset.
    let skip_by = |cursor: Cursor, skip: usize| match cursor {
        Cursor::Packed(point) => {
            Cursor::Packed(point.wrapping_add(pair(skip as i32 * dx, skip as i32 * dy)))
        }
        Cursor::Split(words) => Cursor::Split(std::array::from_fn(|c| {
            words[c].wrapping_add((skip as u16).wrapping_mul(split_axes[2][c]))
        })),
    };
    let step = |cursor: Cursor| match cursor {
        Cursor::Packed(point) => Cursor::Packed(point.wrapping_add(strides[2])),
        Cursor::Split(words) => Cursor::Split(std::array::from_fn(|c| {
            words[c].wrapping_add(split_axes[2][c])
        })),
    };
    let orientation = ORIENTATIONS[section.corner_index];
    let corner = section.corners[orientation[0] as usize];
    // Auxiliary VERA depth is attached to the winning native paint point.
    // It is not used for intra-VXL visibility and does not feed row-Z metadata.
    let depth_steps: [f32; 3] = std::array::from_fn(|axis| {
        (section.corners[orientation[axis + 1] as usize].z - corner.z)
            / f32::from(params.sizes[axis])
    });
    for y in 0..ny {
        for x in 0..nx {
            let column = params.column_steps[0]
                + y as i32 * params.column_steps[2]
                + x as i32 * params.column_steps[1];
            let column = usize::try_from(column).ok()?;
            if column >= nx * ny {
                return None;
            }
            let table = if params.reverse {
                spans.end_offset
            } else {
                spans.start_offset
            };
            let offset = column_offset(body, table, column)?;
            if offset < 0 {
                continue;
            }
            let mut pos = spans.data_offset.checked_add(offset as usize)?;
            let mut cursor = if depth_map.is_some() {
                Cursor::Split(std::array::from_fn(|c| {
                    split_origin[c]
                        .wrapping_add(split_axes[1][c].wrapping_mul(y as u16))
                        .wrapping_add(split_axes[0][c].wrapping_mul(x as u16))
                }))
            } else {
                Cursor::Packed(
                    origin
                        .wrapping_add(strides[1].wrapping_mul(y as u32))
                        .wrapping_add(strides[0].wrapping_mul(x as u32)),
                )
            };
            let mut z = 0usize;
            while z < nz {
                let (skip, count) = if params.reverse {
                    let count = usize::from(*body.get(pos)?);
                    pos = pos.checked_sub(1)?;
                    (0, count)
                } else {
                    let skip = usize::from(*body.get(pos)?);
                    let count = usize::from(*body.get(pos.checked_add(1)?)?);
                    pos = pos.checked_add(2)?;
                    (skip, count)
                };
                z = z.checked_add(skip)?;
                if z.checked_add(count)? > nz {
                    return None;
                }
                // Skip products truncate X and Y independently before their
                // packed dword is added. Occupied strides below retain carry.
                cursor = skip_by(cursor, skip);
                for _ in 0..count {
                    let (color, normal) = if params.reverse {
                        let normal = *body.get(pos)?;
                        let color = *body.get(pos.checked_sub(1)?)?;
                        pos = pos.checked_sub(2)?;
                        (color, normal)
                    } else {
                        let color = *body.get(pos)?;
                        let normal = *body.get(pos.checked_add(1)?)?;
                        pos = pos.checked_add(2)?;
                        (color, normal)
                    };
                    let color = vpl.map_or(color, |vpl| {
                        vpl.get_palette_index(pages[usize::from(normal)], color)
                    });
                    let write = PaintWrite {
                        address: cursor.address(),
                        color,
                        depth: corner.z
                            + x as f32 * depth_steps[0]
                            + y as f32 * depth_steps[1]
                            + z as f32 * depth_steps[2],
                    };
                    match (cursor, depth_map.as_deref_mut()) {
                        (Cursor::Split([.., word]), Some(map)) => {
                            // 757AB8..757B1F, 757CEC..757D53.
                            let depth = (word >> 8) as u8;
                            let address = usize::from(write.address);
                            if depth > map[address] {
                                map[address] = depth;
                                writes.push(write);
                                if let Some(next) = write.address.checked_add(1) {
                                    map[usize::from(next)] = depth;
                                    writes.push(PaintWrite {
                                        address: next,
                                        ..write
                                    });
                                }
                            }
                        }
                        _ => writes.push(write),
                    }
                    cursor = step(cursor);
                    z += 1;
                }
                if params.reverse {
                    // The pointer now names the leading count. Native skips
                    // it and consumes the preceding skip byte backward.
                    pos = pos.checked_sub(1)?;
                    let skip = usize::from(*body.get(pos)?);
                    // Native decrements even after the last run; the value is
                    // then unused. Saturation permits a stream at body start.
                    pos = pos.saturating_sub(1);
                    z = z.checked_add(skip)?;
                    if z > nz {
                        return None;
                    }
                    cursor = skip_by(cursor, skip);
                } else {
                    // The forward leaf ignores the duplicate-count value;
                    // the reverse leaf consumes that byte as its run count.
                    body.get(pos)?;
                    pos = pos.checked_add(1)?;
                }
            }
        }
    }
    Some(())
}

pub(super) fn prepare_draw(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    params: &super::VxlRenderParams,
    vpl: Option<&super::VplFile>,
) -> Option<PreparedDraw> {
    if params.scale != 1.0 || vxl.limbs.iter().any(|limb| limb.native_spans.is_none()) {
        return None;
    }
    let draw_matrix =
        super::voxel_params_draw_rotation(params, super::voxel_facing_step(params.facing));
    let geometry = prepare_geometry(vxl, hva, params.frame, draw_matrix, params.use_buffer)?;
    // 706ED0 calls 753D00 once for the VXL, before submitting its sections;
    // VoxelAnimClass::DrawIt calls 753C80 instead (749DA3). Either reads the
    // normal mode from limb zero, not switched for every limb.
    let normals_mode = vxl.limbs.first()?.normals_mode;
    let rotation = glam::Mat3::from_mat4(draw_matrix);
    let pages = params.lighting.pages(normals_mode, rotation);
    let mut writes = Vec::with_capacity(vxl.limbs.iter().map(|limb| limb.voxels.len()).sum());
    let mut depth_map = geometry.use_buffer.then(|| vec![0u8; 65536]);
    for index in geometry.raster_order() {
        paint_section(
            &vxl.limbs[index],
            &geometry.sections[index],
            &pages,
            vpl,
            &mut writes,
            depth_map.as_deref_mut(),
        )?;
    }
    Some(PreparedDraw {
        writes,
        rect: geometry.rect,
        #[cfg(test)]
        depth_map,
    })
}

impl PreparedDraw {
    /// Extract the native crop after completing the fixed 256x256 draw. A
    /// large/malformed crop is rejected; addressing inside the draw wraps as
    /// the original does, while reading outside its surface is never allowed.
    pub(crate) fn crop_indices(&self, visibility: &[u8]) -> Option<Vec<u8>> {
        let [_, _, x, y, width, height] = self.rect;
        if visibility.len() != 65536
            || x < 0
            || y < 0
            || width <= 0
            || height <= 0
            || x.checked_add(width)? > 256
            || y.checked_add(height)? > 256
        {
            return None;
        }
        let mut pixels = Vec::with_capacity((width * height) as usize);
        for row in y..y + height {
            let start = (row * 256 + x) as usize;
            pixels.extend_from_slice(&visibility[start..start + width as usize]);
        }
        Some(pixels)
    }

    pub(crate) fn render_cpu(&self) -> Option<super::VxlSprite> {
        let mut visibility = vec![0u8; 65536];
        let mut depth = vec![f32::NEG_INFINITY; 65536];
        for write in &self.writes {
            let index = usize::from(write.address);
            visibility[index] = write.color;
            depth[index] = if write.color == 0 {
                f32::NEG_INFINITY
            } else {
                write.depth
            };
        }
        let palette_indices = self.crop_indices(&visibility)?;
        let [offset_x, offset_y, x, y, width, height] = self.rect;
        let mut cropped_depth = Vec::with_capacity(palette_indices.len());
        for row in y..y + height {
            let start = (row * 256 + x) as usize;
            cropped_depth.extend_from_slice(&depth[start..start + width as usize]);
        }
        Some(super::VxlSprite {
            palette_indices,
            depth: cropped_depth,
            width: width as u32,
            height: height as u32,
            offset_x: offset_x as f32,
            offset_y: offset_y as f32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn bytes(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    fn words(value: &Value) -> Vec<u32> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|word| word.as_u64().unwrap() as u32)
            .collect()
    }

    fn matrix_bits(matrix: Mat4) -> Vec<u32> {
        (0..3)
            .flat_map(|row| (0..4).map(move |col| matrix.col(col)[row].to_bits()))
            .collect()
    }

    /// One native Render case through the production geometry, paint and
    /// crop. Returns its backward raster dispatches and the zero results that
    /// erased an earlier visible pixel.
    fn compare_native_render(
        vxl: &VxlFile,
        hva: Option<&HvaFile>,
        vpl: &super::super::VplFile,
        params: &super::super::VxlRenderParams,
        matrix: Mat4,
        case: &Value,
        label: &str,
    ) -> (usize, usize) {
        let mut backward = 0;
        assert_eq!(
            matrix_bits(matrix),
            words(&case["input_matrix_bits"]),
            "{label} production draw matrix"
        );
        let geometry = prepare_geometry(vxl, hva, 0, matrix, params.use_buffer).unwrap();
        let expected_rect: Vec<i32> = case["rect"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as i32)
            .collect();
        assert_eq!(
            geometry.rect.as_slice(),
            expected_rect,
            "{label} all six crop outputs"
        );
        assert_eq!(
            geometry
                .paint_order
                .iter()
                .map(|i| *i as u32)
                .collect::<Vec<_>>(),
            words(&case["paint_order"]),
            "{label} section exchange order"
        );
        for (index, section) in geometry.sections.iter().enumerate() {
            assert_eq!(
                matrix_bits(section.matrix),
                words(&case["section_matrix_bits"][index]),
                "{label} section {index} matrix"
            );
            if let Some(hva_matrix) = section.hva_matrix {
                assert_eq!(
                    matrix_bits(hva_matrix),
                    words(&case["hva_matrix_bits"][index]),
                    "{label} section {index} globally scaled HVA"
                );
            } else {
                assert!(case["hva_matrix_bits"].as_array().unwrap().is_empty());
            }
            let expected = &case["boxes"][index];
            assert_eq!(
                section.corner_index as u64,
                expected["corner_index"].as_u64().unwrap(),
                "{label} section {index} orientation"
            );
            let extents = [section.minimum, section.maximum]
                .into_iter()
                .flat_map(|v| v.to_array().map(f32::to_bits))
                .collect::<Vec<_>>();
            assert_eq!(
                extents,
                words(&expected["extents_bits"]),
                "{label} section {index} bounds bits"
            );
            let corners = section
                .corners
                .into_iter()
                .flat_map(|v| v.to_array().map(f32::to_bits))
                .collect::<Vec<_>>();
            assert_eq!(
                corners,
                words(&expected["corner_bits"]),
                "{label} section {index} corner bits"
            );
        }
        for (order, index) in geometry.raster_order().enumerate() {
            let params = &geometry.sections[index].params;
            let mut actual = serde_json::json!({
                "entry": match (geometry.use_buffer, params.reverse) {
                    (false, false) => 0x7df9c0,
                    (false, true) => 0x7dfae0,
                    (true, false) => 0x757980,
                    (true, true) => 0x757bf0,
                },
                "column_steps": params.column_steps, "origin_xy": params.origin_xy,
                "axis_xy": params.axis_xy, "sizes": params.sizes,
            });
            if geometry.use_buffer {
                actual["origin_z"] = params.origin_z.into();
                actual["axis_z"] = serde_json::json!(params.axis_z);
            }
            assert_eq!(
                actual, case["raster_params"][order],
                "{label} section {index} live integer parameters"
            );
            backward += usize::from(params.reverse);
        }
        let draw = prepare_draw(vxl, hva, params, Some(vpl)).unwrap();
        let mut visibility = vec![0u8; 65536];
        let mut zero = 0;
        let mut erased = 0;
        for write in &draw.writes {
            let address = usize::from(write.address);
            if write.color == 0 {
                zero += 1;
                erased += usize::from(visibility[address] != 0);
            }
            visibility[address] = write.color;
        }
        assert_eq!(
            serde_json::json!({"all": draw.writes.len(), "zero": zero, "zero_erases_nonzero": erased}),
            case["write_counts"],
            "{label} all paint events"
        );

        let mut expected = vec![0u8; 65536];
        for pixel in case["pixels"].as_array().unwrap() {
            expected[pixel[0].as_u64().unwrap() as usize] = pixel[1].as_u64().unwrap() as u8;
        }
        assert_eq!(
            visibility, expected,
            "{label} complete native visibility buffer"
        );
        if params.use_buffer {
            let mut expected = vec![0u8; 65536];
            for pixel in case["depth"].as_array().unwrap() {
                expected[pixel[0].as_u64().unwrap() as usize] = pixel[1].as_u64().unwrap() as u8;
            }
            assert!(
                draw.depth_map.as_ref() == Some(&expected),
                "{label} complete native depth map"
            );
        }
        let sprite = super::super::render_vxl(vxl, hva, params, Some(vpl));
        assert_eq!(
            sprite.palette_indices,
            draw.crop_indices(&expected).unwrap(),
            "{label} production CPU crop"
        );
        assert_eq!(
            [
                sprite.offset_x as i32,
                sprite.offset_y as i32,
                sprite.width as i32,
                sprite.height as i32
            ],
            [
                geometry.rect[0],
                geometry.rect[1],
                geometry.rect[4],
                geometry.rect[5]
            ],
            "{label} production sprite metadata"
        );
        assert_eq!(
            super::super::native_vxl_draw_bounds(vxl, hva, params),
            Some([
                geometry.rect[0],
                geometry.rect[1],
                geometry.rect[4],
                geometry.rect[5]
            ]),
            "{label} shared row-Z metadata authority"
        );
        (backward, erased)
    }

    /// Actual Bullet468090 ->46B0C0 ->754510 execution, including its
    /// velocity conversion, HVA frame0, diffuse pages and encoded span writes.
    #[test]
    fn kirov_bullet_pose_and_raster_match_original_execution() {
        let fixture: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/voxel_oracle/bullet_fixtures.json",
        ))
        .unwrap();
        // Honor the required-retail INI contract as well as archive admission.
        // The corpus contains derived native output and asset identities only;
        // copyrighted VXL/HVA/VPL input bytes come from the local install.
        let Some(_battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let Some((_, local_assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let assets = &fixture["assets"];
        let asset_bytes = |kind: &str| {
            local_assets
                .get_ref(assets[kind]["name"].as_str().unwrap())
                .unwrap()
        };
        for (name, asset) in assets.as_object().unwrap() {
            let data = asset_bytes(name);
            assert_eq!(data.len() as u64, asset["bytes"], "{name} length");
            assert_eq!(
                crate::util::sha256::sha256_hex(data),
                asset["sha256"],
                "{name} hash"
            );
        }
        let vxl = VxlFile::from_bytes(asset_bytes("vxl")).unwrap();
        let hva = HvaFile::from_bytes(asset_bytes("hva")).unwrap();
        let vpl = super::super::VplFile::from_bytes(asset_bytes("vpl")).unwrap();
        let controls = fixture["controls"].as_array().unwrap();
        assert_eq!(controls.len(), 22);
        for row in controls {
            let label = row["name"].as_str().unwrap();
            let velocity =
                crate::sim::projectile::ProjectileVelocity::from_native(std::array::from_fn(|i| {
                    crate::util::native_x87::NativeF64Bits::from_bits(
                        u64::from_str_radix(row["velocity_bits"][i].as_str().unwrap(), 16).unwrap(),
                    )
                }));
            let [heading, pitch] = super::super::projectile_voxel_angles(velocity).unwrap();
            assert_eq!(u64::from(heading), row["heading_word"], "{label} heading");
            assert_eq!(u64::from(pitch), row["pitch_word"], "{label} pitch");
            assert_eq!(
                u64::from(super::super::voxel_facing_step_u16(heading)),
                row["heading_step"],
                "{label}"
            );
            assert_eq!(
                u64::from(super::super::voxel_facing_step_u16(pitch)),
                row["pitch_step"],
                "{label}"
            );
            let matrix = super::super::projectile_voxel_matrix(velocity).unwrap();
            assert_eq!(matrix_bits(matrix), words(&row["matrix_bits"]), "{label}");
            let params = super::super::VxlRenderParams {
                body_tilt: Some(super::super::BodyTilt::Matrix(matrix)),
                lighting: super::super::VxlLighting::Diffuse,
                ..Default::default()
            };
            assert_eq!(
                params
                    .lighting
                    .pages(vxl.limbs[0].normals_mode, glam::Mat3::from_mat4(matrix))
                    .as_slice(),
                bytes(row["body"]["normal_pages"].as_str().unwrap()),
                "{label} diffuse"
            );
            compare_native_render(&vxl, Some(&hva), &vpl, &params, matrix, &row["body"], label);
        }
    }

    #[test]
    fn native_sections_match_all_geometry_and_visibility_bytes() {
        let fixture: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/voxel_oracle/raster_fixtures.json",
        ))
        .unwrap();
        let mut compared = 0;
        let mut backward = 0;
        let mut zero_erases = 0;
        let mut buffered = 0;
        for model in fixture["models"].as_array().unwrap() {
            let use_buffer = model["use_buffer"].as_bool().unwrap_or(false);
            let vxl = VxlFile::from_bytes(&bytes(model["vxl"].as_str().unwrap())).unwrap();
            let hva = model["hva"]
                .as_str()
                .map(|hex| HvaFile::from_bytes(&bytes(hex)).unwrap());
            let vpl =
                super::super::VplFile::from_bytes(&bytes(model["vpl"].as_str().unwrap())).unwrap();
            for case in model["cases"].as_array().unwrap() {
                let step = case["step"].as_u64().unwrap() as u8;
                let label = format!("{} step {step}", model["name"].as_str().unwrap());
                let params = super::super::VxlRenderParams {
                    facing: step * 8,
                    use_buffer,
                    ..Default::default()
                };
                let matrix = super::super::voxel_draw_rotation_for_state(0, None, step);
                assert_eq!(
                    super::super::vxl_normals::blinn_phong_pages(
                        vxl.limbs[0].normals_mode,
                        glam::Mat3::from_mat4(matrix)
                    )
                    .as_slice(),
                    bytes(case["normal_pages"].as_str().unwrap()),
                    "{label} native normal pages"
                );
                let (reversed, erased) =
                    compare_native_render(&vxl, hva.as_ref(), &vpl, &params, matrix, case, &label);
                if use_buffer {
                    buffered += 1;
                    continue;
                }
                backward += reversed;
                zero_erases += erased;
                compared += 1;
            }
        }
        assert_eq!(compared, 32);
        assert_eq!(backward, 20);
        assert_eq!(zero_erases, 1729);
        // UseBuffer: the same models at four facings through 757980/757BF0.
        assert_eq!(buffered, 16);
    }

    /// `VoxelAnimClass::DrawIt`'s body (`0x00749D73..0x00749DD4`): the bounce
    /// drawing matrix under the identity viewer, no HVA, lit through 753C80.
    #[test]
    fn tumbling_body_matches_original_execution() {
        let fixture: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/voxel_oracle/tumble_fixtures.json",
        ))
        .unwrap();
        let vxl = VxlFile::from_bytes(&bytes(fixture["vxl"].as_str().unwrap())).unwrap();
        let vpl =
            super::super::VplFile::from_bytes(&bytes(fixture["vpl"].as_str().unwrap())).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        let mut backward = 0;
        for case in cases {
            let body = &case["body"];
            let label = format!("flight {} tick {}", case["flight"], case["tick"]);
            let bits = words(&body["input_matrix_bits"]);
            let matrix =
                super::super::hva_to_mat4(&std::array::from_fn(|i| f32::from_bits(bits[i])), 1.0);
            assert_eq!(
                super::super::vxl_normals::diffuse_pages(
                    vxl.limbs[0].normals_mode,
                    glam::Mat3::from_mat4(matrix)
                )
                .as_slice(),
                bytes(body["normal_pages"].as_str().unwrap()),
                "{label} native diffuse pages"
            );
            let params = super::super::VxlRenderParams {
                body_tilt: Some(super::super::BodyTilt::Matrix(matrix)),
                lighting: super::super::VxlLighting::Diffuse,
                ..Default::default()
            };
            backward += compare_native_render(&vxl, None, &vpl, &params, matrix, body, &label).0;
        }
        assert_eq!(cases.len(), 24);
        assert_eq!(backward, 7);
    }
}
