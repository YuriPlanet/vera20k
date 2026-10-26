//! Same-object Ares ConvertTechno10043650..10043960, from installed Ares
//! instructions. The same actor, linked team membership and cargo survive;
//! an active temporal chain is released through its normal idle owner first.

use super::GameEntity;
use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::DeliveryType;
use crate::sim::components::DriveCoord;
use crate::sim::conversion_health::ConversionHealth;
use crate::sim::house_tracking::HouseTracking;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::{FacingClass, ground_pose};
use crate::sim::world::{FrameEffects, LifecycleOutput, Simulation, UninitContext};

impl Simulation {
    pub(crate) fn convert_techno_type(
        &mut self,
        id: u64,
        target: &DeliveryType,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        //1004366B..100436C7 admits Infantry15/type16, Unit1/type40 and
        //Aircraft2/type3, and rejects cross-class pointers before any writes.
        if !matches!(
            actor.category(),
            EntityCategory::Infantry | EntityCategory::Unit | EntityCategory::Aircraft
        ) || ObjectCategory::from(actor.category()) != target.category
        {
            return false;
        }
        let Some(destination) = rules.object_in_category(target.category, &target.id) else {
            return false;
        };
        let old_type = actor.type_ref();
        let Some(source) = self.entity_object_type(actor, rules) else {
            return false;
        };
        let old_locomotor = source.locomotor;
        if actor.temporal.is_warping_someone() {
            self.temporal_clear_linked_list(id, rules, effects);
        }
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|actor| !actor.lifecycle.in_limbo())
        {
            self.house_removed_from_game(id, rules, effects);
        }
        Self::update_house_tracking(
            &mut self.substrate,
            &mut self.houses,
            id,
            HouseTracking::remove_tracking,
        );

        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        let health = ConversionHealth::capture_in_place(actor, source, destination);
        let new_type = self.interner.intern(&destination.id);
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .expect("retained conversion actor");
        actor.type_ref = new_type;
        actor.is_voxel = GameEntity::object_uses_voxel(&destination.id, destination, rules);
        actor.refresh_type_projection(Some(destination), rules);
        // RESIDUAL: construction alone builds some Type-derived state, and
        // nothing here refreshes it: passenger capacity (PassengerCargo from
        // Passengers=/SizeLimit=), the SHP vehicle Animation component, the
        // Phobos shield, the gunner weapon and voxel recoil. Trigger: an Ares
        // Convert.Script between types that differ in them (retail sets none).
        // Effect: the object keeps the old type's values; converting a
        // transport is the visible case, keeping its old capacity.
        crate::sim::miner::Miner::change_type_binding(&mut actor.miner, destination);
        health.apply(actor);
        Self::update_house_tracking(
            &mut self.substrate,
            &mut self.houses,
            id,
            HouseTracking::add_tracking,
        );
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|actor| !actor.lifecycle.in_limbo())
        {
            self.house_added_to_game(id, rules);
        }
        if let Some(owner) = self.substrate.entities.owner_of(id)
            && let Some(house) = self.houses.get_mut(&owner)
        {
            house.request_tech_recheck();
        }

        // Ext+9D is IsOperated's no-operator fast-path cache (10045440),
        // not Deactivated or DriverKilled. VERA has no such cache to clear.
        self.reset_self_attach_effect_for_type_change(id, old_type, rules);
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor.recompute_ares_attached_stats(rules, &self.interner);
        }
        self.destroy_spotlight_for_parent(
            id,
            UninitContext::with_rules(rules).with_effects(effects),
        );
        self.construct_spotlight(id, rules);
        let frame = self.session.binary_frame();
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        if let Some(ammo) = actor.ammo_mut() {
            ammo.change_type_capacity(destination.ammo);
        }
        // Conversion writes both native FacingClass rates, even on Infantry;
        // the constructor's special Infantry ROT127 does not run here.
        actor.body_facing.set_rot(destination.turret_rot);
        // 10043839 writes +0x3B4 unconditionally, but only a turreted Unit or
        // an Aircraft reads its Secondary. Keep construction's rule so a
        // turretless destination aims its body, not a phantom turret.
        if GameEntity::type_has_secondary_facing(destination, actor.category()) {
            let seed = actor.body_facing.destination();
            actor
                .barrel_facing
                .get_or_insert_with(|| FacingClass::new(seed, 0))
                .set_rot(destination.secondary_turn_rate());
        } else {
            actor.barrel_facing = None;
        }
        if let Some(locomotor) = actor.locomotor.as_mut() {
            locomotor.change_type_binding(old_locomotor, destination, frame);
        } else {
            // Legacy zero-Speed actors may lack their native Foot instance.
            // Reuse the same constructor/link owner rather than fabricate one.
            actor.locomotor = Some(LocomotorState::from_object_type(destination, frame));
        }
        let coords = ground_pose::object_get_coords(actor, self.resolved_terrain.as_ref());
        if destination.balloon_hover
            && destination.deploy_to_land
            && destination.locomotor == LocomotorKind::Jumpjet
            && old_locomotor != LocomotorKind::Jumpjet
        {
            self.jumpjet_move_to(
                id,
                DriveCoord {
                    x: coords.x,
                    y: coords.y,
                    z: coords.z,
                },
                rules,
            );
        }
        if old_type != new_type {
            // Native draw readers immediately follow the new Type pointer.
            // VERA's derived atlas cache must observe the same change even
            // though the entity ID, count and Display membership stay fixed.
            self.lifecycle_outputs
                .emit(LifecycleOutput::ObjectTypeChanged { stable_id: id });
        }
        true
    }
}
