//! House-color LaserDraw::DrawBeamSpecial5509F0. Geometry/color arguments
//! feed the shared DSurface raster/ordered RGB565 destination owner.
//! Native execution: tools/spatial_oracle/building_prism.json::laser_draw.

use crate::sim::projectile::ProjectileCoord;
use crate::util::native_x87::{NativeF32Bits, X87Chop53 as X};

#[derive(Debug, Clone, Copy)]
pub(crate) struct LaserDraw {
    pub from: ProjectileCoord,
    pub to: ProjectileCoord,
    pub z_adjust: i32,
    pub width: i32,
    pub supported: bool,
    pub rgb: [u8; 3],
    pub age: i32,
    pub duration: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaserBlend {
    /// 4BDF00 pre-scales its RGB by the retained float then admits any >7.
    Add([u8; 3]),
    /// 4BFD30 receives an already packed surface word.
    Replace(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LaserLine {
    pub from: [i32; 2],
    pub to: [i32; 2],
    pub z_adjust: [i32; 2],
    pub blend: LaserBlend,
}

impl LaserDraw {
    /// The native f32 store precedes both downstream consumers. Reuse the
    /// existing finite numeric owner rather than an independent approximation.
    pub(crate) fn intensity(self) -> NativeF32Bits {
        X::div(
            X::load_i32(self.duration.wrapping_sub(self.age)),
            X::load_i32(self.duration),
        )
        .and_then(X::store_f32)
        .unwrap_or(NativeF32Bits::POSITIVE_ZERO)
    }

    pub(crate) fn lines(
        self,
        camera: [i32; 2],
        high_detail: bool,
        mut emit: impl FnMut(LaserLine),
    ) {
        // Draw550268 skips nonpositive duration. Expiry belongs to Update;
        // a prepared age == duration object still reaches this draw leaf.
        if self.duration <= 0 {
            return;
        }
        let project = |p: ProjectileCoord| {
            let (x, y) = crate::util::lepton::absolute_leptons_to_screen(p.x, p.y, p.z);
            [x as i32 - camera[0], y as i32 - camera[1]]
        };
        let from = project(self.from);
        let to = project(self.to);
        let z_adjust = [
            self.z_adjust
                .wrapping_sub(crate::util::native_x87::adjust_for_z_standard(self.from.z))
                .wrapping_sub(2),
            (-2i32).wrapping_sub(crate::util::native_x87::adjust_for_z_standard(self.to.z)),
        ];
        let intensity = X::load_f32(self.intensity()).expect("finite laser fade");
        // 550C21 multiplies the retained float by float[7E2220]=255.
        let toward_white = X::ftol_i32_low_masked(X::mul(intensity, X::load_i32(255))) as u8;
        let mut line = |from, to, rgb: [u8; 3]| {
            let blend = if high_detail {
                let scaled = rgb.map(|v| {
                    X::ftol_i32_low_masked(X::mul(X::load_i32(i32::from(v)), intensity)) as u8
                });
                if scaled.iter().all(|&v| v <= 7) {
                    return;
                }
                LaserBlend::Add(scaled)
            } else {
                // RGBClass6612C0: signed division toward zero, low byte of
                // interpolation argument. Here the destination is white.
                let rgb = rgb.map(|v| {
                    (i32::from(v) + (255 - i32::from(v)) * i32::from(toward_white) / 256) as u8
                });
                LaserBlend::Replace(crate::render::native_surface_format::RGB565.pack_rgb8(rgb))
            };
            emit(LaserLine {
                from,
                to,
                z_adjust,
                blend,
            });
        };
        let center = if self.supported {
            self.rgb.map(|v| v.saturating_mul(2))
        } else {
            self.rgb
        };
        let mut side = if self.supported {
            center
        } else {
            self.rgb.map(|v| v >> 1)
        };
        // 550B3B..550B7B shares the existing atan table/facing conversion.
        let facing = crate::util::direction_tables::facing16_between(
            [self.from.x, self.from.y],
            [self.to.x, self.to.y],
        );
        let octant = (((facing >> 12) + 1) >> 1) & 7;
        const OFFSETS: [[i32; 4]; 8] = [
            [-1, -1, 1, 1],
            [0, -1, 0, 1],
            [-1, 1, 1, -1],
            [-1, 0, 1, 0],
            [-1, -1, 1, 1],
            [0, -1, 0, 1],
            [-1, 1, 1, -1],
            [-1, 0, 1, 0],
        ];
        let steps = OFFSETS[usize::from(octant)];
        let mut a = [0i32; 2];
        let mut b = [0i32; 2];
        for width in 1..=self.width {
            if octant & 1 != 0 || width & 1 != 0 {
                a[0] = a[0].wrapping_add(steps[0]);
                b[1] = b[1].wrapping_add(steps[3]);
            }
            if octant & 1 != 0 || width & 1 == 0 {
                a[1] = a[1].wrapping_add(steps[1]);
                b[0] = b[0].wrapping_add(steps[2]);
            }
            let shift =
                |p: [i32; 2], d: [i32; 2]| [p[0].wrapping_add(d[0]), p[1].wrapping_add(d[1])];
            line(shift(from, a), shift(to, a), side);
            line(shift(from, b), shift(to, b), side);
            if self.supported && width == 1 {
                side = self.rgb;
            } else {
                side = side.map(|v| v >> 1);
                if side.iter().all(|&v| v < if high_detail { 8 } else { 64 }) {
                    break;
                }
            }
        }
        line(from, to, center);
    }
}
