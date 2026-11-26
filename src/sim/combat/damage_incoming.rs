//! Wasm module `DamageIncoming` at the Techno receiver's entry
//! (`world_receiver::commit_entity`).

use super::*;
use crate::sim::ext_modules::modifiers::Immunity;
use crate::sim::world::Simulation;

/// Wasm module `DamageIncoming` (RFC 0001 appendix 4.5, preview) at the
/// Techno receiver's entry: the incoming damage continuing into the native
/// flow, or `None` for the native one. One flag check without a decider.
///
/// Not asked for zero incoming damage, IgnoreDefenses hits, Psychedelic or
/// Temporal warheads, or hits the receiver's gates would reject: the
/// subject's `ImmuneToRadiation=`/`ImmuneToPsionicWeapons=`/`ImmuneToPoison=`
/// against such a warhead (or a module's `ImmunityFlags` in their place), an allied attacker for a warhead without
/// `AffectsAllies=`, an active Iron Curtain for positive damage. Residual:
/// Ares' rank immunities and CanTargetHouse, TypeImmune and the bunker and
/// warp gates are not consulted, so such a hit is asked and then rejected
/// natively.
///
/// Each link's record carries its own `previous` as `value` and, as
/// `value2`, that `previous` through the warhead kernel (falloff, the
/// Verses of the armor its trigger event's `id` names, −1 or a non-retail
/// index for the subject's, and `MaxDamage=`); with bit 0 of the event's
/// `value` a positive `previous` first goes through the subject's armor
/// divides, truncated toward zero, without their minimum of 1.
pub(super) fn decide_damage_incoming(
    world: &mut Simulation,
    rules: &RuleSet,
    event: &EntityDamageEvent,
    incoming: i32,
    scenario_no_damage: bool,
    current_tick: u64,
) -> Option<i32> {
    use crate::rules::armor::{ArmorId, RETAIL_ARMOR_NAMES};
    use vera20k_wasm_ext::decide::EVENT_DAMAGE_INCOMING;
    if !world.trigger_runtime.module_decides(EVENT_DAMAGE_INCOMING)
        || incoming == 0
        || event.receiver_flags.ignore_defenses
    {
        return None;
    }
    let warhead = world.interner.warhead_type(event.warhead_ref, rules)?;
    if warhead.psychedelic || warhead.temporal {
        return None;
    }
    let target = world.substrate.entities.get(event.target_id)?;
    let target_type = world.interner.object_type(target.type_ref(), rules);
    let attacker = (event.attacker_id != RAD_NO_ATTACKER)
        .then(|| world.substrate.entities.get(event.attacker_id))
        .flatten();
    let allied_attacker = attacker.is_some_and(|attacker| {
        world
            .house_alliances
            .is_allied(&world.interner, attacker.owner(), target.owner())
    });
    // The receiver's gates, with a module's `ImmunityFlags` over the type's.
    let type_flag = |flag: fn(&crate::rules::object_type::ObjectType) -> bool| {
        target_type.is_some_and(flag)
    };
    let immune = (warhead.radiation
        && target.module_immune(Immunity::Radiation, type_flag(|o| o.immune_to_radiation)))
        || (warhead.psychic_damage
            && target.module_immune(Immunity::Psychic, type_flag(|o| o.immune_to_psionic_weapons)))
        || (warhead.poison
            && target.module_immune(Immunity::Poison, type_flag(|o| o.immune_to_poison)));
    let curtained = incoming > 0
        && crate::sim::superweapon::invulnerability::is_invulnerable(
            target.invulnerability.as_ref(),
            current_tick as u32,
        );
    if immune || curtained || (allied_attacker && !warhead.affects_allies) {
        return None;
    }
    let divisors = super::receiver_divisors(
        target,
        target_type,
        rules,
        &world.interner,
        &world.houses,
        target.phobos_armor_multiplier(),
    );
    let subject_armor = target_type.map_or(ArmorId::NONE, |object| object.armor_id());
    let warhead_id = rules
        .warhead_index(world.interner.resolve(event.warhead_ref))
        .map_or(-1, |index| index as i32);
    let distance = event.distance_leptons;
    let max_damage = rules.combat_damage.max_damage;
    let shape = move |def: &vera20k_wasm_ext::triggers::EventDef, previous: i64| {
        let previous = previous.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        let input = if def.value & 1 != 0 && previous > 0 {
            super::damage::receive::defence_divides_unfloored(previous, &divisors)
        } else {
            previous
        };
        let armor = usize::try_from(def.id)
            .ok()
            .filter(|&index| index < RETAIL_ARMOR_NAMES.len())
            .map_or(subject_armor, ArmorId::from_index);
        let preview = super::damage::kernel::apply_warhead_damage(
            input,
            warhead.cell_spread_f64,
            warhead.percent_at_max_f64,
            warhead.interaction(armor).multiplier(),
            distance,
            scenario_no_damage,
            max_damage,
        );
        (i64::from(previous), i64::from(preview))
    };
    let request = crate::sim::ext_modules::ModuleEvent {
        other: attacker.map_or(0, |_| event.attacker_id),
        id: warhead_id,
        at_subject: true,
        ..crate::sim::ext_modules::ModuleEvent::new(EVENT_DAMAGE_INCOMING, event.target_id)
    };
    world
        .decide_module_event_shaped(rules, &request, i64::from(incoming), &shape, |answer| {
            i32::try_from(answer).ok().map(i64::from)
        })
        .and_then(|answer| i32::try_from(answer).ok())
}
