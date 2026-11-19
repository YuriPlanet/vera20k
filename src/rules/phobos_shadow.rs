//! Phobos' height-scaled voxel shadow (`ShadowSizeCharacteristicHeight=`).
//!
//! Native: `Ext/TechnoType/Hooks.MatrixOp.cpp:677-709` (units) and `:958-973`
//! (aircraft) scale `shadowMatrix` by `Pade2_2(AirShadowBaseScale_log * height
//! / cHeight)`, clamped below by `HeightShadowScaling.MinScale`. The type key is
//! `Ext/TechnoType/Body.cpp:912` (`Nullable<int>`, `Body.h:172`, empty in the
//! constructor at `:567`); the three globals are `Ext/Rules/Body.cpp:182-189`
//! (`HeightShadowScaling`, `HeightShadowScaling.MinScale`,
//! `AirShadowBaseScale` → `-ln(min(v, 1))`, and `AirShadowBaseScale > 0.98`
//! forces `HeightShadowScaling` off).
//!
//! VERA bakes one shadow stencil per (type, facing, slope) atlas key, so a
//! height-dependent scale cannot live in a key without multiplying the atlas.
//! The caller keeps the baked stencil and scales the drawn quad about its
//! anchor instead; the residual is that the stencil is resampled rather than
//! re-rasterized through a scaled matrix (native also invalidates its
//! `VoxelIndexKey` for exactly that re-raster).

use crate::rules::phobos_globals::PhobosGlobals;

fn min_scale(globals: &PhobosGlobals) -> f64 {
    f64::from_bits(globals.height_shadow_scaling_min_scale.bits())
}

fn base_scale_log(globals: &PhobosGlobals) -> f64 {
    f64::from_bits(globals.air_shadow_base_scale_log.bits())
}

/// `Pade2_2` (`Ext/TechnoType/Hooks.MatrixOp.cpp:592-597`): `exp(-x)` as
/// `FastPow(1/e, int(x)) * (12 - 6s + s^2) / (12 + 6s + s^2)`.
/// `FastPow` is binary exponentiation (`src/Utilities/GeneralUtils.h:47`).
/// Phobos only reaches it with `x >= 0` (`-ln(min(v,1)) >= 0` times a
/// non-negative height ratio), where `static_cast<int>` truncates like the
/// native float-to-int conversion.
pub fn pade_exp_neg(x: f64) -> f64 {
    let whole = x as i32;
    let frac = x - f64::from(whole);
    fast_pow(0.367_879_441_171_442_33, whole) * (12.0 - 6.0 * frac + frac * frac)
        / (12.0 + 6.0 * frac + frac * frac)
}

fn fast_pow(base: f64, exponent: i32) -> f64 {
    let mut result = 1.0;
    let mut base_power = base;
    let mut n = exponent.max(0) as u32;
    while n != 0 {
        if n & 1 != 0 {
            result *= base_power;
        }
        base_power *= base_power;
        n >>= 1;
    }
    result
}

/// `Ext/TechnoType/Hooks.MatrixOp.cpp:677-709`: the scale a *unit* shadow uses.
/// `characteristic_height` is the type's key (unset → the jumpjet's own
/// `Height=` for a Jumpjet, `[AudioVisual] CruiseHeight=` otherwise).
pub fn unit_size_scale(
    globals: &PhobosGlobals,
    height: i32,
    jumpjet_height: Option<i32>,
    characteristic_height: Option<i32>,
    cruise_height: i32,
    considered_aircraft: bool,
) -> f64 {
    if globals.height_shadow_scaling && height > 0 {
        let floor = min_scale(globals);
        let (c_height, scaled_height) = match jumpjet_height {
            Some(loco_height) => (
                characteristic_height.unwrap_or(loco_height),
                f64::from(height),
            ),
            // `:697`: a non-Jumpjet only scales once it is above 208 leptons,
            // and measures from that threshold.
            None => {
                if height <= 208 {
                    return 1.0;
                }
                (
                    characteristic_height.unwrap_or(cruise_height),
                    f64::from(height - 208),
                )
            }
        };
        if c_height > 0 {
            return pade_exp_neg(base_scale_log(globals) * scaled_height / f64::from(c_height))
                .max(floor);
        }
        1.0
    } else if !globals.height_shadow_scaling && considered_aircraft {
        // `:705-708`: with the feature off, an aircraft-considered type still
        // takes the base scale.
        pade_exp_neg(base_scale_log(globals))
    } else {
        1.0
    }
}

/// `Ext/TechnoType/Hooks.MatrixOp.cpp:958-973`: the scale an aircraft's shadow
/// uses. `characteristic_height` unset falls back to the type's
/// `GetFlightLevel()`.
pub fn aircraft_size_scale(
    globals: &PhobosGlobals,
    height: i32,
    flight_level: i32,
    characteristic_height: Option<i32>,
    considered_aircraft: bool,
) -> f64 {
    if globals.height_shadow_scaling {
        let c_height = characteristic_height.unwrap_or(flight_level);
        if c_height > 0 {
            return pade_exp_neg(base_scale_log(globals) * f64::from(height) / f64::from(c_height))
                .max(min_scale(globals));
        }
        1.0
    } else if considered_aircraft {
        pade_exp_neg(base_scale_log(globals))
    } else {
        1.0
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_shadow_tests.rs"]
mod tests;
