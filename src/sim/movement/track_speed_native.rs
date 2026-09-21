//! Exact numeric leaves for the retained Drive/Ship speed prefix.
//!
//! Original Drive4B0F69..1295 / Ship6A0639..095D and Foot4DB1A0.
//! Caller-owned admission, target publication, terrain lookup and flag producers
//! are deliberately separate. This is not wired into production until its live
//! owners retain native precision; no fixed-point mirror is maintained here.

use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};
use crate::util::native_x87::{NativeX87Error, X87Chop53 as X, X87Ordering, sqrt_approx_f32};

// Original qwords7E6240/7E6248/7E6250 (Ship7F1308/7F1310/7F1318)
// are promoted float constants, unlike the actual binary64 crush cap7E3548.
const DESTINATION_FLOOR: NativeF64Bits = NativeF64Bits::from_bits(0x3fd3_3333_4000_0000);
const SINKING_FLOOR: NativeF64Bits = NativeF64Bits::from_bits(0x3fb9_9999_a000_0000);
const SINKING_DECEL: NativeF64Bits = NativeF64Bits::from_bits(0x3f58_9374_c000_0000);
const CRUSH_CAP: NativeF64Bits = NativeF64Bits::from_bits(0x3fc9_9999_9999_999a);

/// Actual Foot4D3710 finite clamp. Negative zero survives the equality arms.
pub(crate) fn set_fraction(value: NativeF64Bits) -> Result<NativeF64Bits, NativeX87Error> {
    let number = X::load_f64(value)?;
    if X::compare(number, X::load_i32(0)) == X87Ordering::Less {
        Ok(NativeF64Bits::POSITIVE_ZERO)
    } else if X::compare(number, X::load_i32(1)) == X87Ordering::Greater {
        Ok(NativeF64Bits::ONE)
    } else {
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FootSpeedInputs {
    pub raw_type_speed: i32,
    pub house_multiplier: NativeF32Bits,
    pub crate_multiplier: NativeF64Bits,
    pub faster: bool,
    pub veteran_multiplier: NativeF64Bits,
    pub applied_fraction: NativeF64Bits,
    pub unit_flag_carrier: bool,
}

/// Foot4DB1A0 consumes low32 after each native signed64 ftol, then optionally
/// halves the signed final i32. Each operand is a live getter input.
pub(crate) fn current_speed(input: FootSpeedInputs) -> Result<i32, NativeX87Error> {
    let house = X::load_f32(input.house_multiplier)?;
    let house = X::load_f64(X::store_f64(house)?)?;
    let product = X::mul(X::load_i32(input.raw_type_speed), house);
    let product = X::mul(product, X::load_f64(input.crate_multiplier)?);
    let mut stage = X::ftol_i64(product)? as i32;
    if input.faster {
        stage = X::ftol_i64(X::mul(
            X::load_i32(stage),
            X::load_f64(input.veteran_multiplier)?,
        ))? as i32;
    }
    stage = X::ftol_i64(X::mul(
        X::load_i32(stage),
        X::load_f64(input.applied_fraction)?,
    ))? as i32;
    Ok(if input.unit_flag_carrier {
        stage / 2
    } else {
        stage
    })
}

/// Drive4B1024..1082 / Ship6A06F4..0752 sum z²+y²+x², store a qword, call
/// Sqrt_Approx4CAC40 and ftol. `destination` already contains the native surface
/// Z and structural bridge offset supplied by the actual destination-cell query.
pub(crate) fn braking_distance(
    current: [i32; 3],
    destination: [i32; 3],
) -> Result<i32, NativeX87Error> {
    let square = |axis: usize| {
        let delta = X::load_i32(current[axis].wrapping_sub(destination[axis]));
        X::mul(delta, delta)
    };
    let sum = X::add(X::add(square(2), square(1)), square(0));
    let stored = X::load_f64(X::store_f64(sum)?)?;
    let root = X::load_f32(sqrt_approx_f32(stored)?)?;
    Ok(X::ftol_i64(root)? as i32)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TrackSpeedInputs {
    pub target: NativeF64Bits,
    pub applied: NativeF64Bits,
    pub accelerates: bool,
    pub is_unit: bool,
    pub unit_passive: bool,
    pub selector: i32,
    pub raw_type_speed: i32,
    pub acceleration: NativeF64Bits,
    pub deceleration: NativeF64Bits,
    pub slowdown_distance: i32,
    pub distance: i32,
    pub sinking: bool,
    pub crush_slowdown: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TrackSpeedOutput {
    pub target: NativeF64Bits,
    pub applied: NativeF64Bits,
    pub called_setter: bool,
    /// Original Unit linked-member propagation follows even the equal-value
    /// ramp arm. It is skipped by Passive/special and Accelerates=false.
    pub propagate_to_linked_units: bool,
}

/// Update retained fractions only; fresh target calculation is a different
/// native ProcessMovement operation. A retry still executes this prefix.
pub(crate) fn track_prefix(input: TrackSpeedInputs) -> Result<TrackSpeedOutput, NativeX87Error> {
    let mut output = TrackSpeedOutput {
        target: input.target,
        applied: input.applied,
        called_setter: false,
        propagate_to_linked_units: false,
    };
    if !input.accelerates {
        output.applied = set_fraction(input.target)?;
        output.called_setter = true;
        return Ok(output);
    }
    if input.selector >= 64 || (input.is_unit && input.unit_passive) {
        return Ok(output);
    }
    output.propagate_to_linked_units = input.is_unit;
    let mut candidate = input.applied;
    let mut braking = false;
    let subtract_with_floor = |deceleration, floor| -> Result<NativeF64Bits, NativeX87Error> {
        let value = X::sub(
            X::load_f64(input.applied)?,
            X::mul(
                X::load_i32(input.raw_type_speed),
                X::load_f64(deceleration)?,
            ),
        );
        Ok(
            if X::compare(value, X::load_f64(floor)?) == X87Ordering::Less {
                floor
            } else {
                X::store_f64(value)?
            },
        )
    };
    if input.distance < input.slowdown_distance {
        candidate = subtract_with_floor(input.deceleration, DESTINATION_FLOOR)?;
        braking = true;
    } else if input.sinking {
        candidate = subtract_with_floor(SINKING_DECEL, SINKING_FLOOR)?;
        braking = true;
    }
    if input.crush_slowdown {
        candidate = if X::compare(X::load_f64(input.target)?, X::load_f64(CRUSH_CAP)?)
            == X87Ordering::Less
        {
            input.target
        } else {
            CRUSH_CAP
        };
        output.target = candidate;
    } else if !braking {
        match X::compare(X::load_f64(input.applied)?, X::load_f64(input.target)?) {
            X87Ordering::Less => {
                let raised = X::add(
                    X::load_f64(input.acceleration)?,
                    X::load_f64(input.applied)?,
                );
                candidate =
                    if X::compare(raised, X::load_f64(input.target)?) == X87Ordering::Greater {
                        input.target
                    } else {
                        X::store_f64(raised)?
                    };
            }
            X87Ordering::Greater => {
                candidate = subtract_with_floor(input.deceleration, input.target)?;
            }
            X87Ordering::Equal => return Ok(output),
        }
    }
    output.called_setter = true;
    output.applied = set_fraction(candidate)?;
    Ok(output)
}

/// Original retry flag masks only the already-evaluated getter contribution.
pub(crate) fn invocation_budget(current_speed: i32, residual: i32, retry: bool) -> i32 {
    residual.wrapping_add(if retry { 0 } else { current_speed })
}

#[cfg(test)]
#[path = "track_speed_native_tests.rs"]
mod tests;
