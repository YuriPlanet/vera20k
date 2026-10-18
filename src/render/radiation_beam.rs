//! RadBeam straight drawing659650/659AC0. Coordinates are world leptons;
//! the shared surface-line owner performs packed RGB565/A/Z stores.
//! Original numeric/geometry controls: procedural_drawing_oracle/rad_beam.

use super::surface_line::{SurfaceLine, SurfaceLineBlend, SurfaceLineViewport};
use crate::map::retail_trig::TrigTable;
use crate::sim::combat::radiation_beam::RadiationBeamBirth;
use crate::sim::projectile::ProjectileCoord;
use crate::util::native_x87::{
    MaskedX87Chop53 as M, MaskedX87Value, NativeF64Bits, X87Chop53 as X,
};

/// Derived once on the first draw. Copied mode0/1 endpoints never change, so
/// this cache needs no invalidation before its owning registry entry retires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BeamGeometry {
    delta: [i32; 3],
    length: NativeF64Bits,
}

impl BeamGeometry {
    ///65966A..6596D4: low32 squares/sums precede the signed FILD. This is
    /// not the shared wide-square distance routine. Sqrt_Approx is reused.
    pub(crate) fn new(from: ProjectileCoord, to: ProjectileCoord) -> Self {
        let delta = [
            to.x.wrapping_sub(from.x),
            to.y.wrapping_sub(from.y),
            to.z.wrapping_sub(from.z),
        ];
        let squared = delta
            .into_iter()
            .fold(0_i32, |sum, v| sum.wrapping_add(v.wrapping_mul(v)));
        let root = crate::util::native_x87::sqrt_approx_f32(X::load_i32(squared))
            .expect("signed dword Sqrt_Approx input is finite");
        let length = X::store_f64(X::load_f32(root).expect("Sqrt_Approx table is finite"))
            .expect("finite length spill");
        Self { delta, length }
    }

    pub(crate) fn length(self) -> NativeF64Bits {
        self.length
    }

    pub(crate) fn lines(
        self,
        birth: RadiationBeamBirth,
        progress: i32,
        spacing: i32,
        viewport: SurfaceLineViewport,
        mut emit: impl FnMut(SurfaceLine),
    ) {
        let frame = StraightFrame::new(self, birth, progress, spacing);
        if M::compare(M::load_f64(self.length), M::load_i32(0))
            != crate::util::native_x87::MaskedX87Ordering::Greater
        {
            return;
        }
        let project = |p: ProjectileCoord| {
            let (x, y) = crate::util::lepton::absolute_leptons_to_screen(p.x, p.y, p.z);
            [
                (x as i32).wrapping_sub(viewport.camera[0]),
                (y as i32).wrapping_sub(viewport.camera[1]),
            ]
        };
        let mut index = 0_i32;
        loop {
            let segment = frame.segment(index);
            let mut from = project(segment.from);
            let mut to = project(segment.to);
            //6598EC clips XY before passing the ORIGINAL world-endpoint Z
            //arguments to4BFD30. The shared raster's second clip is inert.
            if super::surface_line::clip_line(&mut from, &mut to, viewport.clip) {
                emit(SurfaceLine {
                    from,
                    to,
                    z_adjust: [
                        birth
                            .z_adjust
                            .wrapping_sub(crate::util::native_x87::adjust_for_z_standard(
                                segment.from.z,
                            ))
                            .wrapping_sub(2),
                        (-2_i32).wrapping_sub(crate::util::native_x87::adjust_for_z_standard(
                            segment.to.z,
                        )),
                    ],
                    blend: SurfaceLineBlend::Replace(super::tactical_compat::pack_rgb(
                        segment.color.map(i32::from),
                        super::native_surface_format::RGB565,
                    )),
                });
            }
            index = index.wrapping_add(1);
            //659953: ceil iterations use the FLOOR divisor. The last
            //segment can extend beyond the copied target; don't fit it.
            if M::compare(
                M::mul(M::load_i32(index), M::load_i32(spacing)),
                M::load_f64(self.length),
            ) != crate::util::native_x87::MaskedX87Ordering::Less
            {
                break;
            }
        }
    }
}

///65920D..6592CB: unsigned Logic-rate numerator, signed requested rate.
/// The reciprocal spills before the multiply10. Masked qword ftol narrows
/// to its low32 bits, including0 for a nonfinite integer indefinite.
pub(crate) fn segment_spacing(frame_rate: u32, requested_fps: i32) -> i32 {
    let current = M::load_f64(NativeF64Bits::from_bits(f64::from(frame_rate).to_bits()));
    let ratio = M::div(current, M::load_i32(requested_fps));
    let cube = M::mul(M::mul(ratio, ratio), ratio);
    let reciprocal = M::load_f64(M::store_f64_masked_chop(M::div(M::load_i32(1), cube)));
    M::ftol_i32_low_masked(M::mul(reciprocal, M::load_i32(10))).clamp(10, 100)
}

#[derive(Debug, Clone, Copy)]
struct BeamSegment {
    from: ProjectileCoord,
    to: ProjectileCoord,
    color: [u8; 3],
}

struct StraightFrame {
    birth: RadiationBeamBirth,
    count: i32,
    step: [i32; 3],
    amplitude: MaskedX87Value,
    span: MaskedX87Value,
}

impl StraightFrame {
    fn new(geometry: BeamGeometry, birth: RadiationBeamBirth, progress: i32, spacing: i32) -> Self {
        let length = M::load_f64(geometry.length);
        let count = M::ftol_i32_low_masked(M::div(length, M::load_i32(spacing)));
        let step = divide_coordinate(geometry.delta, count);
        let amplitude = M::load_f64(M::store_f64_masked_chop(M::mul(
            M::div(M::load_i32(progress), M::load_i32(birth.duration)),
            M::load_f64(birth.amplitude),
        )));
        let span = if M::compare(length, M::load_i32(360))
            == crate::util::native_x87::MaskedX87Ordering::Less
        {
            M::load_i32(360)
        } else {
            length
        };
        let span = M::load_f64(M::store_f64_masked_chop(M::add(span, span)));
        Self {
            birth,
            count,
            step,
            amplitude,
            span,
        }
    }

    /// Original ComputeStraight659AC0. All exceptional/truncation behavior
    /// stays with the existing masked math and one shared trig-table owner.
    fn segment(&self, index: i32) -> BeamSegment {
        let endpoint = |index: i32| {
            let ratio = M::div(M::load_i32(index), M::load_i32(self.count));
            let radians = M::mul(
                M::load_f64(NativeF64Bits::from_bits(0x3f91_df46_a252_9d39)),
                M::mul(self.span, ratio),
            );
            let radians = M::load_f64(M::store_f64_masked_chop(radians));
            let sine = TrigTable::embedded().sin_from_table(radians);
            let displacement = M::mul(sine.into(), self.amplitude);
            let coord = ProjectileCoord::new(
                self.birth
                    .from
                    .x
                    .wrapping_add(self.step[0].wrapping_mul(index)),
                self.birth
                    .from
                    .y
                    .wrapping_add(self.step[1].wrapping_mul(index)),
                M::ftol_i32_low_masked(M::add(
                    M::load_i32(
                        self.birth
                            .from
                            .z
                            .wrapping_add(self.step[2].wrapping_mul(index)),
                    ),
                    displacement,
                )),
            );
            (coord, displacement)
        };
        let (from, _) = endpoint(index);
        let (to, displacement) = endpoint(index.wrapping_add(1));
        let half = M::load_f64(NativeF64Bits::HALF);
        let strength = M::add(
            M::div(
                M::mul(
                    M::load_i32(M::ftol_i32_low_masked(displacement).wrapping_abs()),
                    half,
                ),
                M::load_f64(self.birth.amplitude),
            ),
            half,
        );
        let color = self
            .birth
            .color
            .map(|v| M::ftol_i32_low_masked(M::mul(M::load_i32(i32::from(v)), strength)) as u8);
        BeamSegment { from, to, color }
    }
}

/// CoordDivide65A1B0. Its zero-divisor branch preserves the coordinate;
/// every production straight count is nonnegative, so MIN/-1 cannot occur.
fn divide_coordinate(value: [i32; 3], count: i32) -> [i32; 3] {
    if count == 0 {
        value
    } else {
        value.map(|v| v / count)
    }
}

#[cfg(test)]
#[path = "radiation_beam_tests.rs"]
pub(crate) mod tests;
