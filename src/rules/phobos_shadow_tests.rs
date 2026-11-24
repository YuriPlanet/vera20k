//! `ShadowSizeCharacteristicHeight=`'s numeric kernel and its three branches.

use super::{aircraft_size_scale, pade_exp_neg, unit_size_scale};
use crate::rules::phobos_globals::PhobosGlobals;
use crate::util::native_x87::NativeF64Bits;

fn bits(value: f64) -> NativeF64Bits {
    NativeF64Bits::from_bits(value.to_bits())
}

fn globals(scaling: bool) -> PhobosGlobals {
    PhobosGlobals {
        height_shadow_scaling: scaling,
        ..PhobosGlobals::default()
    }
}

/// `Pade2_2` (`Ext/TechnoType/Hooks.MatrixOp.cpp:592`): the integer part is
/// `FastPow(1/e, n)` and the fraction the (2,2) Pade of `e^-x`. Zero and a
/// whole exponent are exact; a fraction sits inside the approximant's error.
#[test]
fn pade_2_2_measures_exp_neg() {
    assert_eq!(pade_exp_neg(0.0), 1.0);
    assert!((pade_exp_neg(1.0) - (-1.0f64).exp()).abs() < 1e-3);
    assert!((pade_exp_neg(2.0) - (-2.0f64).exp()).abs() < 1e-3);
    // The Pade fraction approximation, not `exp`, for a non-integer input.
    let x = 0.5;
    let pade = (12.0 - 6.0 * 0.5 + 0.25) / (12.0 + 6.0 * 0.5 + 0.25);
    assert!((pade_exp_neg(x) - pade).abs() < 1e-12);
    assert!((pade - (-0.5f64).exp()).abs() < 1e-4);
    // `AirShadowBaseScale=0.5` is the default log, so a unit exactly at its
    // characteristic height draws at the base half scale.
    assert!((pade_exp_neg(0.693_376_137) - 0.5).abs() < 1e-3);
}

/// `Ext/TechnoType/Hooks.MatrixOp.cpp:677-709`: with the switch off nothing
/// scales except an aircraft-considered type, which takes the base scale.
#[test]
fn the_switch_off_scales_only_considered_aircraft() {
    let off = globals(false);
    assert_eq!(unit_size_scale(&off, 1000, None, None, 400, false), 1.0);
    assert_eq!(unit_size_scale(&off, 0, None, None, 400, false), 1.0);
    let aircraft = unit_size_scale(&off, 0, None, None, 400, true);
    assert!((aircraft - pade_exp_neg(0.693_376_137)).abs() < 1e-12);
}

/// `:680-691`: a Jumpjet divides its live height by the type key, falling back
/// to the locomotion's own `Height=` when the key is unset; `:697` a
/// non-Jumpjet only scales above 208 leptons and measures from that threshold.
#[test]
fn jumpjets_scale_from_their_own_height_and_ground_units_from_208() {
    let on = globals(true);
    // A Jumpjet at its own height (500) draws at half scale.
    let jumpjet = unit_size_scale(&on, 500, Some(500), None, 400, false);
    assert!((jumpjet - pade_exp_neg(0.693_376_137)).abs() < 1e-12);
    // The type key wins over the locomotion height.
    let keyed = unit_size_scale(&on, 500, Some(500), Some(1000), 400, false);
    assert!((keyed - pade_exp_neg(0.693_376_137 * 0.5)).abs() < 1e-12);
    // A ground unit at or below 208 leptons keeps the full size.
    assert_eq!(unit_size_scale(&on, 208, None, None, 400, false), 1.0);
    assert_eq!(unit_size_scale(&on, 100, None, None, 400, false), 1.0);
    // Above it, the excess over 208 is what divides.
    let lifted = unit_size_scale(&on, 608, None, None, 400, false);
    assert!((lifted - pade_exp_neg(0.693_376_137)).abs() < 1e-12);
    // A zero or negative characteristic height leaves the shadow unscaled.
    assert_eq!(unit_size_scale(&on, 600, Some(0), None, 400, false), 1.0);
    assert_eq!(unit_size_scale(&on, 600, None, Some(0), 400, false), 1.0);
}

/// `:686`: the approximation is floored by `HeightShadowScaling.MinScale=`.
#[test]
fn the_min_scale_floors_the_approximation() {
    let mut on = globals(true);
    on.height_shadow_scaling_min_scale = bits(0.6);
    let high = unit_size_scale(&on, 5000, Some(500), None, 400, false);
    assert_eq!(high, 0.6);
    let aircraft = aircraft_size_scale(&on, 5000, 500, None, true);
    assert_eq!(aircraft, 0.6);
    // Below the floor the value still comes from `Pade2_2`.
    let low = unit_size_scale(&on, 100, Some(500), None, 400, false);
    assert!(low <= 1.0 && low > 0.6);
}

/// `:958-973`: the aircraft path divides the live height by
/// `GetFlightLevel()`, and without the switch only a `ConsideredAircraft`
/// type scales — at the base scale, no height term.
#[test]
fn aircraft_scale_uses_the_flight_level_and_the_base_scale() {
    let on = globals(true);
    let leveled = aircraft_size_scale(&on, 200, 200, None, false);
    assert!((leveled - pade_exp_neg(0.693_376_137)).abs() < 1e-12);
    // A type whose `GetFlightLevel()` is zero cannot divide: full size.
    assert_eq!(aircraft_size_scale(&on, 300, 0, None, false), 1.0);
    let keyed = aircraft_size_scale(&on, 200, 200, Some(400), false);
    assert!((keyed - pade_exp_neg(0.693_376_137 * 0.5)).abs() < 1e-12);
    let off = globals(false);
    assert_eq!(aircraft_size_scale(&off, 200, 200, None, false), 1.0);
    assert!((aircraft_size_scale(&off, 0, 200, None, true) - 0.5).abs() < 1e-3);
}
