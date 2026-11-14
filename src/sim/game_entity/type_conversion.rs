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
use crate::sim::world::{FrameEffects, Simulation, UninitContext};

impl Simulation {
    /// Installed Ares TechnoClass_Update1004B320..1004B39C, hooked at6F9E50:
    /// sample Object+8C before GetCell5F6960 and Cell+EC, then reuse the one
    /// in-place conversion owner. A self pointer must not reset health,
    /// attachments, spotlight or tracking on every AI visit.
    pub(crate) fn update_ares_surface_conversion(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(source) = self.entity_object_type(actor, rules) else {
            return;
        };
        if source.surface_conversion(true).is_none() && source.surface_conversion(false).is_none() {
            return;
        }
        let water = if actor.on_bridge() {
            false
        } else {
            let Some(terrain) = self.resolved_terrain.as_ref() else {
                // Map-less component scenes have no CellClass to sample.
                return;
            };
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            let location = ground_pose::object_location(actor, Some(terrain));
            let cell = ground_pose::query_object_cell(&cells, location);
            matches!(cells.land_type(cell), 2 | 6)
        };
        let Some(target) = source.surface_conversion(water) else {
            return;
        };
        let Some(destination) = rules.object_in_category(target.category, &target.id) else {
            return;
        };
        if std::ptr::eq(source, destination) {
            return;
        }
        let target = target.clone();
        self.convert_techno_type(id, &target, rules, effects);
    }

    /// A bare Type store (Unit `+0x6C4`) with Health at the new type's
    /// Strength (`+0x6C`): Ares' visceroid merge
    /// (`UnitClass_UpdatePosition_Visceroid @ 1004C530`) writes nothing else,
    /// unlike [`Self::convert_techno_type`]. The Type facts VERA caches, which
    /// native reads from the Type, follow; house tracking is left as native
    /// leaves it.
    pub(crate) fn store_techno_type(
        &mut self,
        id: u64,
        destination: &crate::rules::object_type::ObjectType,
        rules: &RuleSet,
    ) {
        let new_type = self.interner.intern(&destination.id);
        // The new type is a new look for the presentation's atlas check.
        self.substrate.entities.note_look(id);
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return;
        };
        actor.type_ref = new_type;
        actor.is_voxel = GameEntity::object_uses_voxel(&destination.id, destination, rules);
        actor.refresh_type_projection(Some(destination), rules);
        actor.health.set(destination.strength);
    }

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
        // Native draw readers follow the new Type pointer at once; the
        // presentation's atlas check reads the look log for the new model.
        self.substrate.entities.note_look(id);
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
        // Convert.Script, Convert.Water or Convert.Land between types that
        // differ in them (retail sets none); the surface keys fire on each
        // shoreline crossing. Effect: the object keeps the old type's values;
        // converting a transport is the visible case, keeping its old capacity.
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
        true
    }
}
