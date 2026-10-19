//! Shared native projectile scalar operations and lookup boundaries.
//! FireAt (6FE8EE), HomingTrack (5B20F0), and their direction helpers use
//! the same represented arithmetic. Keep stores explicit at their callers.

use super::ProjectileVelocity as Velocity;
use crate::util::direction_tables::{facing16_from_angle, radians_from_facing16};
use crate::util::native_x87::{
    NativeF32Bits, NativeF64Bits, X87Chop53 as X, X87Ordering, X87Value,
};
use crate::util::retail_trig::TrigTable;

pub(super) const PI_HALF: NativeF64Bits = NativeF64Bits::from_bits(0x3ff9_21fb_5444_2d18);

pub(super) fn d(bits: NativeF64Bits) -> X87Value {
    X::load_f64(bits).expect("finite native launch input")
}

pub(super) fn f(bits: NativeF32Bits) -> X87Value {
    X::load_f32(bits).expect("finite native launch table value")
}

pub(super) fn store(value: X87Value) -> NativeF64Bits {
    X::store_f64(value).expect("finite stored native launch value")
}

pub(super) fn round(value: X87Value) -> X87Value {
    d(store(value))
}

pub(super) fn int(value: X87Value) -> i32 {
    X::ftol_i64(value).expect("native launch conversion fits signed i64") as i32
}

pub(super) fn sqrt(value: X87Value) -> X87Value {
    f(crate::util::native_x87::sqrt_approx_f32(round(value))
        .expect("native launch squared value fits the finite lookup domain"))
}

pub(super) fn less(a: X87Value, b: X87Value) -> bool {
    X::compare(a, b) == X87Ordering::Less
}

pub(super) fn atan(y: X87Value, x: X87Value) -> X87Value {
    crate::util::direction_tables::native_atan2_f32(
        X::store_f32(y).expect("finite launch atan numerator"),
        X::store_f32(x).expect("finite launch atan denominator"),
    )
}

pub(super) fn square(value: X87Value) -> X87Value {
    X::mul(value, value)
}

/// `Vector3D::Length2D` (`0x0041C350`, `0x0041C430`): `Sqrt_Approx(x·x + y·y)`.
pub(super) fn length2(v: Velocity) -> X87Value {
    sqrt(X::add(square(d(v.x)), square(d(v.y))))
}

/// A 3-D length with the squares summed in `order` (`0x0041C3C0` is
/// `[0, 1, 2]`).
pub(super) fn length3(v: Velocity, order: [usize; 3]) -> X87Value {
    let t = v.native().map(|x| square(d(x)));
    sqrt(X::add(X::add(t[order[0]], t[order[1]]), t[order[2]]))
}

/// `FCOMP` against 0.0 answers equal for either signed zero.
pub(super) fn zero(value: NativeF64Bits) -> bool {
    value.bits() & 0x7fff_ffff_ffff_ffff == 0
}

/// `Vector3D::Pitch` (`0x0041C2E0`): the DirStruct of `atan2(z, Length2D)`.
pub(super) fn pitch(v: Velocity) -> u16 {
    facing16_from_angle(atan(d(v.z), round(length2(v))))
}

/// The inline pitch change FireAt-family code makes: divide the X and Y by
/// the cosine of the existing pitch (unless it is zero), then scale them by
/// the cosine of `desired` and set Z to its sine times the prior length.
pub(super) fn apply_pitch(
    mut v: Velocity,
    desired: u16,
    order: [usize; 3],
    trig: &TrigTable,
) -> Velocity {
    let current = radians_from_facing16(pitch(v));
    let magnitude = round(length3(v, order));
    if !zero(current) {
        let divisor = trig.cos_from_table(d(current));
        v.x = store(X::div(d(v.x), divisor).expect("finite native pitch cosine"));
        v.y = store(X::div(d(v.y), divisor).expect("finite native pitch cosine"));
    }
    let angle = radians_from_facing16(desired);
    v.x = store(X::mul(trig.cos_from_table(d(angle)), d(v.x)));
    v.y = store(X::mul(trig.cos_from_table(d(angle)), d(v.y)));
    v.z = store(X::mul(trig.sin_from_table(d(angle)), magnitude));
    v
}

/// `Vector3D::SeedIfZeroXY` (`0x0041C460`) then the XY turn through
/// Length2D (`0x0041C430`): a zero XY seeds X 100.0, then the XY turns to
/// `heading` at its 2-D length; Z stays.
pub(super) fn turn_xy(mut v: Velocity, heading: u16, trig: &TrigTable) -> Velocity {
    if zero(v.x) && zero(v.y) {
        v.x = Velocity::new(100, 0, 0).x;
    }
    let length = round(length2(v));
    let heading = radians_from_facing16(heading);
    v.x = store(X::mul(trig.cos_from_table(d(heading)), length));
    v.y = store(X::neg(X::mul(trig.sin_from_table(d(heading)), length)));
    v
}
