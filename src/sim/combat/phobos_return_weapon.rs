//! Phobos `ReturnWeapon` (`Ext/Bullet/Hooks.DetonateLogics.cpp:484-500`).
//!
//! ```cpp
//! // Return to sender
//! if (pTechno)                                   // pThis->Owner, the firer
//! {
//!     auto const pTypeExt = BulletTypeExt::Fetch(pThis->Type);
//!     if (auto const pWeapon = pTypeExt->ReturnWeapon)
//!     {
//!         int damage = pWeapon->Damage;
//!         if (pTypeExt->ReturnWeapon_ApplyFirepowerMult.Get(
//!                 RulesExt::Global()->ReturnWeapon_ApplyFirepowerMult))
//!             damage = static_cast<int>(damage * pBulletExt->FirepowerMult);
//!
//!         if (auto const pBullet = pWeapon->Projectile->CreateBullet(
//!                 pTechno, pTechno, damage, pWeapon->Warhead,
//!                 pWeapon->Speed, pWeapon->Bright))
//!         {
//!             BulletExt::Fetch(pBullet)->FirepowerMult = pBulletExt->FirepowerMult;
//!             BulletExt::SimulatedFiringUnlimbo(pBullet, pOwner, pWeapon,
//!                                                pThis->Location, true);
//!             BulletExt::SimulatedFiringEffects(pBullet, pOwner, nullptr, false, true);
//!         }
//!     }
//! }
//! ```
//!
//! So the returned shot:
//! - exists only while the **firing techno** is still around (`if (pTechno)`);
//! - is a real bullet of the named weapon's own projectile type, carrying the
//!   weapon's warhead, speed and `Bright=`;
//! - targets **the firer itself** (`CreateBullet(pTechno, pTechno, ...)`);
//! - is unlimboed at the **impact coordinate** (`pThis->Location`), not at the
//!   firer, and heads straight at its target (`headToTarget = true`);
//! - takes the dying bullet's `FirepowerMult`, which also scales `Damage=`
//!   when `ReturnWeapon.ApplyFirepowerMult` (per type, else
//!   `[CombatDamage]`, default `no`) is set — `static_cast<int>` truncates
//!   toward zero;
//! - plays the weapon's **visual** effects only
//!   (`SimulatedFiringEffects(..., firingEffect=false, visualEffect=true)`).
//!
//! `pOwner` is `pTechno->Owner`, falling back to `BulletExt::FirerHouse` only
//! when the firer is gone — but that arm is unreachable here, because the
//! whole block is already gated on `pTechno`.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ and the rest of sim/.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::rules::ruleset::RuleSet;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectilePayload, ProjectileTarget,
};
use crate::sim::world::Simulation;
use crate::util::native_x87::NativeF64Bits;

use super::{FiredBullet, admit_fired_bullet, phobos_airburst};

/// The `ReturnWeapon` emission that runs after this cluster's own detonation
/// (`Ext/Bullet/Hooks.DetonateLogics.cpp:484-500`), before the airburst tail.
pub(super) fn emit(world: &mut Simulation, rules: &RuleSet, detonation: &ProjectileDetonation) {
    // The hook reads the still-live Bullet; a receiver-only packet has none.
    let Some(parent) = world.projectiles.get(detonation.projectile_id) else {
        return;
    };
    let parent_position = parent.position();
    let parent_payload = parent.payload();
    let parent_firepower = parent_payload.firepower_multiplier();
    let Some(parent_type_id) = parent_payload.projectile_type() else {
        return;
    };
    let Some(parent_type) = rules.projectile(world.interner.resolve(parent_type_id)) else {
        return;
    };
    let Some(weapon_name) = parent_type.phobos_return_weapon.weapon() else {
        return;
    };
    // `if (pTechno)`: the shot is only returned while the firer still exists.
    let current = world.projectiles.current_detonation(*detonation);
    let source_id = current.source_id;
    if source_id == 0 || world.substrate.entities.get(source_id).is_none() {
        return;
    }
    let Some(weapon) = rules.weapon(weapon_name) else {
        return;
    };
    let Some(child_type_name) = weapon.projectile.as_deref() else {
        return;
    };
    let Some(child_type) = rules.weapon_projectile(weapon) else {
        return;
    };
    let Some(warhead_name) = weapon.warhead.as_deref() else {
        return;
    };
    // `CreateBullet(..., pWeapon->Warhead, ...)`: the named warhead must exist.
    if rules.weapon_warhead(weapon).is_none() {
        return;
    }
    let warhead_id = world.interner.intern(warhead_name);
    let child_type_id = world.interner.intern(child_type_name);
    let weapon_id = world.interner.intern(weapon_name);

    // `pOwner = pTechno->Owner`; the `FirerHouse` fallback is unreachable
    // behind the `pTechno` gate above.
    let source_owner = world
        .substrate
        .entities
        .get(source_id)
        .map(|source| source.owner());
    let owner_house = source_owner
        .filter(|house| {
            !world
                .houses
                .get(house)
                .is_some_and(|state| state.is_defeated())
        })
        .or_else(|| world.neutral_house());
    let Some(owner_house) = owner_house else {
        return;
    };

    let damage = return_damage(
        weapon.damage,
        parent_type
            .phobos_return_weapon
            .applies_firepower_mult(rules.phobos_globals.return_weapon_apply_firepower_mult),
        parent_firepower,
    );

    // `CreateBullet(pTechno, pTechno, ...)`: the firer owns the shot and is
    // also its target. `SimulatedFiringUnlimbo` then places it at the impact.
    let origin: ProjectileCoord = current.impact;
    let target = ProjectileTarget::Entity(source_id);
    let target_coords = phobos_airburst::target_coord(world, target).unwrap_or(parent_position);
    let velocity = phobos_airburst::simulated_velocity(
        world,
        rules,
        child_type,
        weapon,
        origin,
        target_coords,
        true,
    );

    let native_unique_id = world.next_native_runtime_id();
    let bullet_id = world.substrate.allocate_stable_id();
    let payload = ProjectilePayload::new(damage, warhead_id, Some(child_type_id), weapon.bright)
        .with_installed_weapon(Some(weapon_id))
        // `BulletExt::Fetch(pBullet)->FirepowerMult = pBulletExt->FirepowerMult`.
        .with_firepower_multiplier(parent_firepower);
    let Some(bullet) = FiredBullet::launched(
        world,
        rules,
        native_unique_id,
        source_id,
        target,
        payload,
        weapon.speed,
        origin,
        velocity,
    ) else {
        return;
    };
    admit_fired_bullet(world, rules, weapon, bullet_id, bullet);
    world
        .projectiles
        .set_phobos_firer_house(bullet_id, Some(owner_house));
    if child_type.firers_palette {
        world
            .projectiles
            .set_firer_house(bullet_id, Some(owner_house));
    }
    // `SimulatedFiringEffects(..., firingEffect=false, visualEffect=true)`.
    phobos_airburst::admit_firing_effects(
        world,
        rules,
        weapon,
        child_type,
        source_id,
        owner_house,
        origin,
        target,
        target_coords,
        velocity,
        false,
        false,
    );
}

/// `int damage = pWeapon->Damage;` then, when the multiplier applies,
/// `static_cast<int>(damage * pBulletExt->FirepowerMult)` — a binary64
/// product truncated toward zero and re-cast to `int`.
fn return_damage(weapon_damage: i32, apply_mult: bool, firepower: NativeF64Bits) -> i32 {
    if !apply_mult {
        return weapon_damage;
    }
    let product = f64::from(weapon_damage) * f64::from_bits(firepower.bits());
    if product > f64::from(i32::MAX) {
        i32::MAX
    } else if product < f64::from(i32::MIN) {
        i32::MIN
    } else {
        product as i32
    }
}

#[cfg(all(test, tests_sim_combat_d_z))]
#[path = "phobos_return_weapon_emission_tests.rs"]
mod tests;
