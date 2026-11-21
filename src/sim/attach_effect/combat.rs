//! AttachEffect combat hooks on the simulation: the receiver armor multiplier
//! and hit animations, the unkillable clamp, weapon requirements, reflect,
//! revenge and death weapons, and attaching or removing effects from warheads.

use crate::rules::attach_effect::{AttachEffectType, ExpireWeaponCondition};
use crate::rules::phobos_shields::AffectedHouse;
use crate::sim::world::Simulation;

use super::{
    AttachEffectList, AttachEffectParams, AttachEffectWeaponShot, AttachPolicy, AttachSource,
    affects_house, can_attach_effect, paired, scale_i32,
};

#[cfg(feature = "phobos")]
impl Simulation {
    /// Phobos `TechnoExt::CalculateArmorMultipliers` at the receiver hook
    /// (`0x00701966`, `hitAnim = true`): the cached unrestricted product, then
    /// one `RandomDouble` per active restricted effect with the chance, house
    /// and warhead gates; every applied effect multiplies and, when it lists a
    /// `HitAnim`, picks one and spawns it on the target.
    ///
    /// Native draws each chance before its gates and the pick before the anim
    /// constructor, so a HitAnim's own constructor draws stay between the
    /// following effects' chance draws; this loop keeps that order.
    ///
    /// RESIDUAL: the shield pre-stage still divides with the unrestricted
    /// product only, and this roll runs before the receiver's LaserFence,
    /// bridge-hut, zero-health and null-warhead early returns where native
    /// would not reach the armor step.
    pub(crate) fn attach_effect_armor_multiplier(
        &mut self,
        target_id: u64,
        warhead: Option<crate::sim::intern::InternedId>,
        source_house: Option<crate::sim::intern::InternedId>,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> f64 {
        let Some(target) = self.substrate.entities.get(target_id) else {
            return 1.0;
        };
        let stats = *target.phobos_stat_multipliers();
        if !stats.has_restricted_armor {
            return stats.armor;
        }
        let target_owner = target.owner();
        let position = crate::sim::movement::ground_pose::position_world_coord(&target.position);
        let world = crate::sim::anim_class::AnimWorldCoord {
            x: position.x,
            y: position.y,
            z: position.z,
        };
        let effects = rules.attach_effect_types().types();
        // Instance positions keep the attach order without holding the entity
        // borrow, so the per-instance `ArmorMultiplierTimer` can be read and
        // restarted between the other state and RNG writes.
        let restricted: Vec<(usize, usize)> = target
            .phobos_attach_effects()
            .instances()
            .iter()
            .enumerate()
            .filter(|(_, instance)| instance.is_active(instance.is_self_owned()))
            .filter(|(_, instance)| {
                effects
                    .get(instance.type_index)
                    .is_some_and(|effect| effect.restricted_armor_multiplier())
            })
            .map(|(index, instance)| (index, instance.type_index))
            .collect();
        if restricted.is_empty() {
            return stats.armor;
        }
        let frame = self.session.binary_frame() as i32;
        let mut multiplier = stats.armor;
        for (instance_index, type_index) in restricted {
            let Some(effect) = effects.get(type_index) else {
                continue;
            };
            // `attachEffect->ArmorMultiplierTimer.InProgress()` skips the
            // effect before its chance draw (`TechnoHooks.cpp:552`).
            let timer = self
                .substrate
                .entities
                .get(target_id)
                .and_then(|target| {
                    target
                        .phobos_attach_effects()
                        .instances()
                        .get(instance_index)
                })
                .map_or_else(crate::sim::timer::CdTimer::default, |instance| {
                    instance.armor_multiplier_timer
                });
            if timer.remaining(frame) > 0 {
                continue;
            }
            // `type->ArmorMultiplier_Chance < random.RandomDouble()`: the draw
            // happens for every restricted effect, whatever the chance value.
            let chance = self.scenario_rng.random_double();
            if effect.armor_multiplier_chance < chance {
                continue;
            }
            if !affects_house(
                effect.armor_multiplier_affects_house,
                source_house,
                target_owner,
                &self.houses,
                &self.house_alliances,
                &self.interner,
            ) {
                continue;
            }
            if let Some(warhead) = warhead.map(|id| self.interner.resolve(id)) {
                if effect
                    .armor_multiplier_disallow_warheads
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(warhead))
                {
                    continue;
                }
                if !effect.armor_multiplier_allow_warheads.is_empty()
                    && !effect
                        .armor_multiplier_allow_warheads
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case(warhead))
                {
                    continue;
                }
            }
            multiplier *= effect.armor_multiplier;
            // The real-hit arm starts the per-instance delay before the hit
            // animation (`TechnoHooks.cpp:573..579`).
            if effect.armor_multiplier_delay > 0
                && let Some(instance) =
                    self.substrate
                        .entities
                        .get_mut(target_id)
                        .and_then(|target| {
                            target
                                .phobos_attach_effects_mut()
                                .instances
                                .get_mut(instance_index)
                        })
            {
                instance
                    .armor_multiplier_timer
                    .start(frame, effect.armor_multiplier_delay);
            }
            if !effect.armor_multiplier_hit_anim.is_empty() {
                let pick = self
                    .scenario_rng
                    .next_range_i32_inclusive(0, effect.armor_multiplier_hit_anim.len() as i32 - 1);
                let name = &effect.armor_multiplier_hit_anim[pick as usize];
                self.spawn_attach_effect_hit_anim(target_id, name, world, target_owner, rules);
            }
        }
        multiplier
    }

    /// Phobos' `Unkillable` arm of the ReceiveDamage wrapper:
    /// `MapClass::GetTotalDamage(input, WH, target armor, distance)` reaching
    /// the target's health leaves `Health` and `EstimatedHealth` at 1 and the
    /// receiver packet at 0. `WarheadTypeExt::CanKill` is not ported, so only
    /// the effect's flag opens the arm. Returns whether it applied.
    pub(crate) fn apply_unkillable_clamp(
        &mut self,
        target_id: u64,
        event: &crate::sim::combat::EntityDamageEvent,
        input: i32,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> bool {
        let Some(target) = self.substrate.entities.get(target_id) else {
            return false;
        };
        // With the Wasm module `Unkillable` modifiers composed on.
        if !target.effect_unkillable() || target.health.current() <= 0 || input == 0 {
            return false;
        }
        let Some(warhead) = self.interner.warhead_type(event.warhead_ref, rules) else {
            return false;
        };
        let Some(object) = self.interner.object_type(target.type_ref(), rules) else {
            return false;
        };
        let source_house = event.source_house.or_else(|| {
            self.substrate
                .entities
                .get(event.attacker_id)
                .map(|attacker| attacker.owner())
        });
        if !crate::sim::shield::can_target_house(warhead, source_house, target.owner(), self) {
            return false;
        }
        let total = crate::sim::combat::damage::kernel::apply_warhead_damage(
            input,
            warhead.cell_spread_f64,
            warhead.percent_at_max_f64,
            warhead.interaction(object.armor_id()).multiplier(),
            event.distance_leptons,
            self.session.no_damage(),
            rules.combat_damage.max_damage,
        );
        if total < target.health.current() {
            return false;
        }
        let target = self
            .substrate
            .entities
            .get_mut(target_id)
            .expect("the gated target still exists");
        target.health.set(1);
        target.estimated_health.reset(1);
        true
    }

    /// `AnimExt::CreateRandomAnim`'s creation arm: one anim at the target's
    /// coordinates, owned by the target and attached to it.
    fn spawn_attach_effect_hit_anim(
        &mut self,
        target: u64,
        name: &str,
        world: crate::sim::anim_class::AnimWorldCoord,
        house: crate::sim::intern::InternedId,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
        let type_name = self.interner.intern(name);
        let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
            draw_flags: 0x600,
            owner: Some(target),
            ..crate::sim::components::AnimClassSpawnDescriptor::new(
                type_name, rx, ry, sub_x, sub_y, z,
            )
        };
        match self.spawn_anim_at_world(rules, descriptor, world) {
            Ok(anim) => {
                self.substrate.set_anim_owner_house(anim, house);
            }
            Err(error) => log::debug!(
                "attach effect hit animation [{name}] on {target} did not construct: {error}"
            ),
        }
    }
}

#[cfg(feature = "phobos")]
impl Simulation {
    /// `WeaponTypeExt::HasRequiredAttachedEffects` (`src/Ext/WeaponType/Body.cpp:11`):
    /// a weapon carrying no AE list always allows the shot. Otherwise the
    /// effects of the target (or, with `AttachEffect.CheckOnFirer`, of the
    /// firer) must satisfy the disallowed and required lists in native order,
    /// skipping effects attached by the same invoker and warhead when
    /// `AttachEffect.IgnoreFromSameSource` is set.
    pub(crate) fn weapon_attach_effects_allow(
        &self,
        weapon: &crate::rules::weapon_type::WeaponType,
        target: Option<&crate::sim::game_entity::GameEntity>,
        firer: Option<&crate::sim::game_entity::GameEntity>,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> bool {
        if !weapon.has_attach_effect_gates() {
            return true;
        }
        let techno = if weapon.attach_effect_check_on_firer {
            firer
        } else {
            target
        };
        let Some(techno) = techno else {
            return true;
        };
        let catalog = rules.attach_effect_types();
        let effects = catalog.types();
        let warhead = weapon.warhead.as_deref();
        let invoker = firer.map(|entity| entity.stable_id());
        let ignore = weapon.attach_effect_ignore_from_same_source;
        let list = techno.phobos_attach_effects();

        let resolve = |names: &[String]| -> Vec<usize> {
            names
                .iter()
                .filter_map(|name| catalog.index_of(name))
                .collect()
        };
        let refused = |types: &[usize],
                       require_all: bool,
                       min: &[i32],
                       max: &[i32],
                       houses: AffectedHouse| {
            // Native checks the matched instance's invoker house only when a
            // firer is known and the list narrows houses.
            let firer_owner = firer.map(|entity| entity.owner());
            let techno_owner = techno.owner();
            let need_house = firer.is_some() && houses != AffectedHouse::ALL;
            list.has_attached_effects(
                types,
                require_all,
                ignore,
                weapon.attach_effect_same_source_only,
                invoker,
                warhead,
                effects,
                &self.interner,
                min,
                max,
                |self_owned, instance_house| {
                    if !need_house {
                        return true;
                    }
                    // A self-owned effect's invoker is the techno itself.
                    let instance_house = if self_owned {
                        Some(techno_owner)
                    } else {
                        instance_house
                    };
                    match (firer_owner, instance_house) {
                        (Some(source), Some(target)) => crate::sim::combat::can_target_house(
                            houses,
                            source,
                            target,
                            &self.houses,
                            &self.house_alliances,
                            &self.interner,
                        ),
                        _ => false,
                    }
                },
            )
        };
        let disallowed = resolve(&weapon.attach_effect_disallowed_types);
        if !disallowed.is_empty()
            && refused(
                &disallowed,
                !weapon.attach_effect_disallowed_any,
                &weapon.attach_effect_disallowed_min_counts,
                &weapon.attach_effect_disallowed_max_counts,
                weapon.attach_effect_disallowed_houses,
            )
        {
            return false;
        }
        let disallowed_groups = AttachEffectList::types_with_groups(
            effects.len(),
            &weapon.attach_effect_disallowed_groups,
            effects,
        );
        if !disallowed_groups.is_empty()
            && refused(
                &disallowed_groups,
                !weapon.attach_effect_disallowed_any,
                &weapon.attach_effect_disallowed_min_counts,
                &weapon.attach_effect_disallowed_max_counts,
                weapon.attach_effect_disallowed_houses,
            )
        {
            return false;
        }
        let required = resolve(&weapon.attach_effect_required_types);
        if !required.is_empty()
            && !refused(
                &required,
                !weapon.attach_effect_required_any,
                &weapon.attach_effect_required_min_counts,
                &weapon.attach_effect_required_max_counts,
                weapon.attach_effect_required_houses,
            )
        {
            return false;
        }
        let required_groups = AttachEffectList::types_with_groups(
            effects.len(),
            &weapon.attach_effect_required_groups,
            effects,
        );
        if !required_groups.is_empty()
            && !refused(
                &required_groups,
                !weapon.attach_effect_required_any,
                &weapon.attach_effect_required_min_counts,
                &weapon.attach_effect_required_max_counts,
                weapon.attach_effect_required_houses,
            )
        {
            return false;
        }
        true
    }

    /// One `WeaponTypeExt::DetonateAt`-style shot carried by an effect: a
    /// bullet of the weapon, created at the target and detonated there.
    pub(super) fn attach_effect_weapon_detonation(
        &mut self,
        shot: &AttachEffectWeaponShot,
        target_id: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Option<crate::sim::projectile::ProjectileDetonation> {
        let weapon = rules.weapon(&shot.weapon)?;
        let warhead_name = weapon.warhead.as_deref()?;
        let impact = {
            let target = self.substrate.entities.get(target_id)?;
            crate::sim::movement::ground_pose::object_get_coords(
                target,
                self.resolved_terrain.as_ref(),
            )
        };
        let warhead_ref = self.interner.intern(warhead_name);
        let weapon_ref = self.interner.intern(&weapon.id);
        let projectile = weapon
            .projectile
            .as_deref()
            .map(|name| self.interner.intern(name));
        Some(crate::sim::projectile::ProjectileDetonation {
            projectile_id: target_id,
            source_id: shot.invoker.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
            target: crate::sim::projectile::ProjectileTarget::Entity(target_id),
            impact: crate::sim::projectile::ProjectileCoord::new(impact.x, impact.y, impact.z),
            payload: crate::sim::projectile::ProjectilePayload::new(
                weapon.damage,
                warhead_ref,
                projectile,
                weapon.bright,
            )
            .with_installed_weapon(Some(weapon_ref))
            .with_source_house(shot.house),
            reason: crate::sim::projectile::ProjectileDetonationReason::AttachEffectShot,
        })
    }

    /// `WarheadTypeExt::DetonateAt` (`src/Ext/WarheadType/Body.cpp:107`): a
    /// bullet with no weapon type, carrying `warhead` and `damage`.
    fn attach_effect_warhead_detonation(
        &mut self,
        warhead_name: &str,
        damage: i32,
        target_id: u64,
        invoker: Option<u64>,
        house: Option<crate::sim::intern::InternedId>,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Option<(
        crate::sim::projectile::ProjectileDetonation,
        crate::sim::intern::InternedId,
    )> {
        let warhead = rules.warhead(warhead_name)?;
        let impact = {
            let target = self.substrate.entities.get(target_id)?;
            crate::sim::movement::ground_pose::object_get_coords(
                target,
                self.resolved_terrain.as_ref(),
            )
        };
        let warhead_ref = self.interner.intern(warhead_name);
        Some((
            crate::sim::projectile::ProjectileDetonation {
                projectile_id: target_id,
                source_id: invoker.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
                target: crate::sim::projectile::ProjectileTarget::Entity(target_id),
                impact: crate::sim::projectile::ProjectileCoord::new(impact.x, impact.y, impact.z),
                payload: crate::sim::projectile::ProjectilePayload::new(
                    damage,
                    warhead_ref,
                    None,
                    warhead.bright,
                )
                .with_source_house(house),
                reason: crate::sim::projectile::ProjectileDetonationReason::AttachEffectShot,
            },
            warhead_ref,
        ))
    }

    /// Detonate the given shots through the ordinary bullet receiver chain.
    pub(crate) fn commit_attach_effect_detonations(
        &mut self,
        detonations: &[crate::sim::projectile::ProjectileDetonation],
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut crate::sim::world::FrameEffects<'_>,
    ) {
        if detonations.is_empty() {
            return;
        }
        let mut run =
            crate::sim::combat::world_receiver::ReceiverRun::default().with_effects(effects);
        let commit = crate::sim::combat::world_receiver::commit_projectiles(
            self,
            &mut run,
            detonations,
            rules,
            overlay_registry,
        );
        let navigation = run.finish();
        let _ = self.absorb_noncombat_damage_effects(
            rules,
            commit.effects,
            commit.under_attack_events,
            navigation,
            effects,
        );
    }

    /// Phobos `TechnoClass_ReceiveDamage_ReflectDamage` (`0x701E18`): every
    /// active effect with `ReflectDamage` may deal the received, defense-
    /// divided damage back to the attacker, under its chance, warhead,
    /// multiplier/override and house gates. A reflected hit whose warhead is
    /// already marked `Reflected` never reflects again. `attack` is the
    /// post-divide value the native hook reads from `args->Damage`.
    pub(crate) fn apply_attach_effect_reflect_damage(
        &mut self,
        target_id: u64,
        attacker_id: u64,
        source_house: Option<crate::sim::intern::InternedId>,
        warhead_ref: crate::sim::intern::InternedId,
        damage: i32,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut crate::sim::world::FrameEffects<'_>,
    ) {
        if damage <= 0 || *self.reflected_warhead == Some(warhead_ref) {
            return;
        }
        let Some(warhead) = self.interner.warhead_type(warhead_ref, rules) else {
            return;
        };
        let info = &warhead.attach_effect;
        // Without a source object the native hook has nothing to hit. The
        // list stays borrowed while the walk below draws from the scenario
        // stream, a field of its own.
        let Some(list) = self
            .substrate
            .entities
            .get(target_id)
            .map(|target| target.phobos_attach_effects())
        else {
            return;
        };
        let victim_owner = self
            .substrate
            .entities
            .get(target_id)
            .map(|target| target.owner())
            .expect("the gated target still exists");
        let Some(attacker_owner) = source_house.or_else(|| {
            self.substrate
                .entities
                .get(attacker_id)
                .map(|attacker| attacker.owner())
        }) else {
            return;
        };
        let suppress = info.suppress_reflect_damage;
        let suppress_by_type = !info.suppress_reflect_damage_types.is_empty();
        let suppress_by_group = !info.suppress_reflect_damage_groups.is_empty();
        // `(!suppress || suppressByType || suppressByGroup)` gates the loop,
        // so a bare `SuppressReflectDamage` suppresses every effect before any
        // chance draw.
        if suppress && !suppress_by_type && !suppress_by_group {
            return;
        }
        let effects_table = rules.attach_effect_types().types();
        let mut actions: Vec<(
            bool,
            i32,
            Option<u64>,
            Option<crate::sim::intern::InternedId>,
            &str,
            usize,
            i32,
        )> = Vec::new();
        let frame = self.session.binary_frame() as i32;
        for (instance_index, instance) in list.instances().iter().enumerate() {
            if !instance.is_active(instance.is_self_owned()) {
                continue;
            }
            let Some(effect) = effects_table.get(instance.type_index) else {
                continue;
            };
            if !effect.reflect_damage {
                continue;
            }
            // `attachEffect->ReflectDamageTimer.InProgress()` skips before the
            // chance draw (`Hooks.ReceiveDamage.cpp:429`).
            if effect.reflect_damage_delay > 0 && instance.reflect_damage_timer.remaining(frame) > 0
            {
                continue;
            }
            if effect.reflect_damage_chance < self.scenario_rng.random_double() {
                continue;
            }
            if suppress {
                if suppress_by_type
                    && info
                        .suppress_reflect_damage_types
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case(&effect.name))
                {
                    continue;
                }
                if suppress_by_group
                    && effect.groups.iter().any(|group| {
                        info.suppress_reflect_damage_groups
                            .iter()
                            .any(|suppressed| suppressed.eq_ignore_ascii_case(group))
                    })
                {
                    continue;
                }
            }
            let reflected = effect
                .reflect_damage_override
                .unwrap_or_else(|| scale_i32(damage, effect.reflect_damage_multiplier));
            let warhead_name = effect
                .reflect_damage_warhead
                .as_deref()
                .unwrap_or(&rules.bridge_warheads.c4_name);
            if warhead_name.is_empty() || rules.warhead(warhead_name).is_none() {
                continue;
            }
            let affects = effect.reflect_damage_affects_house;
            let shot = if effect.reflect_damage_use_invoker_as_owner {
                let invoker = instance.source.and_then(|source| source.invoker);
                let house = instance.source.and_then(|source| source.invoker_house);
                let invoker_house = invoker.and_then(|entity| {
                    self.substrate
                        .entities
                        .get(entity)
                        .map(|entity| entity.owner())
                });
                if invoker.is_some()
                    && affects_house(
                        affects,
                        invoker_house,
                        attacker_owner,
                        &self.houses,
                        &self.house_alliances,
                        &self.interner,
                    )
                {
                    Some((invoker, invoker_house, true))
                } else if affects_house(
                    affects,
                    house,
                    attacker_owner,
                    &self.houses,
                    &self.house_alliances,
                    &self.interner,
                ) {
                    Some((None, house, true))
                } else {
                    None
                }
            } else if affects_house(
                affects,
                Some(victim_owner),
                attacker_owner,
                &self.houses,
                &self.house_alliances,
                &self.interner,
            ) {
                Some((Some(target_id), Some(victim_owner), false))
            } else {
                None
            };
            if let Some((invoker, house, detonate)) = shot {
                actions.push((
                    detonate,
                    reflected,
                    invoker,
                    house,
                    warhead_name,
                    instance_index,
                    effect.reflect_damage_delay,
                ));
            }
        }

        for (detonate, reflected, invoker, house, warhead_name, instance_index, delay) in actions {
            let previous = self
                .reflected_warhead
                .replace(self.interner.intern(warhead_name));
            if detonate {
                if let Some((detonation, _)) = self.attach_effect_warhead_detonation(
                    warhead_name,
                    reflected,
                    attacker_id,
                    invoker,
                    house,
                    rules,
                ) {
                    self.commit_attach_effect_detonations(
                        &[detonation],
                        rules,
                        overlay_registry,
                        effects,
                    );
                }
            } else {
                let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                    attacker_id,
                    reflected,
                    0,
                    invoker.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
                    house,
                    self.interner.intern(warhead_name),
                    crate::sim::combat::ReceiverCallFlags {
                        ignore_defenses: false,
                        arg6: false,
                    },
                );
                self.commit_direct_damage_receiver(rules, overlay_registry, event, effects);
            }
            // The reflection starts the per-instance delay before the
            // reflected flag clears (`Hooks.ReceiveDamage.cpp:461..464`).
            if delay > 0
                && let Some(instance) =
                    self.substrate
                        .entities
                        .get_mut(target_id)
                        .and_then(|target| {
                            target
                                .phobos_attach_effects_mut()
                                .instances
                                .get_mut(instance_index)
                        })
            {
                instance.reflect_damage_timer.start(frame, delay);
            }
            *self.reflected_warhead = previous;
        }
    }

    /// Phobos `TechnoExt::ApplyRevengeWeapon` (`src/Ext/Techno/WeaponHelpers.cpp:254`):
    /// once a victim dies from a sourced hit, every active effect carrying
    /// `RevengeWeapon` fires that weapon at the attacker, under the effect's
    /// house gate and invoker ownership.
    pub(crate) fn apply_attach_effect_revenge_weapons(
        &mut self,
        victim_id: u64,
        attacker_id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut crate::sim::world::FrameEffects<'_>,
    ) {
        let Some(victim) = self.substrate.entities.get(victim_id) else {
            return;
        };
        let Some(attacker) = self.substrate.entities.get(attacker_id) else {
            return;
        };
        let victim_owner = victim.owner();
        let attacker_owner = attacker.owner();
        let table = rules.attach_effect_types().types();
        let mut shots = Vec::new();
        for (instance, effect) in victim.phobos_attach_effects().active_effects(table) {
            if effect.revenge_weapon.is_none() {
                continue;
            }
            let Some(shot) = instance.revenge_weapon_shot(effect, victim_id, victim_owner) else {
                continue;
            };
            if !affects_house(
                effect.revenge_weapon_affects_house,
                shot.house,
                attacker_owner,
                &self.houses,
                &self.house_alliances,
                &self.interner,
            ) {
                continue;
            }
            shots.push(shot);
        }
        let detonations: Vec<_> = shots
            .iter()
            .filter_map(|shot| self.attach_effect_weapon_detonation(shot, attacker_id, rules))
            .collect();
        self.commit_attach_effect_detonations(&detonations, rules, overlay_registry, effects);
    }

    /// Phobos `TechnoClass_ReceiveDamage_AttachEffectExpireWeapon`
    /// (`0x702050`, `src/Ext/Techno/Hooks.ReceiveDamage.cpp:350`): when a techno
    /// dies, every attached instance whose type has `ExpireWeapon` and
    /// `TriggerOn=death` fires that weapon at the dying object's coordinates,
    /// before the native death weapon. A cumulative type with
    /// `CumulativeOnlyOnce` fires once; the scan follows attach order.
    pub(crate) fn apply_attach_effect_death_weapons(
        &mut self,
        dead_id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut crate::sim::world::FrameEffects<'_>,
    ) {
        let Some(victim) = self.substrate.entities.get(dead_id) else {
            return;
        };
        let owner_house = victim.owner();
        let table = rules.attach_effect_types().types();
        let mut once = std::collections::HashSet::new();
        let mut shots = Vec::new();
        // The native hook walks every instance, not just active ones.
        for instance in victim.phobos_attach_effects().instances() {
            let Some(effect) = table.get(instance.type_index) else {
                continue;
            };
            if effect.expire_weapon.is_none()
                || !effect
                    .expire_weapon_trigger_on
                    .contains(ExpireWeaponCondition::DEATH)
            {
                continue;
            }
            if effect.cumulative
                && effect.expire_weapon_cumulative_only_once
                && !once.insert(instance.type_index)
            {
                continue;
            }
            if let Some(shot) = instance.expire_weapon_shot_identity(
                effect,
                ExpireWeaponCondition::DEATH,
                dead_id,
                owner_house,
            ) {
                shots.push(shot);
            }
        }
        let detonations: Vec<_> = shots
            .iter()
            .filter_map(|shot| self.attach_effect_weapon_detonation(shot, dead_id, rules))
            .collect();
        self.commit_attach_effect_detonations(&detonations, rules, overlay_registry, effects);
    }

    /// `AttachEffectClass::RemoveAllOfType` (`AttachEffectClass.cpp:1131`): the
    /// list-position bounded removal pass. A cumulative type below
    /// `min_count` is kept entirely; at most `max_count` instances are
    /// removed; a recreatable instance resets instead and stays. Every removed
    /// instance contributes its `ExpireWeapon.TriggerOn=remove` shot.
    fn remove_attach_effect_type(
        &mut self,
        list: &mut AttachEffectList,
        type_index: usize,
        min_count: i32,
        max_count: i32,
        owner: u64,
        owner_house: crate::sim::intern::InternedId,
        effects: &[AttachEffectType],
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Vec<AttachEffectWeaponShot> {
        let Some(effect) = effects.get(type_index) else {
            return Vec::new();
        };
        let mut shots = Vec::new();
        // `stackCount` keeps the active count read before any removal and is
        // -1 for a non-cumulative type.
        let stack = if effect.cumulative {
            list.active_count(type_index)
        } else {
            -1
        };
        if min_count > 0 && stack > -1 && min_count > stack {
            return shots;
        }
        let ignore_cumulative_count_check = stack != 1;
        let mut detached = 0;
        let mut index = 0;
        while index < list.instances.len() {
            if max_count > 0 && detached >= max_count {
                break;
            }
            if list.instances[index].type_index != type_index {
                index += 1;
                continue;
            }
            detached += 1;
            let active_count = list.active_count(type_index);
            if let Some(shot) = list.instances[index].expire_weapon_shot(
                effect,
                ExpireWeaponCondition::REMOVE,
                owner,
                owner_house,
                active_count,
                ignore_cumulative_count_check,
            ) {
                shots.push(shot);
            }
            if list.instances[index].reset_if_recreatable() {
                index += 1;
                continue;
            }
            if let Some(anim) = list.instances[index].anim.take() {
                self.destroy_anim_with_context(anim, rules);
            }
            list.instances.remove(index);
            if !effect.cumulative {
                break;
            }
        }
        shots
    }

    /// Phobos `WarheadTypeExt::ApplyAttachEffects` (`src/Ext/WarheadType/Detonate.cpp:726`),
    /// reached per target before its damage: attach every `AttachTypes` entry,
    /// then remove `RemoveTypes` and every type carrying a `RemoveGroups`
    /// group. `invoker` and `source_house` are the detonation's attacker and
    /// decided house; the caller, `crate::sim::combat::crit::detonate`, owns
    /// `DetonateOnOneUnit`'s target gate.
    pub(crate) fn apply_warhead_attach_effects(
        &mut self,
        target_id: u64,
        warhead_ref: crate::sim::intern::InternedId,
        invoker: Option<u64>,
        source_house: Option<crate::sim::intern::InternedId>,
        rules: &crate::rules::ruleset::RuleSet,
        effects: &mut crate::sim::world::FrameEffects<'_>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let Some(warhead) = self.interner.warhead_type(warhead_ref, rules) else {
            return;
        };
        let info = &warhead.attach_effect;
        if info.is_empty() {
            return;
        }
        let Some(target) = self.substrate.entities.get(target_id) else {
            return;
        };
        let target_owner = target.owner();
        // `CreateAndAttach` gating reads the live target; snapshot the pair
        // before the attach list mutably borrows the entity.
        let target_category = target.category();
        let target_invulnerability = target.invulnerability;
        let target_type_ref = target.type_ref();
        let Some(target_type) = self.interner.object_type(target_type_ref, rules) else {
            return;
        };
        let catalog = rules.attach_effect_types();
        let table = catalog.types();
        let policy = AttachPolicy {
            replace_longer_duration: info
                .replace_longer_duration
                .unwrap_or_else(|| rules.attach_effect_globals().replace_longer_duration),
            cumulative_source_max_count: info.cumulative_source_max_count,
            cumulative_refresh_all: info.cumulative_refresh_all,
            cumulative_refresh_all_on_attach: info.cumulative_refresh_all_on_attach,
            cumulative_refresh_same_source_only: info.cumulative_refresh_same_source_only,
        };
        let source = Some(AttachSource {
            invoker,
            source: Some(u64::from(warhead_ref.index())),
            invoker_house: source_house,
        });
        let Some(target) = self.substrate.entities.get_mut(target_id) else {
            return;
        };
        let mut list = std::mem::take(target.phobos_attach_effects_mut());
        let frame = self.session.binary_frame();
        for (index, name) in info.attach_types.iter().enumerate() {
            let Some(type_index) = catalog.index_of(name) else {
                continue;
            };
            let effect = &table[type_index];
            if !can_attach_effect(
                target_category.into(),
                target_invulnerability.as_ref(),
                target_type,
                effect,
                frame,
            ) {
                continue;
            }
            let params = AttachEffectParams {
                // `GetAttachParams(index, false)`: the warhead carries the
                // duration override but no delay lists, and its delay is -1.
                duration_override: paired(&info.duration_overrides, index, 0),
                delay: -1,
                initial_delay: 0,
                recreation_delay: -1,
            };
            list.attach(type_index, params, effect, policy, source);
        }
        let mut shots = Vec::new();
        for (position, name) in info.remove_types.iter().enumerate() {
            let Some(type_index) = catalog.index_of(name) else {
                continue;
            };
            let min_count = paired(&info.cumulative_remove_min_counts, position, -1);
            let max_count = paired(&info.cumulative_remove_max_counts, position, -1);
            shots.extend(self.remove_attach_effect_type(
                &mut list,
                type_index,
                min_count,
                max_count,
                target_id,
                target_owner,
                table,
                rules,
            ));
        }
        let group_types =
            AttachEffectList::types_with_groups(table.len(), &info.remove_groups, table);
        for (position, type_index) in group_types.into_iter().enumerate() {
            let min_count = paired(&info.cumulative_remove_min_counts, position, -1);
            let max_count = paired(&info.cumulative_remove_max_counts, position, -1);
            shots.extend(self.remove_attach_effect_type(
                &mut list,
                type_index,
                min_count,
                max_count,
                target_id,
                target_owner,
                table,
                rules,
            ));
        }
        if let Some(target) = self.substrate.entities.get_mut(target_id) {
            *target.phobos_attach_effects_mut() = list;
            target.recalculate_phobos_stat_multipliers(table);
        }
        let detonations: Vec<_> = shots
            .iter()
            .filter_map(|shot| self.attach_effect_weapon_detonation(shot, target_id, rules))
            .collect();
        self.commit_attach_effect_detonations(&detonations, rules, overlay_registry, effects);
    }
}
