//! Shared native ballistics: the launch speed the Rules postpass and shot
//! launch use, and the arc test `TechnoClass::InRange` applies to an arcing
//! weapon.

use super::native_x87::{NativeF64Bits, X87Chop53 as X, X87Ordering, X87Value};

/// The gravity source a ballistic helper applies: the stock Rules
/// `Gravity=` integer, or the Phobos `[Projectile] Gravity=` double
/// (`BulletTypeExt::GetAdjustedGravity`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BallisticGravity {
    Rules(i32),
    Value(NativeF64Bits),
}

/// The gravity both helpers take, as their callers hand it over: the
/// selected value, halved for a `Floater=` projectile (`0x0048ACF0`), and
/// stored to a double.
fn ballistic_gravity(gravity: BallisticGravity, floater: bool) -> X87Value {
    let mut gravity = match gravity {
        BallisticGravity::Rules(value) => X::load_i32(value),
        BallisticGravity::Value(bits) => {
            X::load_f64(bits).expect("the Phobos Gravity= reader drops a non-finite value")
        }
    };
    if floater {
        gravity = X::mul(gravity, X::load_f64(NativeF64Bits::HALF).unwrap());
    }
    X::load_f64(X::store_f64(gravity).unwrap()).unwrap()
}

/// `Ballistic_Launch_Speed @ 0x0048AB90`, including the callers' Floater
/// gravity store (`0x0048ACF0`). Inputs are signed leptons and Rules Gravity.
///
/// Reuses the deterministic arithmetic already required by FireAt: native
/// stores, approximate square root and truncation can change flight/contact.
/// Execution corpora: `tools/projectile_oracle/fireat_speed.json` and
/// `tools/rules_oracle/weapon_speed_order.json`.
pub(crate) fn ballistic_launch_speed(
    distance: i32,
    gravity: BallisticGravity,
    floater: bool,
) -> i32 {
    let gravity = ballistic_gravity(gravity, floater);
    let product = X::mul(
        X::mul(X::load_i32(distance), gravity),
        X::load_f64(NativeF64Bits::from_bits(0x3ff3_3333_3333_3333)).unwrap(),
    );
    let stored = X::load_f64(X::store_f64(product).unwrap()).unwrap();
    let root = super::native_x87::sqrt_approx_f32(stored)
        .expect("signed32 ballistic inputs fit the native square-root lookup domain");
    X::ftol_i64(X::load_f32(root).unwrap())
        .expect("ballistic speed fits the native signed64 conversion") as i32
}

/// `Ballistic_Can_Reach @ 0x0048ABC0`, whose one caller is InRange's arcing
/// arm (`0x006F74CA`): whether a shell launched at `speed` reaches a point
/// `height` leptons above its source (target Z less source Z) and `distance`
/// leptons away, under the same gravity as [`ballistic_launch_speed`].
///
/// A zero distance counts as 0.001 (`0x007E3818`). The shot cannot reach when
/// the discriminant `v⁴ − 2v²hg − g²d²` is negative or `2(h²/d² + 1)` is zero;
/// otherwise it can when either root `(v² − hg ± Sqrt_Approx(disc)) /
/// 2(h²/d² + 1)` is at least zero. `h`, `v²`, `d²`, the discriminant, the
/// denominator and `v² − hg` pass through the doubles of the native stack
/// slots.
///
/// Native execution: the arcing rows of `tools/spatial_oracle/walk_cell_range`
/// run it inside InRange under Rules gravity 6, 6 with `Floater=`, -6 and 0,
/// and at a zero distance.
pub(crate) fn ballistic_can_reach(
    speed: i32,
    distance: i32,
    height: i32,
    gravity: BallisticGravity,
    floater: bool,
) -> bool {
    let gravity = ballistic_gravity(gravity, floater);
    let constant = |bits: NativeF64Bits| X::load_f64(bits).unwrap();
    let zero = constant(NativeF64Bits::POSITIVE_ZERO);
    let double = |value: X87Value| {
        X::load_f64(X::store_f64(value).expect("signed32 ballistics stay finite")).unwrap()
    };
    let v = X::load_i32(speed);
    let mut d = X::load_i32(distance);
    if X::compare(d, zero) == X87Ordering::Equal {
        d = constant(NativeF64Bits::from_bits(0x3f50_624d_d2f1_a9fc));
    }
    let h = double(X::load_i32(height));
    let v2 = double(X::mul(v, v));
    let d2 = double(X::mul(d, d));
    let v4 = X::mul(v, X::mul(v2, v));
    let lift = X::mul(X::mul(v2, h), gravity);
    let disc = X::sub(
        X::sub(v4, X::add(lift, lift)),
        X::mul(X::mul(gravity, gravity), d2),
    );
    if X::compare(disc, zero) == X87Ordering::Less {
        return false;
    }
    let disc = double(disc);
    let den = X::add(
        X::div(X::mul(h, h), d2).expect("d² is never zero"),
        constant(NativeF64Bits::ONE),
    );
    let den = X::add(den, den);
    if X::compare(den, zero) == X87Ordering::Equal {
        return false;
    }
    let den = double(den);
    let base = double(X::sub(v2, X::mul(h, gravity)));
    let root = X::load_f32(
        super::native_x87::sqrt_approx_f32(disc).expect("a finite discriminant has a root"),
    )
    .expect("Sqrt_Approx returns a finite value");
    let near = X::div(X::add(root, base), den).expect("the denominator is not zero");
    if X::compare(near, zero) != X87Ordering::Less {
        return true;
    }
    let far = X::div(X::sub(base, root), den).expect("the denominator is not zero");
    X::compare(far, zero) != X87Ordering::Less
}
