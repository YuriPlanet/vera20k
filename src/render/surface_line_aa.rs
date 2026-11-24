//! DSurface4BC750's constant-adjustment, RGB-enabled, no-WriteZ path.
//! The selected radial caller456DD8 uses precisely these arguments. The
//! explicit-color Laser caller and other parameter branches remain open;
//! extend this shared owner when those callers are admitted.

use crate::util::native_x87::NativeF32Bits;

/// Original lazy initializer4BC780..4BC7C0 under CW0E7F. The257-entry
/// table is NOT symmetric. Kept as executed bytes, not a host pow fit.
pub(crate) const CUBIC_TABLE: &[u8; 257 * 4] = include_bytes!("surface_line_cubic_table.bin");

fn weight(index: i32) -> i32 {
    let offset = index as usize * 4;
    i32::from_le_bytes(CUBIC_TABLE[offset..offset + 4].try_into().unwrap())
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AntialiasedPixel {
    pub point: [i32; 2],
    ///4BD646/68F read both words before either store. The X-major arm
    ///4BD85B/8C0 blends each output from the OTHER old word; the Y-major
    ///arm4BDAFD/4BDB46 uses its own. Preserve this native asymmetry.
    pub destination_source: [i32; 2],
    /// Both paired stores use this primary Z/A sample, even when the other
    /// output pixel's Z/A would reject it. Z is immutable for this caller.
    pub primary: [i32; 2],
    pub z: u16,
    pub source_weight: i32,
    pub destination_weight: i32,
}

pub(crate) fn antialiased_line(
    from: [i32; 2],
    to: [i32; 2],
    z_adjust: i32,
    intensity: NativeF32Bits,
    clip: [i32; 4],
    size: [i32; 2],
    z_origin_y: i32,
    mut emit: impl FnMut([AntialiasedPixel; 2]),
) {
    //4BC7D1..4BC831 intersects the caller clip with the surface inset by1,
    //then adds the INTERSECTION origin to both relative endpoints.
    let left = clip[0].max(1);
    let top = clip[1].max(1);
    let right = (clip[0] + clip[2]).min(size[0] - 1);
    let bottom = (clip[1] + clip[3]).min(size[1] - 1);
    let clip = [left, top, right - left, bottom - top];
    let Some((a, b, _, _)) = super::clipped_z_line(from, to, [z_adjust; 2], clip) else {
        return;
    };
    let dx = b[0] - a[0];
    let dy = (b[1] - a[1]).abs();
    let y_step = if b[1] < a[1] { -1 } else { 1 };
    let horizontal = dx > dy;
    let (major, minor) = if horizontal { (dx, dy) } else { (dy, dx) };
    if major == 0 {
        return;
    }
    //4BD581/4BDA55: ftol(minor/major*256). Positive integer division is
    //equivalent here, including exact binary fractions; no second FP owner.
    let step = (i64::from(minor) * 256 / i64::from(major)) as i32;
    let mut coverage = [256, 0];
    let mut point = a;
    let intensity = f64::from(f32::from_bits(intensity.bits()));
    for _ in 0..major {
        let z = (super::super::native_z::DEFAULT_Z.wrapping_add(z_origin_y) as u16 as i32)
            .wrapping_sub(point[1])
            .wrapping_sub(clip[1])
            .wrapping_add(z_adjust) as u16;
        let neighbor = if horizontal {
            [point[0], point[1] + y_step]
        } else {
            [point[0] + 1, point[1]]
        };
        let outputs = [point, neighbor];
        let pair = std::array::from_fn(|i| {
            let output = outputs[i];
            let amount = coverage[i];
            //f32*integer<=256 is exact in binary64. ftol still truncates.
            let k = (f64::from(amount) * intensity) as i32;
            AntialiasedPixel {
                point: output,
                destination_source: outputs[if horizontal { 1 - i } else { i }],
                primary: point,
                z,
                source_weight: weight(k),
                destination_weight: weight(256 - k),
            }
        });
        emit(pair);
        coverage[0] -= step;
        coverage[1] += step;
        if coverage[1] > 256 {
            coverage[0] += 256;
            coverage[1] -= 256;
            if horizontal {
                point[1] += y_step;
            } else {
                point[0] += 1;
            }
        }
        if horizontal {
            point[0] += 1;
        } else {
            point[1] += y_step;
        }
    }
}
