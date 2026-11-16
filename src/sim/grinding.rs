//! Phobos `BuildingExt::CanGrindTechno` and `DoGrindingExtras`
//! (`Ext/Building/Body.cpp:228-290`), consumed by the hooks in
//! `Ext/Building/Hooks.Grinding.cpp`.
//!
//! ## Dependency rules
//! - sim/ may depend on rules/, map/ and util/; never on render/, ui/, audio/
//!   or net/.

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::{FrameEffects, Simulation};

impl Simulation {
    /// `BuildingExt::CanGrindTechno` (`Ext/Building/Body.cpp:228`): the
    /// building type must grind, the victim must be an Infantry or Unit the
    /// type's absorb flags accept, the owner/alliance switches must allow it,
    /// and the type allow/disallow lists must admit the victim.
    ///
    /// `Grinding.AllowTypes`/`DisallowTypes` read as `ValueableVector<
    /// TechnoTypeClass*>`: a token that does not name a registered type was
    /// dropped at read, so only names that resolve count toward the list.
    pub(crate) fn can_grind_techno(
        &self,
        building_id: u64,
        techno_id: u64,
        rules: &RuleSet,
    ) -> bool {
        let (Some(building), Some(techno)) = (
            self.substrate.entities.get(building_id),
            self.substrate.entities.get(techno_id),
        ) else {
            return false;
        };
        let (Some(building_type), Some(techno_type)) = (
            self.object_type(building.type_ref(), rules),
            self.object_type(techno.type_ref(), rules),
        ) else {
            return false;
        };
        if !building_type.grinding
            || !matches!(
                techno.category(),
                EntityCategory::Infantry | EntityCategory::Unit
            )
        {
            return false;
        }
        if (building_type.infantry_absorb || building_type.unit_absorb)
            && (techno.category() == EntityCategory::Infantry && !building_type.infantry_absorb
                || techno.category() == EntityCategory::Unit && !building_type.unit_absorb)
        {
            return false;
        }
        let settings = &building_type.phobos_grinding;
        let owners_match = building.owner() == techno.owner();
        if owners_match && !settings.allow_owner {
            return false;
        }
        if !owners_match
            && crate::sim::combat::combat_weapon::is_ally_by_object(
                Some(&self.house_alliances),
                &self.interner,
                building.owner(),
                techno.owner(),
            )
            && !settings.allow_allies
        {
            return false;
        }
        let resolves = |name: &str| {
            rules
                .object(name)
                .is_some_and(|object| object.id == techno_type.id)
        };
        if settings
            .allow_types
            .iter()
            .any(|name| rules.object(name).is_some())
            && !settings.allow_types.iter().any(|name| resolves(name))
        {
            return false;
        }
        if settings.disallow_types.iter().any(|name| resolves(name)) {
            return false;
        }
        true
    }

    /// `BuildingExt::DoGrindingExtras` (`Ext/Building/Body.cpp:264`): every
    /// grinder refund accumulates on the building; once the type's
    /// `Grinding.Weapon=` is due (`CurFrame >= last + ROF`) and the
    /// accumulated refund reaches `Grinding.Weapon.RequiredCredits=`, the
    /// weapon detonates on the building itself and the accumulator clears. A
    /// type `Grinding.Sound=` plays at the victim's coordinates.
    ///
    /// Returns whether the type sound played: the caller then skips the stock
    /// `[AudioVisual] EnterGrinderSound=` for this visit
    /// (`Hooks.Grinding.cpp:149-166`, `:184-193`).
    pub(crate) fn do_grinding_extras(
        &mut self,
        building_id: u64,
        victim_id: u64,
        refund: i32,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(building) = self.substrate.entities.get(building_id) else {
            return false;
        };
        let Some(object) = self.object_type(building.type_ref(), rules) else {
            return false;
        };
        let settings = object.phobos_grinding.clone();
        let weapon_name = settings.weapon.clone();
        let weapon = weapon_name
            .as_deref()
            .and_then(|name| rules.weapon(name))
            .cloned();
        let current_frame = self.session.binary_frame() as i32;
        let accumulated = building
            .phobos_grinding_weapon_accumulated_credits()
            .wrapping_add(refund);
        let impact = crate::sim::movement::ground_pose::object_get_coords(
            building,
            self.resolved_terrain.as_ref(),
        );
        let fire = weapon.as_ref().is_some_and(|weapon| {
            building
                .phobos_grinding_weapon_last_fired_frame()
                .wrapping_add(weapon.rof)
                <= current_frame
                && accumulated >= settings.weapon_required_credits
        });
        let building = self
            .substrate
            .entities
            .get_mut(building_id)
            .expect("the grinder stays represented");
        building.set_phobos_grinding_weapon_accumulated_credits(if fire { 0 } else { accumulated });
        // Phobos `DoGrindingExtras` opens with `AccumulatedIncome += refund`
        // (`Ext/Building/Body.cpp:266-271`) for the `DisplayIncome` label.
        let income = building.phobos_accumulated_income().wrapping_add(refund);
        building.set_phobos_accumulated_income(income);
        if fire {
            building.set_phobos_grinding_weapon_last_fired_frame(current_frame);
        }
        if let Some(weapon) = weapon.filter(|_| fire) {
            let (warhead, projectile, bright) = (
                weapon
                    .warhead
                    .as_deref()
                    .map(|name| self.interner.intern(name)),
                weapon
                    .projectile
                    .as_deref()
                    .map(|name| self.interner.intern(name)),
                weapon.bright,
            );
            let Some(warhead) = warhead else {
                return self.grinding_sound(building_id, victim_id, rules);
            };
            let weapon_ref = self.interner.intern(&weapon.id);
            let detonation = crate::sim::projectile::ProjectileDetonation {
                projectile_id: building_id,
                source_id: building_id,
                target: crate::sim::projectile::ProjectileTarget::Entity(building_id),
                impact: crate::sim::projectile::ProjectileCoord::new(impact.x, impact.y, impact.z),
                payload: crate::sim::projectile::ProjectilePayload::new(
                    weapon.damage,
                    warhead,
                    projectile,
                    bright,
                )
                .with_installed_weapon(Some(weapon_ref)),
                reason: crate::sim::projectile::ProjectileDetonationReason::AttachEffectShot,
            };
            self.commit_attach_effect_detonations(
                std::slice::from_ref(&detonation),
                rules,
                registry,
                effects,
            );
        }
        self.grinding_sound(building_id, victim_id, rules)
    }

    /// Play the type's `Grinding.Sound=` at the victim's coordinates.
    fn grinding_sound(&mut self, building_id: u64, victim_id: u64, rules: &RuleSet) -> bool {
        let Some(sound) = self
            .substrate
            .entities
            .get(building_id)
            .and_then(|building| self.object_type(building.type_ref(), rules))
            .and_then(|object| object.phobos_grinding.sound.clone())
        else {
            return false;
        };
        let Some(victim) = self.substrate.entities.get(victim_id) else {
            return false;
        };
        let coordinate = crate::sim::movement::ground_pose::object_get_coords(
            victim,
            self.resolved_terrain.as_ref(),
        );
        self.sound_events
            .emit(crate::sim::world::SimSoundEvent::voc_at_world_coord(
                sound,
                crate::sim::anim_class::AnimWorldCoord {
                    x: coordinate.x,
                    y: coordinate.y,
                    z: coordinate.z,
                },
            ));
        true
    }
}

#[cfg(test)]
#[path = "grinding_tests.rs"]
mod tests;
