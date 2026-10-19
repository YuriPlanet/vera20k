//! LaserDraw::DrawBeam550260 and house-color DrawBeamSpecial5509F0.
//! Geometry/color feed the shared DSurface raster/ordered RGB565 destination.
//! Native execution: tools/spatial_oracle/{building_prism,disk_laser}.json::laser_draw.

use super::surface_line::{SurfaceLine, SurfaceLineBlend};
use crate::sim::projectile::ProjectileCoord;
use crate::util::native_x87::{NativeF32Bits, X87Chop53 as X};

/// Copied House RGB, or GeneralDraw's per-composite resolved colors. A
/// missing outer color means the native base outer RGB was entirely zero;
/// a sampled Some([0; 3]) still selects the ordinary outer-line arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LaserDrawColor {
    House([u8; 3]),
    Explicit {
        inner: [u8; 3],
        outer: Option<[u8; 3]>,
    },
}

impl LaserDrawColor {
    pub(crate) fn inner(self) -> [u8; 3] {
        match self {
            Self::House(rgb) | Self::Explicit { inner: rgb, .. } => rgb,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LaserDraw {
    pub from: ProjectileCoord,
    pub to: ProjectileCoord,
    pub z_adjust: i32,
    pub width: i32,
    pub supported: bool,
    pub color: LaserDrawColor,
    pub age: i32,
    pub duration: i32,
    pub fade_end: NativeF32Bits,
}

impl LaserDraw {
    /// The native f32 store precedes both downstream consumers. Reuse the
    /// existing finite numeric owner rather than an independent approximation.
    pub(crate) fn intensity(self) -> NativeF32Bits {
        let end = X::load_f32(self.fade_end).expect("finite laser fade endpoint");
        X::div(
            X::mul(
                X::sub(X::load_i32(1), end),
                X::load_i32(self.duration.wrapping_sub(self.age)),
            ),
            X::load_i32(self.duration),
        )
        .map(|value| X::add(value, end))
        .and_then(X::store_f32)
        .unwrap_or(NativeF32Bits::POSITIVE_ZERO)
    }

    pub(crate) fn lines(
        self,
        camera: [i32; 2],
        high_detail: bool,
        mut emit: impl FnMut(SurfaceLine),
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
                SurfaceLineBlend::Add(scaled)
            } else {
                // General55088B packs the supplied RGB directly. Only
                // Special550C2C interpolates toward white at reduced detail.
                let rgb = if matches!(self.color, LaserDrawColor::House(_)) {
                    crate::render::native_surface_format::interpolate_rgb8(
                        rgb,
                        [255; 3],
                        u32::from(toward_white),
                    )
                } else {
                    rgb
                };
                SurfaceLineBlend::Replace(
                    crate::render::native_surface_format::RGB565.pack_rgb8(rgb),
                )
            };
            emit(SurfaceLine {
                from,
                to,
                z_adjust,
                blend,
            });
        };
        // General5503B9..55041C and Special550B3B..550B7B use the
        // existing atan table/facing conversion, then distinct offset tables.
        let facing = crate::util::direction_tables::facing16_between(
            [self.from.x, self.from.y],
            [self.to.x, self.to.y],
        );
        let octant = crate::util::direction_tables::dir_from_facing16(facing);
        let shift = |p: [i32; 2], d: [i32; 2]| [p[0].wrapping_add(d[0]), p[1].wrapping_add(d[1])];
        if let LaserDrawColor::Explicit { inner, outer } = self.color {
            // Original lazy initializer550295..5503A6, ABC738/128B.
            // General always uses one fixed pair; width/support belong to
            // the separate Special arm and do not widen this beam.
            const OFFSETS: [[i32; 4]; 8] = [
                [0, -1, 1, 0],
                [0, -1, 0, 1],
                [1, 0, 0, 1],
                [-1, 0, 1, 0],
                [-1, 0, 0, 1],
                [0, -1, 1, 0],
                [0, -1, -1, 0],
                [-1, 0, 1, 0],
            ];
            if high_detail && outer.is_none() {
                // RESIDUAL: General outerRGB[0,0,0] selects DSurface4BC750
                // with per-channel masks and interpolated Z. Those shared
                // AA branches remain unported; stock frequency is unknown.
                // Missing AA pixels affect generic laser/ray coverage, not
                // this stock DiskLaser's nonzero80/0/88 outer-color arm.
                return;
            }
            // General detail0 draws center/A/B without fade; detail!=0
            // draws A/B/center through4BDF00 with the same retained fade.
            if !high_detail {
                line(from, to, inner);
            }
            if let Some(outer) = outer {
                let [ax, ay, bx, by] = OFFSETS[usize::from(octant)];
                line(shift(from, [ax, ay]), shift(to, [ax, ay]), outer);
                line(shift(from, [bx, by]), shift(to, [bx, by]), outer);
            }
            if high_detail {
                line(from, to, inner);
            }
            return;
        }
        let rgb = self.color.inner();
        let center = if self.supported {
            rgb.map(|v| v.saturating_mul(2))
        } else {
            rgb
        };
        let mut side = if self.supported {
            center
        } else {
            rgb.map(|v| v >> 1)
        };
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
            line(shift(from, a), shift(to, a), side);
            line(shift(from, b), shift(to, b), side);
            if self.supported && width == 1 {
                side = rgb;
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
