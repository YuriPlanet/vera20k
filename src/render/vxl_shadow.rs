//! Ordinary single-section, flat ground shadow geometry.
//!
//! Original 753F90 submits four bottom-face corners; 756860 walks the span
//! start table in 8.8 coordinates and writes TWO horizontal stencil pixels.
//! This is separate from the opaque VXL encoded-Z painter and from final
//! destination darkening. The 256-square composed Unit surface masks the
//! first cache fill; the resulting stencil is then retained by its owner.

use super::native::{ftol, load, matrix_product, store, transform_point};
use super::{HvaFile, Vec3, VxlFile, VxlRenderParams, VxlSprite};
use crate::util::native_x87::X87Chop53 as Fpu;

pub(crate) fn supported(vxl: &VxlFile, params: &VxlRenderParams) -> bool {
    params.scale == 1.0
        && params.frame == 0
        && params.slope_type == 0
        && params.slope_blend.is_none()
        && vxl.limbs.len() == 1
        && vxl.limbs[0].native_spans.is_some()
}

#[cfg(test)]
#[path = "vxl_shadow_tests.rs"]
mod tests;

/// Native clear visibility buffer and crop. HVA ShadowIndex/frame zero and
/// ordinary scale-one viewer are the supported caller boundary. Other callers
/// continue through the explicitly retained legacy shadow path.
pub(crate) fn render(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    params: &VxlRenderParams,
) -> Option<VxlSprite> {
    if !supported(vxl, params) {
        return None;
    }
    let limb = vxl.limbs.first()?;
    let draw =
        super::voxel_draw_rotation_for_state(0, None, super::voxel_facing_step(params.facing));
    let matrix = if let Some(hva) = hva {
        let mut raw = *hva.get_transform(0, 0)?;
        for column in [3, 7, 11] {
            raw[column] = store(Fpu::mul(load(raw[column])?, load(limb.scale)?))?;
        }
        matrix_product(draw, super::hva_to_mat4(&raw, 1.0))?
    } else {
        draw
    };
    render_matrix(vxl, matrix)
}

/// The stencil of a single-section VXL at a supplied matrix.
/// `VoxelAnimClass::DrawIt` passes 753F90 the bounce body's drawing matrix
/// directly, with the identity viewer and no HVA (`0x00749CE8`).
pub(crate) fn render_matrix(vxl: &VxlFile, matrix: super::Mat4) -> Option<VxlSprite> {
    if vxl.limbs.len() != 1 || vxl.limbs[0].native_spans.is_none() {
        return None;
    }
    let limb = &vxl.limbs[0];
    let nx = usize::from(limb.size_x);
    let ny = usize::from(limb.size_y);
    if nx == 0 || ny == 0 {
        return None;
    }
    let [lx, ly, lz, hx, hy, _hz] = limb.bounds;
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
        p.x = store(Fpu::add(load(p.x)?, load(light_x)?))?;
        p.z = 0.0;
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
    let mut center = [0.0; 2];
    let mut rect = [0i32; 6];
    let mut origin = [0u16; 2];
    let mut step_x = [0u16; 2];
    let mut step_y = [0u16; 2];
    for axis in 0..2 {
        center[axis] = store(Fpu::mul(
            Fpu::add(load(lo[axis])?, load(hi[axis])?),
            load(0.5)?,
        ))?;
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
    let spans = limb.native_spans.as_ref()?;
    let mut visibility = vec![0u8; 65536];
    let mut row = origin;
    for y in 0..ny {
        let mut xy = row;
        for x in 0..nx {
            let p = spans
                .start_offset
                .checked_add((y * nx + x).checked_mul(4)?)?;
            let start = i32::from_le_bytes(spans.body.get(p..p + 4)?.try_into().ok()?);
            if start != -1 {
                let address = usize::from(xy[0] >> 8) | usize::from(xy[1] & 0xff00);
                // Original second byte is adjacent, not an independently
                // wrapped X coordinate. Reject out-of-surface malformed art.
                *visibility.get_mut(address)? = 1;
                *visibility.get_mut(address + 1)? = 1;
            }
            xy[0] = xy[0].wrapping_add(step_x[0]);
            xy[1] = xy[1].wrapping_add(step_x[1]);
        }
        row[0] = row[0].wrapping_add(step_y[0]);
        row[1] = row[1].wrapping_add(step_y[1]);
    }
    let [ox, oy, x, y, w, h] = rect;
    if x < 0 || y < 0 || w <= 0 || h <= 0 || x + w > 256 || y + h > 256 {
        return None;
    }
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

/// 756860's sample origin is trunc(center)+caller origin. For the native
/// 754510 crop, crop_origin + trunc(center) == 128 + output_offset.
/// Thus the exact sample for each cropped stencil byte uses this shared
/// Unit-surface coordinate, including the second horizontal pixel.
pub(crate) fn mask_body(
    pixels: &mut [u8],
    width: u32,
    offset: [i32; 2],
    body: &[u8],
) -> Option<()> {
    if body.len() != 65536 || width == 0 {
        return None;
    }
    for (i, pixel) in pixels.iter_mut().enumerate() {
        if *pixel == 0 {
            continue;
        }
        let x = 128 + offset[0] + (i % width as usize) as i32;
        let y = 128 + offset[1] + (i / width as usize) as i32;
        // Native BSurface::GetPixel returns zero outside the surface.
        if (0..256).contains(&x) && (0..256).contains(&y) && body[(y * 256 + x) as usize] != 0 {
            *pixel = 0;
        }
    }
    Some(())
}
