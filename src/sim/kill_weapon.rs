//! Phobos `TechnoExt::ApplyKillWeapon` (`Ext/Techno/WeaponHelpers.cpp:230`),
//! called from `TechnoClass_ReceiveDamage` (`TechnoClass_ReceiveDamage_
//! RevengeWeapon`, hook `0x702672`) for the hit that killed the object.
//!
//! ## Dependency rules
//! - sim/ may depend on rules/, map/ and util/; never on render/, ui/, audio/
//!   or net/.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::EntityDamageEvent;
use crate::sim::world::{FrameEffects, Simulation};

impl Simulation {
    /// Fire the killing warhead's `KillWeapon=` at the dead object and
    /// `KillWeapon.OnFirer=` at the killing source, through the ordinary
    /// bullet receiver. Both are gated by their own house and target flags
    /// and by the victim type's `SuppressKillWeapons` list; `OnFirer` needs a
    /// live source pointer.
    pub(crate) fn apply_phobos_kill_weapons(
        &mut self,
        dead_id: u64,
        event: &EntityDamageEvent,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(warhead) = rules.warhead(self.interner.resolve(event.warhead_ref)) else {
            return;
        };
        let settings = warhead.phobos_kill_weapon.clone();
        if settings.kill_weapon.is_none() && settings.kill_weapon_on_firer.is_none() {
            return;
        }
        let Some(victim) = self.substrate.entities.get(dead_id) else {
            return;
        };
        let Some(victim_type) = self.object_type(victim.type_ref(), rules) else {
            return;
        };
        let victim_owner = victim.owner();
        let victim_category = victim.category();
        let victim_strange = victim_type.is_vehicle();
        let suppression = victim_type.phobos_kill_weapon_suppress.clone();
        let victim_coords = crate::sim::movement::ground_pose::object_get_coords(
            victim,
            self.resolved_terrain.as_ref(),
        );
        let source = (event.attacker_id != crate::sim::combat::RAD_NO_ATTACKER)
            .then(|| self.substrate.entities.get(event.attacker_id))
            .flatten()
            .map(|source| {
                let coords = crate::sim::movement::ground_pose::object_get_coords(
                    source,
                    self.resolved_terrain.as_ref(),
                );
                (source.stable_id(), source.owner(), coords)
            });
        let source_owner = source.map(|(_, owner, _)| owner).or(event.source_house);

        let mut detonations = Vec::new();
        if let Some(weapon_name) = settings.kill_weapon.as_deref()
            && source_owner.is_none_or(|owner| {
                crate::sim::combat::can_target_house(
                    settings.kill_weapon_affects_house,
                    owner,
                    victim_owner,
                    &self.houses,
                    &self.house_alliances,
                    &self.interner,
                )
            })
            && crate::sim::combat::crit::techno_eligible(
                victim_category,
                settings.kill_weapon_affects_target,
                victim_strange,
            )
            && !suppression.suppresses(weapon_name)
            && let Some(weapon) = rules.weapon(weapon_name)
            && let Some(detonation) = self.weapon_detonation_at(
                weapon,
                dead_id,
                event.attacker_id,
                source_owner,
                [victim_coords.x, victim_coords.y, victim_coords.z],
                weapon.damage,
            )
        {
            detonations.push(detonation);
        }
        if let Some(weapon_name) = settings.kill_weapon_on_firer.as_deref()
            && let Some((source_id, owner, coords)) = source
            && crate::sim::combat::can_target_house(
                settings.kill_weapon_on_firer_affects_house,
                owner,
                victim_owner,
                &self.houses,
                &self.house_alliances,
                &self.interner,
            )
            && crate::sim::combat::crit::techno_eligible(
                victim_category,
                settings.kill_weapon_on_firer_affects_target,
                victim_strange,
            )
            && !suppression.suppresses(weapon_name)
            && let Some(weapon) = rules.weapon(weapon_name)
            && let Some(detonation) = self.weapon_detonation_at(
                weapon,
                source_id,
                source_id,
                None,
                [coords.x, coords.y, coords.z],
                weapon.damage,
            )
        {
            detonations.push(detonation);
        }
        self.commit_attach_effect_detonations(&detonations, rules, registry, effects);
    }

    /// `WeaponTypeExt::DetonateAt(weapon, target, owner, damage, firingHouse)`:
    /// a bullet of the weapon detonating on `target_id` at `impact`. `owner`
    /// is the bullet's owning techno, which can be missing for a kill without
    /// a source. `damage` replaces the weapon's `Damage=` for callers that
    /// override it (Phobos `WarpInWeapon.UseDistanceAsDamage`).
    pub(crate) fn weapon_detonation_at(
        &mut self,
        weapon: &crate::rules::weapon_type::WeaponType,
        target_id: u64,
        owner_id: u64,
        firing_house: Option<crate::sim::intern::InternedId>,
        impact: [i32; 3],
        damage: i32,
    ) -> Option<crate::sim::projectile::ProjectileDetonation> {
        self.weapon_detonation(
            weapon,
            target_id,
            crate::sim::projectile::ProjectileTarget::Entity(target_id),
            owner_id,
            firing_house,
            impact,
            damage,
        )
    }

    /// The coordinate overload of `WeaponTypeExt::DetonateAt(weapon, coords,
    /// owner, damage, firingHouse)`: a bullet detonating at `impact` with no
    /// AbstractClass target (Phobos `DropPod.Weapon.HitLandOnly`).
    pub(crate) fn weapon_detonation_at_point(
        &mut self,
        weapon: &crate::rules::weapon_type::WeaponType,
        owner_id: u64,
        firing_house: Option<crate::sim::intern::InternedId>,
        impact: [i32; 3],
        damage: i32,
    ) -> Option<crate::sim::projectile::ProjectileDetonation> {
        self.weapon_detonation(
            weapon,
            owner_id,
            crate::sim::projectile::ProjectileTarget::None,
            owner_id,
            firing_house,
            impact,
            damage,
        )
    }

    fn weapon_detonation(
        &mut self,
        weapon: &crate::rules::weapon_type::WeaponType,
        projectile_id: u64,
        target: crate::sim::projectile::ProjectileTarget,
        owner_id: u64,
        firing_house: Option<crate::sim::intern::InternedId>,
        impact: [i32; 3],
        damage: i32,
    ) -> Option<crate::sim::projectile::ProjectileDetonation> {
        self.weapon_detonation_with_target(
            weapon,
            projectile_id,
            target,
            owner_id,
            firing_house,
            impact,
            damage,
            crate::sim::projectile::ProjectileDetonationReason::AttachEffectShot,
        )
    }

    /// The same `WeaponTypeExt::DetonateAt` bullet with an explicit
    /// AbstractClass target and reason, for a caller outside this module
    /// (Phobos `FeedbackWeapon`, which detonates on the firer).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn weapon_detonation_with_target(
        &mut self,
        weapon: &crate::rules::weapon_type::WeaponType,
        projectile_id: u64,
        target: crate::sim::projectile::ProjectileTarget,
        owner_id: u64,
        firing_house: Option<crate::sim::intern::InternedId>,
        impact: [i32; 3],
        damage: i32,
        reason: crate::sim::projectile::ProjectileDetonationReason,
    ) -> Option<crate::sim::projectile::ProjectileDetonation> {
        let warhead = weapon
            .warhead
            .as_deref()
            .map(|name| self.interner.intern(name))?;
        let projectile = weapon
            .projectile
            .as_deref()
            .map(|name| self.interner.intern(name));
        let weapon_ref = self.interner.intern(&weapon.id);
        Some(crate::sim::projectile::ProjectileDetonation {
            projectile_id,
            source_id: owner_id,
            target,
            impact: crate::sim::projectile::ProjectileCoord::new(impact[0], impact[1], impact[2]),
            payload: crate::sim::projectile::ProjectilePayload::new(
                damage,
                warhead,
                projectile,
                weapon.bright,
            )
            .with_installed_weapon(Some(weapon_ref))
            .with_source_house(firing_house),
            reason,
        })
    }
}

#[cfg(all(test, tests_sim_h_o))]
#[path = "kill_weapon_tests.rs"]
mod tests;
