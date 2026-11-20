//! Phobos `BulletClass_Logics_Extras`'s ExtraWarheads arm
//! (`Ext/Bullet/Hooks.DetonateLogics.cpp:367-470`, hook `0x469AA4`): after the
//! weapon's own warhead detonated, fire the weapon's `ExtraWarheads=` list at
//! the same impact.
//!
//! Selection (`:429-470`): with `ExtraWarheads.RandomWeightsN` present, one
//! scenario `RandomDouble` per `RollChances` entry (one sure roll when the
//! list is empty), the same dice choosing a weight index through
//! `GeneralUtils::ChooseOneWeighted` (single-precision weight sums), then an
//! optional second draw against the selected warhead's `DetonationChances`
//! entry; without weights, each warhead draws its own chance. Damages and the
//! full/area split come from the per-index override lists (last entry for
//! indices past their length). A full detonation is `BulletExt::Detonate`
//! (`WarheadTypeExt::DetonateAt`, a bullet with no weapon type); the other arm
//! is `DamageAreaWithTarget` with this bullet's target as the intended one.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim::combat and sim::projectile.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::world_receiver::{
    ReceiverRun, apply_area_damage_in_run_with_target, commit_projectiles, detonation_source_house,
};
use crate::sim::combat::{CombatEmit, UnderAttackEvent, warhead_triggers_allow};
use crate::sim::intern::InternedId;
use crate::sim::projectile::{
    ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload, ProjectileTarget,
};
use crate::sim::world::Simulation;

pub(super) fn emit(
    world: &mut Simulation,
    run: &mut ReceiverRun<'_>,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    detonation: &ProjectileDetonation,
    out: &mut CombatEmit,
    under_attack_events: &mut Vec<UnderAttackEvent>,
) {
    // `pThis->WeaponType`: only a weapon-fired bullet remembers it.
    let Some(weapon_ref) = detonation.payload.installed_weapon() else {
        return;
    };
    let weapon_name = world.interner.resolve(weapon_ref).to_owned();
    let Some(weapon) = rules.weapon(&weapon_name) else {
        return;
    };
    let settings = &weapon.phobos_extra_warheads;
    if settings.is_empty() {
        return;
    }
    // `ValueableVector<WarheadTypeClass*>` resolves through `FindOrAllocate`
    // (`Utilities/TemplateDef.h:1975-1991`); the rules projection materializes
    // an empty type for every `ExtraWarheads` reference, so a name the
    // sections omit still keeps its index and detonates.
    let warheads: Vec<(&str, &crate::rules::warhead_type::WarheadType)> = settings
        .warheads
        .iter()
        .filter_map(|name| rules.warhead(name).map(|warhead| (name.as_str(), warhead)))
        .collect();
    if warheads.is_empty() {
        return;
    }
    let source_house = detonation_source_house(world, detonation);
    let target_id = match detonation.target {
        ProjectileTarget::Entity(id) => Some(id),
        _ => None,
    };
    let invoker = (detonation.source_id != 0).then_some(detonation.source_id);
    let default_damage = weapon.damage;

    if !settings.weights.is_empty() {
        let roll_count = settings.roll_chances.len().max(1);
        for index in 0..roll_count {
            let dice = world.scenario_rng.random_double();
            if !settings.roll_chances.is_empty() && dice > settings.roll_chances[index] {
                continue;
            }
            let weight_index = index.min(settings.weights.len() - 1);
            let selected = choose_one_weighted(dice, &settings.weights[weight_index]);
            let mut detonate = true;
            if !settings.detonation_chances.is_empty() {
                // A `-1` pick still draws the chance dice and indexes the
                // last entry before `detonateWarhead` rejects it.
                let chance_index = selected
                    .filter(|&pick| pick < settings.detonation_chances.len())
                    .unwrap_or(settings.detonation_chances.len() - 1);
                let chance_dice = world.scenario_rng.random_double();
                detonate = settings.detonation_chances[chance_index] >= chance_dice;
            }
            if detonate {
                detonate_warhead(
                    world,
                    run,
                    rules,
                    overlay_registry,
                    detonation,
                    settings,
                    &warheads,
                    selected,
                    target_id,
                    invoker,
                    source_house,
                    default_damage,
                    out,
                    under_attack_events,
                );
            }
        }
    } else {
        for index in 0..warheads.len() {
            let mut detonate = true;
            if !settings.detonation_chances.is_empty() {
                let chance_index = index.min(settings.detonation_chances.len() - 1);
                detonate =
                    settings.detonation_chances[chance_index] >= world.scenario_rng.random_double();
            }
            if detonate {
                detonate_warhead(
                    world,
                    run,
                    rules,
                    overlay_registry,
                    detonation,
                    settings,
                    &warheads,
                    Some(index),
                    target_id,
                    invoker,
                    source_house,
                    default_damage,
                    out,
                    under_attack_events,
                );
            }
        }
    }
}

/// `GeneralUtils::ChooseOneWeighted` (`Utilities/GeneralUtils.cpp:108-127`):
/// single-precision running sums; `dice < sum2 / sum` picks the first match.
pub(crate) fn choose_one_weighted(dice: f64, weights: &[i32]) -> Option<usize> {
    let mut sum = 0.0_f32;
    for &weight in weights {
        sum += weight as f32;
    }
    let mut sum2 = 0.0_f32;
    for (index, &weight) in weights.iter().enumerate() {
        sum2 += weight as f32;
        if dice < f64::from(sum2 / sum) {
            return Some(index);
        }
    }
    None
}

/// The per-index override lists share native's "last entry past the end"
/// rule and their defaults (weapon `Damage`, full detonation).
fn entry<T: Copy>(list: &[T], index: usize, default: T) -> T {
    if list.is_empty() {
        return default;
    }
    list[index.min(list.len() - 1)]
}

#[allow(clippy::too_many_arguments)]
fn detonate_warhead(
    world: &mut Simulation,
    run: &mut ReceiverRun<'_>,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    detonation: &ProjectileDetonation,
    settings: &crate::rules::phobos_extra_warheads::PhobosExtraWarheads,
    warheads: &[(&str, &crate::rules::warhead_type::WarheadType)],
    selected: Option<usize>,
    target_id: Option<u64>,
    invoker: Option<u64>,
    source_house: Option<InternedId>,
    default_damage: i32,
    out: &mut CombatEmit,
    under_attack_events: &mut Vec<UnderAttackEvent>,
) {
    let Some(index) = selected.filter(|&index| index < warheads.len()) else {
        return;
    };
    let (name, extra) = warheads[index];
    // `IsHealthInThreshold`/`IsVeterancyInThreshold`/`IsInvokerAllowed` on
    // the bullet's target only; unlike `CanAffectTarget` this gate has no
    // `EffectsRequireVerses` split or Verses test.
    if let Some(target) = target_id.and_then(|id| world.substrate.entities.get(id))
        && !warhead_triggers_allow(
            extra,
            target,
            world.interner.object_type(target.type_ref(), rules),
            invoker,
            rules
                .phobos_globals
                .affects_invoker_only_ignore_invoker_state,
        )
    {
        return;
    }
    let damage = entry(&settings.damage_overrides, index, default_damage);
    let full = entry(&settings.full_detonation, index, true);
    let warhead_ref = world.interner.intern(name);
    if full {
        // `WarheadTypeExt::DetonateAt`: a bullet with no weapon type,
        // exploded at the same impact.
        let packet = ProjectileDetonation {
            projectile_id: u64::MAX,
            source_id: detonation.source_id,
            target: detonation.target,
            impact: detonation.impact,
            payload: ProjectilePayload::new(damage, warhead_ref, None, extra.bright)
                .with_source_house(source_house),
            reason: ProjectileDetonationReason::ExtraWarhead,
        };
        let commit = commit_projectiles(world, run, &[packet], rules, overlay_registry);
        out.effects.append(commit.effects);
        under_attack_events.extend(commit.under_attack_events);
    } else {
        let (nested, mut pings, _crit_active, _area_result) = apply_area_damage_in_run_with_target(
            world,
            run,
            rules,
            overlay_registry,
            detonation.impact,
            damage,
            extra,
            true,
            (detonation.source_id, source_house, warhead_ref),
            target_id,
        );
        out.effects.append(nested);
        under_attack_events.append(&mut pings);
    }
}
