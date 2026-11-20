//! Phobos `TechnoExt::ApplyInterceptor`
//! (`Ext/Techno/Body.Update.cpp:33-137`), the `TechnoClass_AI` early-update
//! hook at `0x6F9E50`: an interceptor TechnoType scans the projectile store
//! for an enemy, interceptable Bullet inside its guard range and sets it as
//! its target.
//!
//! ## RNG draws
//! - None in the scan: it reads the frame counter, range, armor versus and
//!   house filters only. Native's TargetingDelay phase adds the techno's
//!   `RandomFactor`, itself drawn once per techno by
//!   `TechnoExt::InitializeState` (`Ext/Techno/Hooks.cpp:292`,
//!   `RandomRanged(0, 15)`). VERA models neither, so the phase is fixed and
//!   the Phobos Scenario stream is one construction draw ahead; recorded as
//!   a residual in the doc.
//!
//! ## Dependency rules
//! - Part of sim/; uses the shared projectile store, target setter, house
//!   filter and building mission owner.

use crate::rules::mission_data::MissionType;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::projectile::intercepted_status;
use crate::sim::world::{FrameEffects, Simulation};

#[cfg(all(test, tests_sim_world_techno))]
#[path = "interceptor_tests.rs"]
mod tests;

impl Simulation {
    /// `TechnoExt::ApplyInterceptor` for one Techno visit.
    pub(crate) fn apply_interceptor(
        &mut self,
        id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let Some(settings) = object.phobos_interceptor.clone() else {
            return;
        };
        // `(CurrentFrame + RandomFactor) % TargetingDelay == 0`; the
        // RandomFactor term is not modeled (see the module note).
        let frame = self.session.binary_frame();
        if i64::from(frame) % i64::from(settings.targeting_delay) != 0 {
            return;
        }
        if self.projectiles.is_empty() {
            return;
        }
        // `IsBurrowedState`.
        if entity.locomotor.as_ref().is_some_and(|locomotor| {
            locomotor.layer() == crate::sim::movement::locomotor::MovementLayer::Underground
        }) {
            return;
        }
        // `IsArmed()`: the interceptor weapon slot resolves through the
        // shared GetWeapon owner, so a building answers an armed upgrade's or
        // its firing occupant's record exactly as its FireAt call will.
        let Some(weapon) = crate::sim::combat::combat_weapon::get_weapon(
            entity,
            object,
            settings.weapon.max(0),
            &self.substrate.entities,
            rules,
            &self.interner,
        ) else {
            return;
        };
        let Some(warhead) = rules.weapon_warhead(weapon) else {
            return;
        };
        // An existing target must be a Bullet whose status has Locked; any
        // other target returns.
        match entity.attack_target.target() {
            None => {}
            Some(TargetKind::Bullet(target)) => {
                if self.projectiles.get(target).is_some_and(|bullet| {
                    bullet.intercepted_status() & intercepted_status::LOCKED == 0
                }) {
                    return;
                }
            }
            Some(_) => return,
        }
        let is_building = entity.category() == crate::map::entities::EntityCategory::Structure;
        if is_building
            && matches!(
                entity.mission.current().known(),
                Some(MissionType::Selling | MissionType::Construction)
            )
        {
            return;
        }

        let rank = i32::from(entity.veterancy.rank());
        let guard_range = i64::from(settings.guard_range_for(rank));
        let minimum_range = i64::from(settings.minimum_guard_range_for(rank));
        let guard_sq = guard_range.wrapping_mul(guard_range);
        let minimum_sq = minimum_range.wrapping_mul(minimum_range);
        let cylindrical = settings
            .guard_range_is_cylindrical
            .unwrap_or(rules.phobos_globals.interceptor_guard_range_is_cylindrical);
        let owner = entity.owner();
        let origin = position_world_coord(&entity.position);

        let mut optional_target = None;
        let mut target_bullet = None;
        for (&bullet_id, bullet) in self.projectiles.iter() {
            let Some(kind) = bullet
                .payload()
                .projectile_type()
                .and_then(|type_id| rules.projectile(self.interner.resolve(type_id)))
            else {
                continue;
            };
            if !kind
                .interceptable
                .unwrap_or(rules.phobos_globals.projectile_interceptable)
            {
                continue;
            }
            let status = bullet.intercepted_status();
            let targeted_or_locked =
                status & (intercepted_status::TARGETED | intercepted_status::LOCKED) != 0;
            if optional_target.is_some() && targeted_or_locked {
                continue;
            }
            let point = bullet.position();
            let z = if cylindrical { origin.z } else { point.z };
            let dx = i64::from(point.x.wrapping_sub(origin.x));
            let dy = i64::from(point.y.wrapping_sub(origin.y));
            let dz = i64::from(z.wrapping_sub(origin.z));
            let distance_sq = dx
                .wrapping_mul(dx)
                .wrapping_add(dy.wrapping_mul(dy))
                .wrapping_add(dz.wrapping_mul(dz));
            if distance_sq > guard_sq || distance_sq < minimum_sq {
                continue;
            }
            if let Some(armor) = kind.armor
                && f64::from_bits(warhead.interaction(armor).multiplier().bits()) == 0.0
            {
                continue;
            }
            let bullet_owner = self
                .substrate
                .entities
                .get(bullet.source_id())
                .map(|source| source.owner())
                .or_else(|| bullet.phobos_firer_house());
            let Some(bullet_owner) = bullet_owner else {
                continue;
            };
            if !crate::sim::combat::can_target_house(
                settings.can_target_houses,
                owner,
                bullet_owner,
                &self.houses,
                &self.house_alliances,
                &self.interner,
            ) {
                continue;
            }
            if optional_target.is_none() && targeted_or_locked {
                optional_target = Some(bullet_id);
                continue;
            }
            target_bullet = Some(bullet_id);
            break;
        }
        let Some(target_bullet) = target_bullet.or(optional_target) else {
            return;
        };

        let _ = self.assign_target_represented(
            id,
            Some(TargetKind::Bullet(target_bullet)),
            rules,
            frame_effects,
        );
        // Buildings skip the idle-to-attack transition: QueueMission(Attack,
        // false) then NextMission().
        if is_building {
            super::building_missions::queue_and_commence(self, id, MissionType::Attack, rules);
        }
    }
}

impl Simulation {
    /// Phobos `BulletClass_AI`'s interceptor block (`0x4666F7`) and the
    /// `Inviso` interceptor skip (`0x4668BD`), run before the stock AI prefix:
    /// a Targeted bullet locks an armorless target, an Intercepted bullet
    /// detonates (when `DetonateOnInterception`) and retires, and an Inviso
    /// interceptor closes on and detonates at its prey.
    ///
    /// Returns whether the bullet retired and the caller must skip its stock
    /// AI this visit.
    pub(crate) fn bullet_interceptor_pretick(
        &mut self,
        id: u64,
        rules: &RuleSet,
        ctx: super::ObjectAiCtx<'_>,
    ) -> bool {
        use crate::sim::projectile::{ProjectileDetonationReason, detonation};

        let Some(bullet) = self.projectiles.get(id) else {
            return true;
        };
        let status = bullet.intercepted_status();
        let prey = bullet.interceptor_target();
        let position = bullet.position();
        let interceptor_bullet = bullet.interceptor_type().is_some();

        if status & intercepted_status::TARGETED != 0
            && let Some(target) = prey
            && let Some(target_bullet) = self.projectiles.get(target)
            && !target_bullet
                .payload()
                .projectile_type()
                .and_then(|type_id| rules.projectile(self.interner.resolve(type_id)))
                .is_some_and(|kind| kind.armor.is_some())
        {
            self.projectiles
                .add_phobos_intercepted_status(target, intercepted_status::LOCKED);
        }

        if status & intercepted_status::INTERCEPTED != 0 {
            if let Some(target) = prey {
                self.projectiles
                    .clear_phobos_intercepted_status(target, intercepted_status::LOCKED);
            }
            let detonate = self
                .projectiles
                .get(id)
                .is_some_and(|bullet| bullet.detonate_on_interception());
            if detonate {
                let detonation = self.projectiles.get(id).map(|bullet| {
                    detonation(
                        bullet,
                        bullet.position(),
                        ProjectileDetonationReason::ReachedTarget,
                    )
                });
                if let Some(detonation) = detonation {
                    self.commit_logic_projectile_detonations(
                        rules,
                        ctx.overlay_registry,
                        &[detonation],
                        ctx.effects,
                    );
                }
            }
            let source = self.projectiles.get(id).map(|bullet| bullet.source_id());
            let installed_weapon = self
                .projectiles
                .get(id)
                .and_then(|bullet| bullet.payload().installed_weapon());
            let retired = self.retire_non_entity_object(
                id,
                crate::sim::world::UninitContext::new(rules).with_effects(ctx.effects),
            );
            debug_assert!(retired, "an Intercepted bullet is stored");
            // `LimboLaunch`'s self-damage tail: the owner is in limbo and its
            // firing weapon has LimboLaunch.
            let limbo_launch = installed_weapon
                .and_then(|weapon| rules.weapon(self.interner.resolve(weapon)))
                .is_some_and(|weapon| weapon.limbo_launch);
            if let Some(source) = source
                && limbo_launch
                && self
                    .substrate
                    .entities
                    .get(source)
                    .is_some_and(|owner| owner.lifecycle.in_limbo())
            {
                let health = self
                    .substrate
                    .entities
                    .get(source)
                    .map_or(0, |owner| owner.health.current());
                if let Some(owner) = self.substrate.entities.get_mut(source) {
                    owner
                        .position
                        .put_location(crate::sim::components::DriveCoord {
                            x: position.x,
                            y: position.y,
                            z: position.z,
                        });
                }
                let c4 = self.interner.intern(&rules.bridge_warheads.c4_name);
                let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                    source,
                    health.wrapping_mul(2),
                    0,
                    crate::sim::combat::RAD_NO_ATTACKER,
                    None,
                    c4,
                    crate::sim::combat::ReceiverCallFlags {
                        ignore_defenses: true,
                        arg6: false,
                    },
                );
                self.commit_direct_damage_events(
                    rules,
                    ctx.overlay_registry,
                    &[event],
                    ctx.effects,
                );
            }
            return true;
        }

        // `0x4668BD`: an Inviso interceptor bullet jumps straight to the
        // detonate path. `Inviso=` bullets are placed on the target
        // coordinate at Fire, so it detonates at its own position; a
        // non-Inviso interceptor detonates only once the 1/8-cell direct
        // gate already holds.
        if !interceptor_bullet {
            return false;
        }
        let Some(prey) = prey else {
            return false;
        };
        let Some(prey_bullet) = self.projectiles.get(prey) else {
            self.projectiles.set_phobos_interceptor_target(id, None);
            return false;
        };
        let target = prey_bullet.position();
        let invis = self
            .projectiles
            .get(id)
            .and_then(|bullet| bullet.payload().projectile_type())
            .and_then(|type_id| rules.projectile(self.interner.resolve(type_id)))
            .is_some_and(|kind| kind.inviso);
        let dx = i64::from(target.x.wrapping_sub(position.x));
        let dy = i64::from(target.y.wrapping_sub(position.y));
        let dz = i64::from(target.z.wrapping_sub(position.z));
        let distance_sq = dx.wrapping_mul(dx) + dy.wrapping_mul(dy) + dz.wrapping_mul(dz);
        if !invis && distance_sq > 32 * 32 {
            return false;
        }
        let detonation = self
            .projectiles
            .get(id)
            .map(|bullet| detonation(bullet, position, ProjectileDetonationReason::ReachedTarget));
        if let Some(detonation) = detonation {
            self.commit_logic_projectile_detonations(
                rules,
                ctx.overlay_registry,
                &[detonation],
                ctx.effects,
            );
        }
        let retired = self.retire_non_entity_object(
            id,
            crate::sim::world::UninitContext::new(rules).with_effects(ctx.effects),
        );
        debug_assert!(retired, "an interceptor bullet is stored");
        true
    }
}
