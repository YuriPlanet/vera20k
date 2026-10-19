//! EBolt subdivision4C1F20 and the manager4C2830 projection/clip gate.
//!
//! Geometry consumes the retained process Main capability once per actual tactical
//! visit. Lifetime, registry order and scene cleanup belong to app presentation;
//! this producer returns immutable DSurface4BFD30 lines to the shared compositor.
//! Native comparisons: tools/procedural_drawing_oracle/electric_bolt.json.

use super::surface_line::{SurfaceLine, SurfaceLineBlend, SurfaceLineViewport, clip_line};
use crate::sim::projectile::ProjectileCoord;
use crate::sim::rng::MainRngDraws;
use crate::util::native_x87::{NativeF64Bits, X87Chop53 as X, adjust_for_z_standard};

/// Native Convert+174 words for PALETTE.PAL indices10,5,15. The caller
/// resolves the current converter once; packed colors never round-trip via RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ElectricBoltPalette {
    pub ordinary: u16,
    pub alternate: u16,
    pub center: u16,
}

impl ElectricBoltPalette {
    /// FileSystem52BE61..52BFCE installs PALETTE.PAL's plain N53 converter.
    /// Bolt4C24BE/4C26C7 reads its middle row directly, like action lines.
    pub(crate) fn from_palette(palette: &crate::assets::pal_file::Palette) -> Self {
        let color = |index: u8| {
            let color = palette.color(index);
            super::palette_light::PaletteLight::plain(53, 1000).rgb565(
                [color.r, color.g, color.b],
                index,
                127,
            )
        };
        Self {
            ordinary: color(10),
            alternate: color(5),
            center: color(15),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ElectricBoltDraw {
    pub from: ProjectileCoord,
    pub to: ProjectileCoord,
    pub z_adjust: i32,
    pub phase: i32,
    pub alternate_color: bool,
}

/// Pending second half of a split. Each triplet stores center, side1, side2;
/// subdivision uses one stack of at most8 entries, independent of bolt length.
#[derive(Clone, Copy, Default)]
struct Piece {
    from: [[i32; 3]; 3],
    to: [[i32; 3]; 3],
    length: i32,
    displacement: i32,
    z_adjust: [i32; 2],
}

impl ElectricBoltDraw {
    /// Reports only manager clip admission. Rejection and zero-length draws
    /// still age in the registry; clipped screen endpoints never replace raw
    /// coordinates passed to the subdivision body.
    pub(crate) fn lines(
        self,
        viewport: SurfaceLineViewport,
        palette: ElectricBoltPalette,
        main: &mut MainRngDraws,
        mut emit: impl FnMut(SurfaceLine),
    ) -> bool {
        let from = [self.from.x, self.from.y, self.from.z];
        let to = [self.to.x, self.to.y, self.to.z];
        let project = |[x, y, z]: [i32; 3]| {
            let (x, y) = crate::util::lepton::absolute_leptons_to_screen(x, y, z);
            [
                (x as i32).wrapping_sub(viewport.camera[0]),
                (y as i32).wrapping_sub(viewport.camera[1]),
            ]
        };
        let mut clipped_from = project(from);
        let mut clipped_to = project(to);
        if viewport.clip[2] <= 0
            || viewport.clip[3] <= 0
            || !clip_line(&mut clipped_from, &mut clipped_to, viewport.clip)
        {
            return false;
        }

        // 4C1FAD..4C1FDE sums x²+z²+y² and uses Sqrt_Approx4CAC40.
        // The existing numeric/table owner preserves that order and conversion.
        let delta: [i32; 3] = std::array::from_fn(|i| from[i].wrapping_sub(to[i]));
        let length = crate::util::native_x87::sqrt_approx_length([delta[0], delta[2], delta[1]]);
        if length == 0 {
            return true;
        }
        let side_threshold = length.wrapping_mul(102) >> 8;
        let mut current = Piece {
            from: [from; 3],
            to: [to; 3],
            length,
            displacement: length.wrapping_mul(23) >> 8,
            z_adjust: [self.z_adjust, 0],
        };
        let mut pending = [Piece::default(); 8];
        let mut depth = 0;
        let mut first_split = true;
        let outer_color = if self.alternate_color {
            palette.alternate
        } else {
            palette.ordinary
        };
        loop {
            while current.length > 64 && depth < pending.len() {
                // The original uses ADD/SAR, including negative odd sums and
                // signed32 overflow, for all three midpoint coordinates.
                let mut middle: [[i32; 3]; 3] = std::array::from_fn(|strand| {
                    std::array::from_fn(|axis| {
                        current.from[strand][axis].wrapping_add(current.to[strand][axis]) >> 1
                    })
                });
                if first_split {
                    first_split = false;
                    let trig = crate::util::retail_trig::TrigTable::embedded();
                    // PI at7E44D0. Native4C2151 multiplies the returned table
                    // sine by initial displacement before Math__ftol; the
                    // decompile omits that x87 value's dependency.
                    let pi = X::load_f64(NativeF64Bits::from_bits(0x4009_21fb_5444_2d18))
                        .expect("finite original pi");
                    let wave: [i32; 6] = std::array::from_fn(|i| {
                        let angle = X::div(
                            X::mul(X::load_i32(self.phase), pi),
                            X::load_i32(i as i32 + 7),
                        )
                        .expect("positive bolt wave divisor");
                        X::ftol_i32_low_masked(X::mul(
                            trig.sin_from_table(angle),
                            X::load_i32(current.displacement),
                        ))
                    });
                    let shift = [
                        wave[3].wrapping_add(wave[0]),
                        wave[5].wrapping_add(wave[1]),
                        wave[4]
                            .wrapping_add(current.displacement.wrapping_mul(2))
                            .wrapping_add(wave[2])
                            >> 1,
                    ];
                    for point in &mut middle {
                        for axis in 0..3 {
                            point[axis] = point[axis].wrapping_add(shift[axis]);
                        }
                    }
                }
                // Every split makes nine inclusive requests from process
                // Main. Rejection sampling remains solely in its RNG owner.
                for coord in &mut middle[0] {
                    let shift = if current.length <= 128 {
                        main.ranged(-1, 1)
                            .wrapping_mul(current.displacement)
                            .wrapping_mul(2)
                    } else {
                        main.ranged(current.displacement.wrapping_neg(), current.displacement)
                    };
                    *coord = coord.wrapping_add(shift);
                }
                let center = middle[0];
                for point in &mut middle[1..3] {
                    for (coord, center_coord) in point.iter_mut().zip(center) {
                        let shift =
                            main.ranged(current.displacement.wrapping_neg(), current.displacement);
                        *coord = if current.length > side_threshold {
                            center_coord.wrapping_add(shift >> 1)
                        } else {
                            coord.wrapping_add(shift)
                        };
                    }
                }
                let middle_z = current.z_adjust[0].wrapping_add(current.z_adjust[1]) >> 1;
                current.length >>= 1;
                current.displacement >>= 1;
                pending[depth] = Piece {
                    from: middle,
                    z_adjust: [middle_z, current.z_adjust[1]],
                    ..current
                };
                depth += 1;
                current.to = middle;
                current.z_adjust[1] = middle_z;
            }
            // 4C2512,4C2627,4C270D: both outer strands precede the center.
            for strand in [1, 2, 0] {
                let from = current.from[strand];
                let to = current.to[strand];
                emit(SurfaceLine {
                    from: project(from),
                    to: project(to),
                    z_adjust: [
                        current.z_adjust[0]
                            .wrapping_sub(adjust_for_z_standard(from[2]))
                            .wrapping_sub(2),
                        current.z_adjust[1]
                            .wrapping_sub(adjust_for_z_standard(to[2]))
                            .wrapping_sub(2),
                    ],
                    blend: SurfaceLineBlend::Replace(if strand == 0 {
                        palette.center
                    } else {
                        outer_color
                    }),
                });
            }
            if depth == 0 {
                break;
            }
            depth -= 1;
            current = pending[depth];
        }
        true
    }
}
