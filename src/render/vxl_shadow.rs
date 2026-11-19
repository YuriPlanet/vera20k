//! Native selected-section VXL ground shadow geometry.
//!
//! Original 753F90 submits four bottom-face corners; 756860 walks the span
//! start table in 8.8 coordinates and writes TWO horizontal stencil pixels.
//! This is separate from the opaque VXL encoded-Z painter and from final
//! destination darkening. The 256-square composed Unit surface masks the
//! first cache fill; the resulting stencil is then retained by its owner.

use super::{HvaFile, Vec3, VxlFile, VxlRenderParams, VxlSprite};
use crate::util::native_affine::{ftol, load, matrix_product, store, transform_point};
use crate::util::native_x87::X87Chop53 as Fpu;

pub(crate) fn supported(vxl: &VxlFile, params: &VxlRenderParams) -> bool {
    params.scale == 1.0
        && params.frame == 0
        && params.slope_type < 17
        && params.slope_blend.is_none()
        && !vxl.limbs.is_empty()
}

#[cfg(all(test, tests_render_s_z))]
#[path = "vxl_shadow_tests.rs"]
mod tests;

/// `0x7072A1`'s `ChooseFrame` (`Ext/TechnoType/Hooks.MatrixOp.cpp:1074-1104`):
/// a non-negative `ShadowIndex.Frame` picks the shadow section's HVA frame,
/// modulo the file's frame count. A cleared `-1` falls through to
/// `WalkedFramesSoFar`, which this renderer cannot key: the shadow atlas is
/// baked by facing and slope, with no walked-frame dimension. It therefore
/// stays at frame 0, which is also what retail reaches because the constructor
/// value 0 never takes the `-1` arm. `ShadowIndices.Frame` carries its own
/// per-section frames and is not part of this function.
fn shadow_hva_frame(frame: i32, hva: &HvaFile) -> u32 {
    u32::try_from(frame.max(0)).unwrap_or(0) % hva.frame_count.max(1)
}

/// Native clear visibility buffer and crop. ShadowIndex selects one section
/// at `frame` of its HVA (`ShadowIndex.Frame`); ConsideredAircraft selects
/// the original half-size viewer.
pub(crate) fn render(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    params: &VxlRenderParams,
    shadow_index: i32,
    frame: i32,
    considered_aircraft: bool,
) -> Option<VxlSprite> {
    let matrix = draw_matrix(vxl, hva, params, shadow_index, frame)?;
    render_matrix(vxl, matrix, shadow_index, considered_aircraft)
}

/// `Matrix3D::VoxelDefaultMatrix * shadowMatrix`: the draw rotation the
/// caller hands every section (`Ext/TechnoType/Hooks.MatrixOp.cpp:783-800`
/// builds it once and passes `&mtx` to each `DrawVoxelShadow`).
fn draw_rotation(vxl: &VxlFile, params: &VxlRenderParams) -> Option<super::Mat4> {
    if !supported(vxl, params) {
        return None;
    }
    let step = super::voxel_facing_step(params.facing);
    // Phobos `UnitClass_DrawAsVXL_Shadow` (hook `0x73C47A`,
    // `Ext/TechnoType/Hooks.MatrixOp.cpp:729-773`): the shadow matrix inherits
    // the body's airborne `JumpjetTilt` roll/pitch, the same crash yaw/X/Y
    // product the body arm draws through. The other tilt arms keep the stock
    // slope/facing stencil matrix.
    Some(match params.body_tilt {
        Some(super::BodyTilt::JumpjetTilt(angles)) if tilt_is_active(angles) => {
            super::voxel_crash_rotation(step, angles)
        }
        _ => super::voxel_draw_rotation_for_state(params.slope_type, None, step),
    })
}

/// Phobos's shadow-matrix activity gate (`Ext/TechnoType/Hooks.MatrixOp.cpp:733`,
/// `:768`): while both angles stay under 0.005 the native body leaves the
/// `shadowMatrix` and the `vxlIndexKey` untouched, so such a pose draws the
/// stock facing/slope stencil bit for bit and reuses the baked cache entry.
pub(crate) fn tilt_is_active([sideways, forwards]: [f32; 2]) -> bool {
    sideways.abs() >= 0.005 || forwards.abs() >= 0.005
}

/// `*pMat * hva->Matrixes[section + LayerCount * ChooseFrame()]`
/// (`Ext/TechnoType/Hooks.MatrixOp.cpp:1074-1104`, hooked in
/// `VoxelClass::DrawVoxelShadow @ 0x7072A1`): the shared base matrix times
/// this section's own HVA transform. `ChooseFrame` picks the section's
/// `ShadowIndices.Frame`, `ShadowIndex.Frame` or the walked frame.
fn section_matrix(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    draw: super::Mat4,
    shadow_index: i32,
    frame: i32,
) -> Option<super::Mat4> {
    let limb_index = usize::try_from(shadow_index).ok()?;
    vxl.limbs.get(limb_index)?;
    if let Some(hva) = hva {
        let mut raw = *hva.get_transform(shadow_hva_frame(frame, hva), limb_index as u32)?;
        // The native HVA loader scales every section's translation by the
        // first VXL tailer's scale (5BD730), even for a different ShadowIndex.
        let scale = vxl.limbs.first()?.scale;
        for column in [3, 7, 11] {
            raw[column] = store(Fpu::mul(load(raw[column])?, load(scale)?))?;
        }
        matrix_product(draw, super::hva_to_mat4(&raw, 1.0))
    } else {
        Some(draw)
    }
}

fn draw_matrix(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    params: &VxlRenderParams,
    shadow_index: i32,
    frame: i32,
) -> Option<super::Mat4> {
    let draw = draw_rotation(vxl, params)?;
    section_matrix(vxl, hva, draw, shadow_index, frame)
}

/// Fractional center retained by 754510 for 756860's body-surface queries.
pub(crate) fn center(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    params: &VxlRenderParams,
    index: i32,
    frame: i32,
    aircraft: bool,
) -> Option<[f32; 2]> {
    let matrix = draw_matrix(vxl, hva, params, index, frame)?;
    center_of_matrix(vxl, matrix, index, aircraft)
}

/// The projected-bounds center of one section at a supplied matrix.
fn center_of_matrix(
    vxl: &VxlFile,
    matrix: super::Mat4,
    index: i32,
    aircraft: bool,
) -> Option<[f32; 2]> {
    let limb = vxl.limbs.get(usize::try_from(index).ok()?)?;
    let (_, lo, hi) = projected_quad(matrix, limb.bounds, aircraft)?;
    center_of_bounds(lo, hi)
}

fn center_of_bounds(lo: Vec3, hi: Vec3) -> Option<[f32; 2]> {
    let mut center = [0.; 2];
    for axis in 0..2 {
        center[axis] = store(Fpu::mul(
            Fpu::add(load(lo[axis])?, load(hi[axis])?),
            load(0.5)?,
        ))?;
    }
    Some(center)
}

///7568ED..756905 adds the caller's integer offset BEFORE ftol. Truncation
/// across zero is not translation-invariant (e.g. center -0.5, offset 1).
pub(crate) fn body_sample_offset(center: [f32; 2], offset: [i32; 2]) -> Option<[i32; 2]> {
    let mut result = [0; 2];
    for axis in 0..2 {
        let c = load(center[axis])?;
        result[axis] = ftol(Fpu::add(c, Fpu::load_i32(offset[axis])))?.wrapping_sub(ftol(c)?);
    }
    Some(result)
}

/// 756860's raster of one section's stencil before its stores: the 8.8
/// origin and column steps, and the crop.
#[derive(Debug, Clone)]
pub(crate) struct StencilRaster {
    pub limb: usize,
    pub origin: [u16; 2],
    pub step_x: [u16; 2],
    pub step_y: [u16; 2],
    pub rect: [i32; 6],
}

/// The stencil raster of a selected VXL section at a supplied matrix, with a
/// crop the 256-square buffer holds.
/// `VoxelAnimClass::DrawIt` passes 753F90 the bounce body's drawing matrix
/// directly, with the identity viewer and no HVA (`0x00749CE8`).
pub(crate) fn prepare_matrix(
    vxl: &VxlFile,
    matrix: super::Mat4,
    shadow_index: i32,
    considered_aircraft: bool,
) -> Option<StencilRaster> {
    prepare_matrix_at(vxl, matrix, shadow_index, considered_aircraft, None)
}

/// [`prepare_matrix`] with the projected bounds center supplied instead of
/// derived from this section's own bounds alone. Phobos `ShadowIndices=` hands
/// every extra section the same `matrix` and `bnd` as the primary
/// (`Ext/TechnoType/Hooks.MatrixOp.cpp:793-800`), so their stencils share the
/// primary's center.
fn prepare_matrix_at(
    vxl: &VxlFile,
    matrix: super::Mat4,
    shadow_index: i32,
    considered_aircraft: bool,
    center_override: Option<[f32; 2]>,
) -> Option<StencilRaster> {
    let limb_index = usize::try_from(shadow_index).ok()?;
    let limb = vxl.limbs.get(limb_index)?;
    let nx = usize::from(limb.size_x);
    let ny = usize::from(limb.size_y);
    if nx == 0 || ny == 0 {
        return None;
    }
    let (corners, lo, hi) = projected_quad(matrix, limb.bounds, considered_aircraft)?;
    let center = match center_override {
        Some(center) => center,
        None => center_of_bounds(lo, hi)?,
    };
    let mut rect = [0i32; 6];
    let mut origin = [0u16; 2];
    let mut step_x = [0u16; 2];
    let mut step_y = [0u16; 2];
    for axis in 0..2 {
        let extent = ftol(Fpu::sub(load(hi[axis])?, load(lo[axis])?))?;
        rect[axis + 4] = extent.checked_add(8)?;
        rect[axis + 2] = 124 - extent / 2;
        rect[axis] = ftol(load(center[axis])?)? - rect[axis + 4] / 2;
        origin[axis] = ftol(Fpu::mul(
            Fpu::sub(
                Fpu::add(load(corners[2][axis])?, load(128.0)?),
                load(center[axis])?,
            ),
            load(256.0)?,
        ))? as u16;
        step_x[axis] = ftol(Fpu::mul(
            Fpu::div(
                Fpu::sub(load(corners[1][axis])?, load(corners[2][axis])?),
                Fpu::load_i32(nx as i32),
            )
            .ok()?,
            load(256.0)?,
        ))? as u16;
        step_y[axis] = ftol(Fpu::mul(
            Fpu::div(
                Fpu::sub(load(corners[3][axis])?, load(corners[2][axis])?),
                Fpu::load_i32(ny as i32),
            )
            .ok()?,
            load(256.0)?,
        ))? as u16;
    }
    limb.native_spans.as_ref()?;
    let [_, _, x, y, w, h] = rect;
    if x < 0 || y < 0 || w <= 0 || h <= 0 || x + w > 256 || y + h > 256 {
        return None;
    }
    Some(StencilRaster {
        limb: limb_index,
        origin,
        step_x,
        step_y,
        rect,
    })
}

/// The columns 756860 marks, those whose start offset is not -1, as
/// `x | y << 8` in its order. None when an offset cannot be read, which
/// fails its raster.
pub(crate) fn stencil_columns(limb: &super::VxlLimb) -> Option<Vec<u32>> {
    let spans = limb.native_spans.as_ref()?;
    let nx = usize::from(limb.size_x);
    let mut columns = Vec::new();
    for y in 0..usize::from(limb.size_y) {
        for x in 0..nx {
            let p = spans
                .start_offset
                .checked_add((y * nx + x).checked_mul(4)?)?;
            let start = i32::from_le_bytes(spans.body.get(p..p + 4)?.try_into().ok()?);
            if start != -1 {
                columns.push(x as u32 | (y as u32) << 8);
            }
        }
    }
    Some(columns)
}

/// The visibility-map byte 756860 marks first for `column` (`x | y << 8`):
/// the high bytes of its separately wrapped 8.8 Y and X words.
pub(crate) fn stencil_address(raster: &StencilRaster, column: u32) -> u16 {
    let (x, y) = ((column & 0xff) as u16, (column >> 8) as u16);
    let [word_x, word_y]: [u16; 2] = std::array::from_fn(|axis| {
        raster.origin[axis]
            .wrapping_add(raster.step_y[axis].wrapping_mul(y))
            .wrapping_add(raster.step_x[axis].wrapping_mul(x))
    });
    (word_x >> 8) | (word_y & 0xff00)
}

/// `raster`'s stencil: two adjacent bytes at each marked column. None when
/// a column's offset cannot be read or its second byte falls past the
/// buffer.
pub(crate) fn render_raster(vxl: &VxlFile, raster: &StencilRaster) -> Option<VxlSprite> {
    let mut visibility = vec![0u8; 65536];
    for column in stencil_columns(vxl.limbs.get(raster.limb)?)? {
        let address = usize::from(stencil_address(raster, column));
        // The second byte is adjacent, not independently wrapped X.
        *visibility.get_mut(address)? = 1;
        *visibility.get_mut(address + 1)? = 1;
    }
    let [ox, oy, x, y, w, h] = raster.rect;
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for row in y..y + h {
        let p = (row * 256 + x) as usize;
        pixels.extend_from_slice(&visibility[p..p + w as usize]);
    }
    Some(VxlSprite {
        palette_indices: pixels,
        depth: Vec::new(),
        width: w as u32,
        height: h as u32,
        offset_x: ox as f32,
        offset_y: oy as f32,
    })
}

/// [`prepare_matrix`]'s stencil, rasterized.
pub(crate) fn render_matrix(
    vxl: &VxlFile,
    matrix: super::Mat4,
    shadow_index: i32,
    considered_aircraft: bool,
) -> Option<VxlSprite> {
    render_raster(
        vxl,
        &prepare_matrix(vxl, matrix, shadow_index, considered_aircraft)?,
    )
}

/// Phobos `ShadowIndices=`'s listed sections as one stencil
/// (`Ext/TechnoType/Hooks.MatrixOp.cpp:793-800`): every section is handed the
/// same `&mtx`, then `0x7072A1` multiplies that section's own HVA transform
/// into it, so each listed section draws at its own `ShadowIndices.Frame`
/// (`Body.cpp:1327-1337`). Their two-byte columns compose in one 256-square,
/// exactly the way the loop's repeated `DrawVoxelShadow` calls accumulate on
/// the one surface. The crop is the union the composed pixels occupy, and its
/// placement answers the same screen offset the single-section path does.
///
/// The shared anchor is the primary section's frame-0 center: native's
/// `shadowCenter = *pt + ShadowPoint` (`:726-727`) is built before
/// `ChooseFrame`, so a nonzero `ShadowIndex.Frame` never moves it.
pub(crate) fn render_indices(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    params: &VxlRenderParams,
    primary_index: i32,
    indices: &[i32],
    frames: &[i32],
    considered_aircraft: bool,
) -> Option<VxlSprite> {
    let draw = draw_rotation(vxl, params)?;
    // Native anchors every listed section at `shadowCenter = *pt +
    // ShadowPoint` (`Hooks.MatrixOp.cpp:726-727`). Malformed data can leave
    // that point without a primary section; the first listed section's
    // frame-0 center then stands in, instead of dropping the whole list.
    let center = section_matrix(vxl, hva, draw, primary_index, 0)
        .and_then(|matrix| center_of_matrix(vxl, matrix, primary_index, considered_aircraft))
        .or_else(|| {
            indices.iter().find_map(|&index| {
                let matrix = section_matrix(vxl, hva, draw, index, 0)?;
                center_of_matrix(vxl, matrix, index, considered_aircraft)
            })
        })?;
    let mut visibility = vec![0u8; 65536];
    let mut any = false;
    for (slot, &index) in indices.iter().enumerate() {
        // `-1` is the native resize placeholder; `ChooseFrame` then draws the
        // walked frame, which this atlas cannot key and holds at 0.
        let frame = frames.get(slot).copied().unwrap_or(-1);
        let Some(matrix) = section_matrix(vxl, hva, draw, index, frame) else {
            continue;
        };
        let Some(raster) = prepare_matrix_at(vxl, matrix, index, considered_aircraft, Some(center))
        else {
            continue;
        };
        // A section the raster cannot read is skipped like native's loop; it
        // must not discard the sections already composed.
        let Some(limb) = vxl.limbs.get(raster.limb) else {
            continue;
        };
        let Some(columns) = stencil_columns(limb) else {
            continue;
        };
        for column in columns {
            let address = usize::from(stencil_address(&raster, column));
            visibility[address] = 1;
            if address + 1 < visibility.len() {
                // The second byte is adjacent, not independently wrapped X.
                visibility[address + 1] = 1;
            }
            any = true;
        }
    }
    if !any {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (256i32, 256i32, -1i32, -1i32);
    for y in 0..256usize {
        for x in 0..256usize {
            if visibility[y * 256 + x] != 0 {
                x0 = x0.min(x as i32);
                y0 = y0.min(y as i32);
                x1 = x1.max(x as i32);
                y1 = y1.max(y as i32);
            }
        }
    }
    if x1 < x0 || y1 < y0 {
        return None;
    }
    let (w, h) = ((x1 - x0 + 1) as usize, (y1 - y0 + 1) as usize);
    let mut pixels = Vec::with_capacity(w * h);
    for row in y0 as usize..=y1 as usize {
        let p = row * 256 + x0 as usize;
        pixels.extend_from_slice(&visibility[p..p + w]);
    }
    // The 256-square's column 128 is the placement anchor: a pixel at square
    // `x` sits at `center + (x - 128)` on the tactical surface.
    let anchor = [ftol(load(center[0])?)?, ftol(load(center[1])?)?];
    Some(VxlSprite {
        palette_indices: pixels,
        depth: Vec::new(),
        width: w as u32,
        height: h as u32,
        offset_x: (anchor[0] - 128 + x0) as f32,
        offset_y: (anchor[1] - 128 + y0) as f32,
    })
}

/// [`render_matrix`] as an image for the GPU raster.
pub(crate) fn matrix_image(
    vxl: &VxlFile,
    matrix: super::Mat4,
    shadow_index: i32,
    considered_aircraft: bool,
) -> Option<super::VoxelImage<'_>> {
    let raster = prepare_matrix(vxl, matrix, shadow_index, considered_aircraft)?;
    Some(super::VoxelImage::Stencil { vxl, raster })
}

fn projected_quad(
    matrix: super::Mat4,
    bounds: [f32; 6],
    considered_aircraft: bool,
) -> Option<([Vec3; 4], Vec3, Vec3)> {
    let [lx, ly, lz, hx, hy, _hz] = bounds;
    let mut corners = [Vec3::ZERO; 4];
    let mut lo = Vec3::splat(10000.0);
    let mut hi = Vec3::splat(-10000.0);
    // 754C00's original startup light transformation stores 0x403ffff1 at
    // 887420. The rounded decimal 3.0 changes fixed-point column boundaries.
    let light_x = f32::from_bits(0x403f_fff1);
    for (i, point) in [
        Vec3::new(hx, hy, lz),
        Vec3::new(hx, ly, lz),
        Vec3::new(lx, ly, lz),
        Vec3::new(lx, hy, lz),
    ]
    .into_iter()
    .enumerate()
    {
        let mut p = transform_point(matrix, point)?;
        p.y = -p.y;
        p.z = 0.0;
        //707319..70732C scales the viewer. 753F90 applies its light
        //translation afterward; the light itself is not halved. Executed
        //half-viewer controls in aircraft_shadow.json catch this ordering.
        if considered_aircraft {
            p.x = store(Fpu::mul(load(p.x)?, load(0.5)?))?;
            p.y = store(Fpu::mul(load(p.y)?, load(0.5)?))?;
        }
        p.x = store(Fpu::add(load(p.x)?, load(light_x)?))?;
        for axis in 0..2 {
            if p[axis] < lo[axis] {
                lo[axis] = p[axis];
            }
            if p[axis] > hi[axis] {
                hi[axis] = p[axis];
            }
        }
        corners[i] = p;
    }
    Some((corners, lo, hi))
}

/// 756860 truncates the fractional center plus the native caller displacement.
/// `body_offset` here is the effective difference from `body_sample_offset`,
/// not that raw displacement. For the native 754510 crop,
/// crop_origin + trunc(center) == 128 + output_offset.
/// Thus the exact sample for each cropped stencil byte uses this shared
/// Unit-surface coordinate, including the second horizontal pixel.
pub(crate) fn mask_body(
    pixels: &mut [u8],
    width: u32,
    offset: [i32; 2],
    body: &[u8],
    body_offset: [i32; 2],
) -> Option<()> {
    if body.len() != 65536 || width == 0 {
        return None;
    }
    for (i, pixel) in pixels.iter_mut().enumerate() {
        if *pixel == 0 {
            continue;
        }
        let x = 128 + offset[0] + (i % width as usize) as i32 + body_offset[0];
        let y = 128 + offset[1] + (i / width as usize) as i32 + body_offset[1];
        // Native BSurface::GetPixel returns zero outside the surface.
        if (0..256).contains(&x) && (0..256).contains(&y) && body[(y * 256 + x) as usize] != 0 {
            *pixel = 0;
        }
    }
    Some(())
}
