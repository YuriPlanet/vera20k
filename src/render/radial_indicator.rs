//! Game::DrawRadialIndicator456980, selected-building arguments456750.
//! This producer emits shared surface operations; it owns no selection, clocks,
//! RNG or simulation state. Original execution: procedural_drawing_oracle/radial_indicator.

use super::surface_line::{SurfaceDraw, SurfaceLine, SurfaceLineBlend};
use crate::util::native_x87::{NativeF64Bits, X87Chop53 as X, X87Value};

#[derive(Debug, Clone, Copy)]
pub(crate) struct RadialIndicatorDraw {
    /// CoordsToClient2 plus tactical origin, in logical surface pixels.
    pub center: [i32; 2],
    pub radius: i32,
    pub pixel_radius: bool,
    pub rgb: [u8; 3],
    pub fade: bool,
    pub rays: bool,
    pub frame: i32,
}

fn real(value: f64) -> X87Value {
    X::load_f64(NativeF64Bits::from_bits(value.to_bits())).expect("finite radial constant")
}

fn root(value: X87Value) -> X87Value {
    X::load_f32(crate::util::native_x87::sqrt_approx_f32(value).expect("finite radial root"))
        .expect("finite retail square root")
}

fn div(a: X87Value, b: X87Value) -> X87Value {
    X::div(a, b).expect("nonzero radial geometry divisor")
}

///4569B9..4569DC. Use the existing numeric owner for the native f32 root
/// and truncation; no fitted screen radius or new x87 implementation.
pub(crate) fn radius_pixels(radius: i32) -> i32 {
    X::ftol_i32_low_masked(X::mul(
        div(X::add(X::load_i32(radius), real(0.5)), root(real(2.))),
        real(60.),
    ))
}

impl RadialIndicatorDraw {
    pub(crate) fn draws(self, clip: [i32; 4], mut emit: impl FnMut(SurfaceDraw)) {
        let rx = if self.pixel_radius {
            self.radius
        } else {
            radius_pixels(self.radius)
        };
        let ry = rx / 2;
        if self.center[0].wrapping_add(rx) <= clip[0]
            || self.center[0].wrapping_sub(rx) >= clip[0] + clip[2]
            || self.center[1].wrapping_add(ry) <= clip[1]
            || self.center[1].wrapping_sub(ry) >= clip[1] + clip[3]
        {
            return;
        }
        let rgb = if self.fade {
            super::native_surface_format::interpolate_rgb8(self.rgb, [0; 3], 50)
        } else {
            self.rgb
        };
        let word = super::native_surface_format::RGB565.pack_rgb8(rgb);
        emit(SurfaceDraw::Ellipse {
            center: self.center,
            radius: [rx, ry],
            word,
        });
        emit(SurfaceDraw::Ellipse {
            center: self.center,
            radius: [rx - 1, ry - 1],
            word,
        });
        if !self.rays {
            return;
        }
        let x2 = X::mul(X::load_i32(rx), X::load_i32(rx));
        let y2 = X::mul(X::load_i32(ry), X::load_i32(ry));
        for (offset, strength) in [0.05, 0.2, 0.4, 1.].into_iter().enumerate() {
            //456C17 uses the binary frame, including its signed DWORD wrap.
            let angle = X::mul(
                X::load_i32(self.frame.wrapping_add(offset as i32)),
                real(0.005),
            );
            let turns = X::ftol_i32_low_masked(X::mul(angle, real(0.15915494309189535)));
            let angle = X::sub(
                angle,
                X::mul(X::load_i32(turns), real(std::f64::consts::TAU)),
            );
            let radians = f64::from_bits(X::store_f64(angle).expect("bounded radial angle").bits());
            let end = if (radians - std::f64::consts::FRAC_PI_2).abs() < 0.001 {
                [self.center[0], self.center[1].wrapping_sub(ry)]
            } else if (radians - 3. * std::f64::consts::FRAC_PI_2).abs() < 0.001 {
                [self.center[0], self.center[1].wrapping_add(ry)]
            } else {
                let tangent = crate::util::native_trig::native_tan_from_table(angle);
                let x = root(div(
                    real(1.),
                    X::add(div(real(1.), x2), div(X::mul(tangent, tangent), y2)),
                ));
                let y = root(X::mul(X::sub(real(1.), div(X::mul(x, x), x2)), y2));
                let x = if radians > std::f64::consts::FRAC_PI_2
                    && radians < 3. * std::f64::consts::FRAC_PI_2
                {
                    X::neg(x)
                } else {
                    x
                };
                let y = if radians < std::f64::consts::PI {
                    X::neg(y)
                } else {
                    y
                };
                [
                    self.center[0].wrapping_add(X::ftol_i32_low_masked(x)),
                    self.center[1].wrapping_add(X::ftol_i32_low_masked(y)),
                ]
            };
            //456DD8 sends the ORIGINAL RGB to4BC750; the50/256 fade is
            //only for the two ellipse words. The double factors store to f32.
            let intensity = X::store_f32(real(strength)).expect("finite radial intensity");
            emit(SurfaceDraw::Line(SurfaceLine {
                from: [self.center[0] - clip[0], self.center[1] - clip[1]],
                to: [end[0] - clip[0], end[1] - clip[1]],
                z_adjust: [-500; 2],
                blend: SurfaceLineBlend::Antialiased {
                    rgb: self.rgb,
                    intensity,
                },
            }));
        }
    }
}

#[cfg(test)]
#[path = "radial_indicator_tests.rs"]
pub(crate) mod tests;
