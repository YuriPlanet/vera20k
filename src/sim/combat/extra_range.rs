//! Phobos `ExtraRange.*`: the four bonuses
//! `TechnoClass_InRange_WeaponRange @ 0x6F7248`
//! (`Ext/Techno/Hooks.WeaponRange.cpp:133..180`) adds to the range returned by
//! `WeaponTypeExt::GetRangeWithModifiers` before `TechnoClass::InRange`'s other
//! terms. Phobos reads all four keys from `[General]`
//! (`Ext/Rules/Body.cpp:511..515`) and its per-weapon copies
//! (`Ext/WeaponType/Body.cpp:200..203`) also read `GameStrings::General`, so
//! they always mirror the globals; VERA owns the globals only.
//!
//! The additions are skipped when `KeepRange` replaced the maximum range and
//! when the -512 always-in-range sentinel is returned (`:145..148`).

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::in_range::RangeTarget;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::SpeedRules;
use crate::sim::movement::motion_query::is_really_moving_now;
use crate::sim::world::Simulation;

/// The sum of `ExtraRange.Prefiring`, `.TargetMoving` and `.FirerMoving` that
/// applies to this shot, in leptons.
pub(crate) fn extra_range_leptons(
    world: &Simulation,
    rules: &RuleSet,
    attacker: &GameEntity,
    target: RangeTarget<'_>,
    weapon: &crate::rules::weapon_type::WeaponType,
) -> i64 {
    let globals = &rules.phobos_globals;
    let frame = world.session.binary_frame() as u32;
    let speed = SpeedRules::new(rules, &world.interner, &world.houses);
    let mut extra: i64 = 0;

    // `:152..155` `IsPrefiring` with the global `IncludeBurst` default true.
    let prefiring = i64::from(globals.extra_range_prefiring);
    if prefiring != 0
        && is_prefiring(
            attacker,
            weapon,
            globals.extra_range_prefiring_include_burst,
        )
    {
        extra = extra.wrapping_add(prefiring);
    }

    // `:157..162`: the target's `IsChasing`; the firer must be a Foot class
    // (`:53..54` returns false otherwise). Phobos's per-weapon
    // `ExtraRange_TargetMoving` is itself read from `[General]`
    // (`Ext/WeaponType/Body.cpp:200`), so an authored global makes its
    // `isset()` true and the `CloseRangeOnly` fallback arm never runs: the
    // bonus applies without the `CloseRange` gate. VERA reproduces that.
    let target_moving = i64::from(globals.extra_range_target_moving);
    if target_moving != 0
        && matches!(
            attacker.category(),
            crate::map::entities::EntityCategory::Infantry
                | crate::map::entities::EntityCategory::Unit
                | crate::map::entities::EntityCategory::Aircraft
        )
        && let RangeTarget::Techno(target) = target
        && is_really_moving_now(target, speed, frame)
    {
        extra = extra.wrapping_add(target_moving);
    }

    // `:164..168` `IsMovingFire`.
    let firer_moving = i64::from(globals.extra_range_firer_moving);
    if firer_moving != 0 && is_really_moving_now(attacker, speed, frame) {
        extra = extra.wrapping_add(firer_moving);
    }

    extra
}

/// `IsPrefiring` (`Ext/Techno/Hooks.WeaponRange.cpp:74..131`). Only the burst
/// arm is ported: `CurrentBurstIndex % Burst != 0` under `IncludeBurst`.
///
/// RESIDUAL: the `DelayedFireTimer`, the Unit `CurrentFiringFrame`/
/// `FiringSyncFrame`, the Aircraft attack-status and the Building
/// `DelayBeforeFiring`/`IsFiringNow` arms need state VERA does not own yet, so
/// they answer false.
fn is_prefiring(
    attacker: &GameEntity,
    weapon: &crate::rules::weapon_type::WeaponType,
    include_burst: bool,
) -> bool {
    include_burst && weapon.burst > 0 && attacker.weapon_burst.index() % weapon.burst != 0
}

#[cfg(all(test, feature = "phobos"))]
#[path = "extra_range_tests.rs"]
mod tests;
