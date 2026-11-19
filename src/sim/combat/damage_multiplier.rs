//! Phobos damage multipliers (`Ext/Techno/Hooks.ReceiveDamage.cpp:48-82`), the
//! block inside `TechnoClass_ReceiveDamage_Shield @ 0x701900` that runs after
//! the per-target effects (`:39-45`) and before the combat alert (`:86`) and
//! the shield stage.
//!
//! The block is guarded by `!args->IgnoreDefenses && damage`: a forced hit and
//! a zero packet never rescale. It picks one of three columns from the source
//! House's relation to the victim's House, resolves that column through the
//! warhead's own `Nullable<double>` and then the `[CombatDamage]` global, and
//! finally **adds** the two health-fraction terms. A `.Berzerk` column is used
//! when the **attacker** is berserk, and the owner/ally columns substitute the
//! global `.NotAffectsEnemies` value when the warhead does not affect enemies.
//!
//! The result is applied as `(int)(damage * multiplier)`, and a truncation to
//! zero keeps the damage's sign instead (`:82-87`), so a 0.4× hit on one point
//! of damage, or a 0.5× hit on two, still lands as ±1.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ and sim/ only; never audio/ or ui/.

use crate::rules::phobos_damage_multiplier::DamageRelation;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::receiver_call::EntityDamageEvent;
use crate::sim::world::Simulation;

/// The receiver's damage after the Phobos multiplier block.
///
/// `damage` is the value the preceding stages left in `args->Damage`; the
/// returned value replaces it before the shield stage reads the pointer.
pub(crate) fn apply(
    world: &Simulation,
    rules: &RuleSet,
    event: &EntityDamageEvent,
    damage: i32,
) -> i32 {
    // `if (!args->IgnoreDefenses && damage)` (`:50`).
    if event.receiver_flags.ignore_defenses || damage == 0 {
        return damage;
    }
    let Some(warhead) = rules.warhead(world.interner.resolve(event.warhead_ref)) else {
        return damage;
    };
    let values = &warhead.phobos_damage_multiplier;
    let Some(target) = world.substrate.entities.get(event.target_id) else {
        return damage;
    };
    // `pAttacker` (`:52`) is the live firing object; `pSourceHouse` (`:53`)
    // falls back to the event's recorded House when the shot has none.
    let attacker = {
        let id = event.attacker_id;
        (id != crate::sim::combat::receiver_call::RAD_NO_ATTACKER)
            .then(|| world.substrate.entities.get(id))
            .flatten()
    };
    let source_house = attacker.map(|entity| entity.owner()).or(event.source_house);
    let target_house = target.owner();
    let allied = source_house.is_some_and(|owner| {
        world
            .house_alliances
            .is_allied(&world.interner, owner, target_house)
    });
    let relation = DamageRelation::classify(
        source_house.map(|owner| owner.index()),
        target_house.index(),
        allied,
    );
    let globals = &rules.phobos_globals;
    let berzerk = attacker.is_some_and(|entity| entity.berserk.is_active());
    let affects_enemies = warhead.affects_enemies;
    let mut multiplier = match (relation, berzerk) {
        // `:62-63`.
        (DamageRelation::Enemies, true) => values
            .enemies_berzerk
            .or(globals.damage_enemies_multiplier_berzerk())
            .unwrap_or(globals.damage_enemies_multiplier()),
        // `:69-70`.
        (DamageRelation::Enemies, false) => values
            .enemies
            .unwrap_or(globals.damage_enemies_multiplier()),
        // `:64-65`.
        (DamageRelation::Allies, true) => values
            .allies_berzerk
            .or(globals.damage_allies_multiplier_berzerk())
            .unwrap_or_else(|| allies_column_fallback(globals, affects_enemies)),
        // `:71-72`.
        (DamageRelation::Allies, false) => values
            .allies
            .unwrap_or_else(|| allies_column_fallback(globals, affects_enemies)),
        // `:66-67`.
        (DamageRelation::Owner, true) => values
            .owner_berzerk
            .or(globals.damage_owner_multiplier_berzerk())
            .unwrap_or_else(|| owner_column_fallback(globals, affects_enemies)),
        // `:73-74`.
        (DamageRelation::Owner, false) => values
            .owner
            .unwrap_or_else(|| owner_column_fallback(globals, affects_enemies)),
    };
    // `if (pWHExt->DamageSourceHealthMultiplier && pAttacker)` (`:76-77`): the
    // term is a `+=` scaled by the shooter's health fraction, and a zero
    // value drops it.
    if values.source_health != 0.0
        && let Some(attacker) = attacker
        && let Some(object) = world.interner.object_type(attacker.type_ref(), rules)
    {
        multiplier +=
            values.source_health * health_fraction(attacker.health.current(), object.strength);
    }
    // `if (pWHExt->DamageTargetHealthMultiplier)` (`:79-80`).
    if values.target_health != 0.0
        && let Some(object) = world.interner.object_type(target.type_ref(), rules)
    {
        multiplier +=
            values.target_health * health_fraction(target.health.current(), object.strength);
    }
    // `if (multiplier != 1.0)` (`:82`).
    if multiplier == 1.0 {
        return damage;
    }
    // `const auto sgnDamage = damage > 0 ? 1 : -1;` (`:84`).
    let sign = if damage > 0 { 1 } else { -1 };
    // `static_cast<int>(damage * multiplier)` (`:85`) truncates through
    // `Math__ftol`; a NaN or out-of-range product becomes the integer
    // indefinite, whose low dword is zero.
    let scaled =
        crate::util::native_x87::X87Chop53::ftol_f64_low_masked(f64::from(damage) * multiplier);
    // `damage = calculateDamage ? calculateDamage : sgnDamage;` (`:86`).
    if scaled == 0 { sign } else { scaled }
}

/// `!pWHExt->AffectsEnemies ? RulesExt::Global()->DamageAlliesMultiplier_NotAffectsEnemies.Get(RulesExt::Global()->DamageAlliesMultiplier) : RulesExt::Global()->DamageAlliesMultiplier`
/// (`Ext/Techno/Hooks.ReceiveDamage.cpp:72`).
fn allies_column_fallback(
    globals: &crate::rules::phobos_globals::PhobosGlobals,
    affects_enemies: bool,
) -> f64 {
    if affects_enemies {
        globals.damage_allies_multiplier()
    } else {
        globals
            .damage_allies_multiplier_not_affects_enemies()
            .unwrap_or(globals.damage_allies_multiplier())
    }
}

/// The owner-column counterpart of [`allies_column_fallback`] (`:74`).
fn owner_column_fallback(
    globals: &crate::rules::phobos_globals::PhobosGlobals,
    affects_enemies: bool,
) -> f64 {
    if affects_enemies {
        globals.damage_owner_multiplier()
    } else {
        globals
            .damage_owner_multiplier_not_affects_enemies()
            .unwrap_or(globals.damage_owner_multiplier())
    }
}

/// `TechnoClass::GetHealthPercentage()`: `Health` over the type's `Strength`,
/// both as binary64. A zero Strength divides to infinity, as it does natively.
fn health_fraction(health: i32, strength: i32) -> f64 {
    f64::from(health) / f64::from(strength)
}

#[cfg(all(test, tests_sim_combat_d_z))]
#[path = "damage_multiplier_tests.rs"]
mod tests;
