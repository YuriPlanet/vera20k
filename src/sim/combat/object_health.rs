//! Object5F5390's signed HP commit. Damage preparation never predicts this result.
//! Original-byte comparisons: tools/spatial_oracle/object_health (718 rows).
use super::{GameEntity, damage::DamageState};
use crate::util::native_x87::{MaskedX87Chop53 as X87, MaskedX87Ordering, NativeF64Bits};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HealthCallback {
    Changed,
    Kill,
    Destroy,
}

/// The callback boundary is synchronous. The world adapter stages existing kill/
/// destruction transactions after releasing its entity borrow; that adapter does
/// not claim complete native trigger/callback-body parity.
pub(super) fn commit(
    target: &mut GameEntity,
    packet: &mut i32,
    strength: i32,
    admitted: bool,
    building_no_c4: bool,
    condition_red: f64,
    mut callback: impl FnMut(&mut GameEntity, HealthCallback),
) -> DamageState {
    let previous = target.health.current;
    if previous <= 0 || !admitted {
        return DamageState::Unaffected;
    }
    if building_no_c4 {
        *packet = (*packet).max(1);
    }
    if *packet == 0 {
        return DamageState::Unaffected;
    }
    if *packet < 0 {
        // 5F546A..5495: the intermediate write precedes the signed cap;
        // changed callback receives the capped value, and cannot change return0.
        target.health.current = previous.wrapping_sub(*packet);
        if target.health.current > strength {
            target.health.current = strength;
        }
        if target.health.current != previous {
            callback(target, HealthCallback::Changed);
        }
        return DamageState::Unaffected;
    }
    *packet = (*packet).min(previous);
    let post = previous.wrapping_sub(*packet);
    let mut result = DamageState::Damaged;
    let yellow = strength >> 1;
    if yellow <= previous && post < yellow {
        result = DamageState::Yellow;
    }
    let red = X87::mul(
        X87::load_i32(strength),
        X87::load_f64(NativeF64Bits::from_bits(condition_red.to_bits())),
    );
    if X87::compare(red, X87::load_i32(previous)) == MaskedX87Ordering::Less
        && X87::compare(X87::load_i32(post), red) == MaskedX87Ordering::Less
    {
        result = DamageState::Red;
    }
    target.health.current = post;
    // Alive0 is result5, independent of the newly written health (including0).
    if !target.lifecycle.object_alive {
        return DamageState::AlreadyDead;
    }
    if target.health.current == 0 {
        callback(target, HealthCallback::Kill);
        callback(target, HealthCallback::Destroy);
        result = DamageState::Dead;
    }
    result
}

#[cfg(test)]
#[path = "object_health_tests.rs"]
mod tests;
