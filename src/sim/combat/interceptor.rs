//! Phobos `WarheadTypeExt::InterceptBullets`
//! (`Ext/WarheadType/Detonate.cpp:633-672`) and `BulletExt::InterceptBullet`
//! (`Ext/Bullet/Body.cpp:17-90`): the interceptor bullet's warhead entry
//! resolves nearby or directly targeted interceptable bullets before any of
//! its own detonation effects.
//!
//! ## Dependency rules
//! - Part of sim/; the projectile store is the only bullet authority.

use crate::rules::ruleset::RuleSet;
use crate::rules::warhead_type::WarheadType;
use crate::sim::projectile::{ProjectileDetonation, intercepted_status};
use crate::sim::world::{FrameEffects, Simulation};

impl Simulation {
    /// `WarheadTypeExt::Detonate`'s interceptor head, called before create-gap,
    /// cluster and area work. Only a detonating bullet carrying
    /// `InterceptorTechnoType` runs it.
    pub(crate) fn intercept_bullets(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        detonation: &ProjectileDetonation,
        warhead: &WarheadType,
        mut effects: FrameEffects<'_>,
    ) {
        let Some(interceptor) = self.projectiles.get(detonation.projectile_id) else {
            return;
        };
        let Some(interceptor_type_id) = interceptor.interceptor_type() else {
            return;
        };
        let Some(settings) = self
            .interner
            .object_type(interceptor_type_id, rules)
            .and_then(|object| object.phobos_interceptor.clone())
        else {
            return;
        };
        let interceptor_inviso = interceptor
            .payload()
            .projectile_type()
            .and_then(|type_id| rules.projectile(self.interner.resolve(type_id)))
            .is_some_and(|kind| kind.inviso);
        let coords = detonation.impact;

        let targets: Vec<u64> = if warhead.cell_spread_f64 == 0.0 {
            // The direct target only, within 1/8 cell unless the interceptor
            // bullet is Inviso.
            let Some(target) = interceptor.interceptor_target() else {
                return;
            };
            let Some(target_bullet) = self.projectiles.get(target) else {
                return;
            };
            if !self.bullet_type_interceptable(target_bullet, rules) {
                return;
            }
            let point = target_bullet.position();
            if interceptor_inviso || distance_squared(point, coords) <= f64::from(32 * 32) {
                vec![target]
            } else {
                Vec::new()
            }
        } else {
            let threshold = warhead.cell_spread_f64 * warhead.cell_spread_f64 * 65536.0;
            self.projectiles
                .iter()
                .filter_map(|(&id, bullet)| {
                    if !self.bullet_type_interceptable(bullet, rules) {
                        return None;
                    }
                    (distance_squared(bullet.position(), coords) <= threshold).then_some(id)
                })
                .collect()
        };

        for target in targets {
            self.intercept_bullet(
                rules,
                overlay_registry,
                detonation.source_id,
                detonation.projectile_id,
                target,
                &settings,
                warhead,
                &mut effects.reborrow(),
            );
        }
    }

    fn bullet_type_interceptable(
        &self,
        bullet: &crate::sim::projectile::Projectile,
        rules: &RuleSet,
    ) -> bool {
        bullet
            .payload()
            .projectile_type()
            .and_then(|type_id| rules.projectile(self.interner.resolve(type_id)))
            .is_some_and(|kind| {
                kind.interceptable
                    .unwrap_or(rules.phobos_globals.projectile_interceptable)
            })
    }

    /// `BulletExt::InterceptBullet` for one intercepted bullet.
    #[allow(clippy::too_many_arguments)]
    fn intercept_bullet(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        source_id: u64,
        interceptor_id: u64,
        target_id: u64,
        settings: &crate::rules::phobos_interceptor::InterceptorSettings,
        warhead: &WarheadType,
        effects: &mut FrameEffects<'_>,
    ) {
        let Some(target) = self.projectiles.get(target_id) else {
            return;
        };
        let Some(kind) = target
            .payload()
            .projectile_type()
            .and_then(|type_id| rules.projectile(self.interner.resolve(type_id)))
        else {
            return;
        };
        let keep_intact = settings.keep_intact;
        let armor = kind.armor;
        let mut current_strength = target.current_strength();
        let mut intercepted = false;
        match armor {
            None => {
                if !keep_intact {
                    intercepted = true;
                }
            }
            Some(armor) => {
                let versus = warhead.interaction(armor).multiplier();
                if f64::from_bits(versus.bits()) == 0.0 {
                    return;
                }
                let health = self
                    .projectiles
                    .get(interceptor_id)
                    .map_or(0, |bullet| bullet.payload().base_damage);
                // `static_cast<int>(pInterceptor->Health * versus)`.
                let damage = (f64::from(health) * f64::from_bits(versus.bits())) as i32;
                current_strength = current_strength.wrapping_sub(damage);
                if current_strength <= 0 {
                    current_strength = 0;
                    if !keep_intact {
                        intercepted = true;
                    }
                }
            }
        }
        self.projectiles
            .set_phobos_current_strength(target_id, current_strength);
        if intercepted {
            self.projectiles
                .add_phobos_intercepted_status(target_id, intercepted_status::INTERCEPTED);
        }
        let detonate_on_interception = !settings
            .delete_on_intercept
            .unwrap_or(kind.interceptable_delete_on_intercept);
        self.projectiles
            .set_phobos_detonate_on_interception(target_id, detonate_on_interception);

        let override_name = settings
            .weapon_override
            .clone()
            .or_else(|| kind.interceptable_weapon_override.clone());
        let Some(override_name) = override_name else {
            return;
        };
        let Some(weapon) = rules.weapon(&override_name) else {
            return;
        };
        let cumulative = settings.weapon_cumulative_damage;
        // Native Health is the bullet's damage-carried Health (+0x6C), which
        // VERA keeps in `payload.base_damage`; CurrentStrength is a separate
        // interception pool.
        let carried_health = self
            .projectiles
            .get(target_id)
            .map_or(0, |bullet| bullet.payload().base_damage);
        let new_health = if cumulative {
            carried_health.wrapping_add(weapon.damage)
        } else {
            weapon.damage
        };
        let warhead_id = rules
            .weapon_warhead(weapon)
            .map(|warhead| self.interner.intern(&warhead.id));
        let weapon_id = self.interner.intern(&weapon.id);
        let current_type = self
            .projectiles
            .get(target_id)
            .and_then(|bullet| bullet.payload().projectile_type())
            .map(|type_id| self.interner.resolve(type_id).to_owned());
        let mut replacement_type = None;
        let mut replacement_speed = None;
        let mut new_kind = None;
        // The native compare is between BulletType pointers resolved by the
        // case-insensitive INI lookup, so compare the names that way too.
        if settings.weapon_replace_projectile
            && let Some(projectile) = rules.weapon_projectile(weapon)
            && current_type
                .as_deref()
                .is_none_or(|current| !current.eq_ignore_ascii_case(&projectile.id))
        {
            replacement_type = Some(self.interner.intern(&projectile.id));
            replacement_speed = Some(weapon.speed.clamp(0, i32::from(u16::MAX)) as u16);
            new_kind = Some(projectile);
        }
        self.projectiles.apply_phobos_weapon_override(
            target_id,
            new_health,
            warhead_id,
            weapon.bright,
            weapon_id,
            replacement_type,
            replacement_speed,
        );
        // Phobos `InterceptBullet` (`Ext/Bullet/Body.cpp:68..72`): the new
        // BulletType gets a fresh trail list unless it is `Inviso`.
        if let Some(new_kind) = new_kind {
            let names = (!new_kind.inviso).then_some(new_kind.laser_trail_types.as_slice());
            self.projectiles.reset_phobos_laser_trails(target_id, names);
        }
        // Lose the source's target if the replaced projectile no longer
        // qualifies for interception.
        if let Some(new_kind) = new_kind
            && source_id != 0
        {
            let still_interceptable = new_kind
                .interceptable
                .unwrap_or(rules.phobos_globals.projectile_interceptable)
                && new_kind.armor.is_none_or(|armor| {
                    f64::from_bits(warhead.interaction(armor).multiplier().bits()) != 0.0
                });
            if !still_interceptable {
                let _ = self.assign_target_represented(source_id, None, rules, effects);
            }
        }
    }
}

/// `CoordStruct::DistanceFromSquared`'s double result.
fn distance_squared(
    a: crate::sim::projectile::ProjectileCoord,
    b: crate::sim::projectile::ProjectileCoord,
) -> f64 {
    let dx = f64::from(a.x.wrapping_sub(b.x));
    let dy = f64::from(a.y.wrapping_sub(b.y));
    let dz = f64::from(a.z.wrapping_sub(b.z));
    dx * dx + dy * dy + dz * dz
}
