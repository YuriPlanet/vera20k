//! Same-object Ares ConvertTechno10043650..10043960, from installed Ares
//! instructions. The same actor, linked team membership and cargo survive;
//! an active temporal chain is released through its normal idle owner first.

use super::GameEntity;
use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::DeliveryType;
use crate::sim::components::DriveCoord;
use crate::sim::conversion_health::ConversionHealth;
use crate::sim::house_tracking::HouseTracking;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::{FacingClass, ground_pose};
use crate::sim::world::{FrameEffects, Simulation, UninitContext};

/// Only the live type store can request a fresh projection for a registered actor.
pub(in crate::sim) struct LiveTypeStore(());

impl Simulation {
    /// Installed Ares TechnoClass_Update1004B320..1004B39C, hooked at6F9E50:
    /// sample Object+8C before GetCell5F6960 and Cell+EC, then reuse the one
    /// in-place conversion owner. A self pointer must not reset health,
    /// attachments, spotlight or tracking on every AI visit.
    pub(crate) fn update_ares_surface_conversion(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
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

    /// A bare Type store (Unit `+0x6C4`), unlike [`Self::convert_techno_type`]:
    /// team script actions 44 and 45 (`0x006E94AA`, `0x006E9511`) write
    /// nothing else, and Ares' visceroid merge
    /// (`UnitClass_UpdatePosition_Visceroid @ 1004C530`) only Health besides.
    /// The Type facts VERA caches, which native reads from the Type, follow
    /// ([`Self::replace_live_type`]), the locomotor's included; the locomotor
    /// object itself, house tracking and Health are left as native leaves
    /// them.
    pub(crate) fn store_techno_type(&mut self, id: u64, destination: &ObjectType, rules: &RuleSet) {
        self.replace_live_type(id, destination, rules);
    }

    /// Publish a registered actor's new type and refresh the facts read from
    /// it, the locomotor's type caches included
    /// ([`LocomotorState::rebind_type_caches`]).
    fn replace_live_type(
        &mut self,
        id: u64,
        destination: &ObjectType,
        rules: &RuleSet,
    ) -> Option<&mut GameEntity> {
        let new_type = self.interner.intern(&destination.id);
        // The new type is a new look for the presentation's atlas check.
        self.substrate.entities.note_look(id);
        let actor = self.substrate.entities.get_mut(id)?;
        actor.type_ref = new_type;
        actor.is_voxel = GameEntity::object_uses_voxel(&destination.id, destination, rules);
        actor.refresh_after_type_store(LiveTypeStore(()), destination, rules);
        if let Some(locomotor) = actor.locomotor.as_mut() {
            locomotor.rebind_type_caches(destination);
        }
        Some(actor)
    }

    pub(crate) fn convert_techno_type(
        &mut self,
        id: u64,
        target: &DeliveryType,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
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
        // `TechnoExt::ConvertToType` (`Ext/Techno/Body.cpp:355-362`) refuses a
        // same-type or cross-class conversion before it writes anything.
        if std::ptr::eq(source, destination) {
            return false;
        }
        let old_locomotor = source.locomotor;
        // Phobos `FootExt::UpdateTypeData`'s capture-manager arm
        // (`Ext/Foot/Body.cpp:470-540`) is gated by the OLD type's
        // `Convert.ResetMindControl` (`Nullable`, `[General]` fallback).
        let reset_mind_control = source
            .phobos_convert
            .reset_mind_control
            .unwrap_or(rules.phobos_globals.convert_reset_mind_control);
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
        HouseTracking::unregister_object(&mut self.substrate, &mut self.houses, id);

        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        let health = ConversionHealth::capture_in_place(actor, source, destination);
        let new_type = self.interner.intern(&destination.id);
        let actor = self
            .replace_live_type(id, destination, rules)
            .expect("retained conversion actor");
        actor.type_ref = new_type;
        actor.is_voxel = GameEntity::object_uses_voxel(&destination.id, destination, rules);
        actor.refresh_type_projection(Some(destination), rules);
        // Phobos `FootExt::UpdateTypeData` (`Ext/Foot/Body.cpp:219-221`,
        // called from `TechnoClass::ChangeType` at `Ext/Techno/Body.cpp:366`):
        // a running countdown is dropped when the new type has no timed
        // AutoDeath. The untimed type then never re-arms it.
        if actor.auto_death_timer().start_frame() != crate::sim::timer::PAUSED_START_FRAME
            && destination.phobos_auto_death.after_delay <= 0
        {
            *actor.auto_death_timer_mut() = crate::sim::timer::CdTimer::default();
        }
        // Phobos `FootExt::UpdateTypeData` (`Ext/Foot/Body.cpp:223-225`): the
        // passenger-deletion countdown stops when the new type configures a
        // PassengerDeletion with `Rate <= 0`. A type with no config at all
        // leaves the clock running, as native does.
        if actor.passenger_deletion_timer().start_frame() != crate::sim::timer::PAUSED_START_FRAME
            && destination
                .phobos_passenger_deletion
                .as_ref()
                .is_some_and(|settings| settings.rate <= 0)
        {
            *actor.passenger_deletion_timer_mut() = crate::sim::timer::CdTimer::default();
        }
        // RESIDUAL: construction alone builds some Type-derived state, and
        // nothing here refreshes it: passenger capacity (PassengerCargo from
        // Passengers=/SizeLimit=), the SHP vehicle Animation component, the
        // gunner weapon and voxel recoil. Trigger: an Ares
        // Convert.Script, Convert.Water or Convert.Land between types that
        // differ in them (retail sets none); the surface keys fire on each
        // shoreline crossing. Effect: the object keeps the old type's values;
        // converting a transport is the visible case, keeping its old capacity.
        crate::sim::miner::Miner::change_type_binding(&mut actor.miner, destination);
        health.apply(actor);
        // Phobos `FootExt::UpdateTypeData` (`Ext/Foot/Body.cpp:186-187`):
        // `pShield->ConvertCheck(pCurrentType)`, i.e.
        // `ShieldClass::ConvertCheck` (`New/Entity/ShieldClass.cpp:608-760`)
        // with no old shield. The transfer keys decide whether the shield
        // survives; a surviving one is rebound to the destination type, with
        // its HP rescaled by the strength ratio and its recovery timers
        // rescaled toward the new type's rates.
        self.phobos_shield_convert_check(id, destination, rules);
        // Phobos `FootExt::UpdateTypeData`'s capture-manager arm: rebuild the
        // manager's limits from the new type's weapons.
        if reset_mind_control {
            self.phobos_reset_mind_control_on_convert(
                id,
                destination,
                rules,
                &mut effects.reborrow(),
            );
        }
        HouseTracking::register_object(&mut self.substrate, &mut self.houses, id);
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
        self.destroy_spotlight_for_parent(id, UninitContext::new(rules).with_effects(effects));
        self.construct_spotlight(id, rules);
        let frame = self.session.binary_frame();
        // Conversion writes both native FacingClass rates, even on Infantry;
        // the constructor's special Infantry ROT127 does not run here. The
        // body's goes through a module `ROTMultiplier`.
        self.set_body_rot(id, destination.turret_rot);
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        if let Some(ammo) = actor.ammo_mut() {
            ammo.change_type_capacity(destination.ammo);
        }
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
                .emit(crate::sim::world::LifecycleOutput::ObjectTypeChanged { stable_id: id });
        }
        // Phobos `FootExt::UpdateTypeData` (`Ext/Foot/Body.cpp:722-793`), run
        // by `TechnoExt::ConvertToType` once the instance is rebound
        // (`Ext/Techno/Body.cpp:353` under Ares, `:455` otherwise).
        self.phobos_update_type_data_in_air(id, destination, rules, effects);
        true
    }

    /// Phobos `FootExt::UpdateTypeData`'s in-air arm
    /// (`Ext/Foot/Body.cpp:722-793`).
    ///
    /// A destination that is not an Aircraft and whose locomotor is neither
    /// `Fly` nor `Hover` (those land themselves) either rewrites the running
    /// Jumpjet instance from the new type and, when the body is in the air,
    /// parks it there or re-targets it, or — with no Jumpjet at all — lets an
    /// airborne body fall, as a crash death when the cell below refuses it.
    fn phobos_update_type_data_in_air(
        &mut self,
        id: u64,
        destination: &crate::rules::object_type::ObjectType,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        if destination.category == ObjectCategory::Aircraft
            || matches!(
                destination.locomotor,
                LocomotorKind::Fly | LocomotorKind::Hover
            )
        {
            return;
        }
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        // `pThis->IsInAir() && !pThis->IsAttackedByLocomotor` (`:729`): a
        // Magnetron's lift (`FootClass+0x6AD`) displaces the locomotor, so a
        // held body is not flying on its own and must not be dropped.
        let in_air = crate::sim::movement::air_movement::is_high_flying(
            actor,
            self.resolved_terrain.as_ref(),
            Some((rules, &self.interner)),
        ) && !actor.is_attacked_by_locomotor();
        let frame = self.session.binary_frame();
        // `pJJLoco->Is_Really_Moving_Now()` (`:750`, ILocomotion+0xA8).
        let was_moving = crate::sim::movement::motion_query::is_really_moving_now(
            actor,
            crate::sim::movement::SpeedRules::new(rules, &self.interner, &self.houses),
            frame,
        );
        let coords = ground_pose::object_get_coords(actor, self.resolved_terrain.as_ref());
        if destination.locomotor == LocomotorKind::Jumpjet {
            // `:733-746`: the instance keeps its state, destination and moving
            // byte and takes the new type's `Jumpjet*` block and facing rate.
            let hover_in_place = in_air && destination.balloon_hover;
            if let Some(runtime) = self
                .substrate
                .entities
                .get_mut(id)
                .and_then(|actor| actor.locomotor.as_mut())
                .and_then(|locomotor| locomotor.jumpjet_runtime_mut())
            {
                runtime.phobos_relink_for_type_change(&destination.jumpjet_params);
                if hover_in_place {
                    // `:752-759`: hover without descending or re-targeting.
                    runtime.phobos_hover_in_place(
                        DriveCoord {
                            x: coords.x,
                            y: coords.y,
                            z: coords.z,
                        },
                        was_moving,
                    );
                }
            }
            if in_air && !destination.balloon_hover && !was_moving {
                // `:761-764` `Move_To(pThis->Location)`: an idle airborne
                // instance re-enters the climb at the place it hangs.
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
            return;
        }
        if !in_air {
            return;
        }
        // `:770` Let it go into free fall.
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor.raise_falling_down();
        }
        match self.landing_cell_verdict(id, rules) {
            Some(verdict) if !verdict.clear => {
                // `:774-779` the landing cells refuses the new type: the body
                // lands as a bomb and dies.
                if let Some(actor) = self.substrate.entities.get_mut(id) {
                    actor.begin_bomb_fall();
                }
            }
            verdict => {
                // `:782-785` a null cell (`TryGetCellAt`) takes this branch
                // with `OnBridge` false.
                let on_bridge = verdict.is_some_and(|verdict| verdict.on_bridge);
                if let Some(actor) = self.substrate.entities.get_mut(id) {
                    actor.on_bridge.store(on_bridge);
                    if let Some(locomotor) = actor.locomotor.as_mut() {
                        locomotor.store_ground_layer(on_bridge);
                    }
                    actor.mark_on_parachuted();
                }
            }
        }
        // `:787-791` runs after both landing arms: an Infantry that changed to
        // parachute status force-arms `Sequence::Paradrop` with a deterministic
        // first frame. `0x0051D6F0` is the same `Do_Action` the rest of the
        // class uses, so its own refusals apply (an unchanged `Doing` answers
        // false, `0x0051D70F` refuses a record without frames).
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|actor| actor.category() == EntityCategory::Infantry)
            && let Err(cause) = self.infantry_do_action(
                id,
                crate::sim::movement::infantry_action::DO_PARADROP,
                true,
                rules,
                effects,
            )
        {
            log::debug!("infantry {id} Paradrop PlayAnim: {cause}");
        }
    }

    /// Phobos `FootExt::HealthAutoConvertActions` (`Ext/Foot/Body.cpp:909-927`),
    /// called from the `FootClass_AI` tail (`Ext/Techno/Hooks.cpp:42-47`):
    /// with `Convert.Health` set and the band enabled, convert when
    /// `IsHealthInThreshold` holds.
    pub(crate) fn phobos_health_auto_convert(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        let Some((target_name, above, below, health, strength)) =
            self.substrate.entities.get(id).and_then(|actor| {
                let object = self.entity_object_type(actor, rules)?;
                let settings = &object.phobos_convert_health;
                if !settings.enabled() {
                    return None;
                }
                Some((
                    settings.health.clone()?,
                    settings.above_percent,
                    settings.below_percent,
                    actor.health,
                    object.strength,
                ))
            })
        else {
            return;
        };
        if !health_in_threshold(health, strength, above, below) {
            return;
        }
        let Some(target) = rules.object(&target_name) else {
            return;
        };
        let target = DeliveryType {
            category: target.category,
            id: target.id.clone(),
        };
        self.convert_techno_type(id, &target, rules, effects);
    }

    /// Phobos `FootExt::AmmoAutoConvertActions` (`Ext/Foot/Body.cpp:888-909`),
    /// called from the `FootClass_AI` tail before the health-band arm: with
    /// `Ammo.AutoConvertType` set and a non-empty band, a Foot whose type has
    /// `Ammo=` above zero and whose instance ammo is inside the band converts.
    /// Both bounds default to -1 (unbounded); a reversed pair is an empty band.
    pub(crate) fn phobos_ammo_auto_convert(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        let Some((target_name, min, max, type_ammo, ammo)) =
            self.substrate.entities.get(id).and_then(|actor| {
                let object = self.entity_object_type(actor, rules)?;
                let convert = &object.phobos_convert;
                let target = convert.ammo_target.clone()?;
                Some((
                    target,
                    convert.ammo_min,
                    convert.ammo_max,
                    object.ammo,
                    actor.ammo().map_or(0, |ammo| ammo.current()),
                ))
            })
        else {
            return;
        };
        if min < 0 && max < 0 {
            return;
        }
        // `pTypeExt->OwnerObject()->Ammo <= 0`: the TYPE's ammo capacity.
        if type_ammo <= 0 {
            return;
        }
        if !((min < 0 || ammo >= min) && (max < 0 || ammo <= max)) {
            return;
        }
        let Some(target) = rules.object(&target_name) else {
            return;
        };
        let target = DeliveryType {
            category: target.category,
            id: target.id.clone(),
        };
        self.convert_techno_type(id, &target, rules, effects);
    }

    /// Phobos `TechnoClass_Captured_UpdateTracking @ 0x7015C9`'s Foot arm
    /// (`Ext/House/Hooks.cpp:203-300`): on a human<->computer owner change,
    /// convert through the old type's `Convert.HumanToComputer` /
    /// `Convert.ComputerToHuman`, then run the mission fix when the old
    /// owner was computer-controlled. The caller invokes this before the
    /// house swap, with the old owner still installed.
    pub(crate) fn phobos_convert_on_owner_change(
        &mut self,
        id: u64,
        new_owner: InternedId,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.category() == EntityCategory::Structure {
            return;
        }
        let old_owner = entity.owner();
        let policy = self.session.rule_policy();
        let old_human = self
            .houses
            .get(&old_owner)
            .is_some_and(|house| house.is_controlled_by_human(policy));
        let new_human = self
            .houses
            .get(&new_owner)
            .is_some_and(|house| house.is_controlled_by_human(policy));
        if old_human == new_human {
            return;
        }
        let Some(object) = self.entity_object_type(entity, rules) else {
            return;
        };
        let target_name = if old_human {
            object.phobos_convert.human_to_computer.clone()
        } else {
            object.phobos_convert.computer_to_human.clone()
        };
        // The mission fix reads the OLD type's fields, as native passes the
        // pre-conversion `pType` (`Ext/House/Hooks.cpp:291`).
        let old_resource_gatherer = object.resource_gatherer;
        let old_default_to_guard_area = object.default_to_guard_area;
        if let Some(name) = target_name
            && let Some(target) = rules.object(&name)
        {
            let target = DeliveryType {
                category: target.category,
                id: target.id.clone(),
            };
            self.convert_techno_type(id, &target, rules, effects);
        }
        if !old_human {
            self.phobos_change_owner_mission_fix(
                id,
                old_resource_gatherer,
                old_default_to_guard_area,
                rules,
                registry,
                effects,
            );
        }
        // An Ares game refreshes only a declared prerequisite alternate
        // (both Houses); a Building keeps its independent vanilla tail, and
        // the stock game rechecks both.
        let entity = self.substrate.entities.get(id);
        let declared = entity.is_some_and(|entity| {
            rules.is_ares_prerequisite_alternate(
                entity.category().into(),
                self.interner.resolve(entity.type_ref()),
            )
        });
        let vanilla_building =
            entity.is_some_and(|entity| entity.category() == EntityCategory::Structure);
        if !rules.ares() || declared || vanilla_building {
            self.houses.request_house_tech_tree_recheck(old_owner);
            self.houses.request_house_tech_tree_recheck(new_owner);
        }
    }

    /// Phobos `TechnoExt::ChangeOwnerMissionFix`
    /// (`Ext/Techno/Body.Internal.cpp:234-260`): clear the `+0x68F`/`+0x690`
    /// entry seeks, then unless the Foot still has a radio link or the old
    /// type gathers resources or sits on Harvest/Sleep/Harmless/Repair,
    /// override it to Guard, mark the next scan to drop its target and queue
    /// `AreaGuard`/`Guard` with the immediate flag.
    ///
    /// RESIDUAL: native also clears `ShouldScanForTarget` and
    /// `ShouldGarrisonStructure`; VERA has no owner for either byte.
    fn phobos_change_owner_mission_fix(
        &mut self,
        id: u64,
        old_resource_gatherer: bool,
        old_default_to_guard_area: bool,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) {
        let Some((has_links, mission)) = self.substrate.entities.get(id).map(|entity| {
            (
                entity.seldom.radio_contacts.iter_live().next().is_some(),
                entity.mission.effective().known(),
            )
        }) else {
            return;
        };
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.seldom.building_entry.clear_mission_fix_seeks();
        }
        if has_links || old_resource_gatherer {
            return;
        }
        if matches!(
            mission,
            Some(
                MissionType::Harvest
                    | MissionType::Sleep
                    | MissionType::Harmless
                    | MissionType::Repair
            )
        ) {
            return;
        }
        let now = self.session.binary_frame();
        let _ = self.mission_override_represented(
            id,
            crate::sim::mission::authority::OverridePacket {
                mission: MissionId::from_known(MissionType::Guard),
                combat_target: None,
                destination: None,
            },
            rules,
            effects,
        );
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.passive_scan.ares_lose_target_now();
        }
        let guard = if old_default_to_guard_area {
            MissionType::AreaGuard
        } else {
            MissionType::Guard
        };
        let _ = self.mission_queue_exact(
            id,
            MissionId::from_known(guard),
            1,
            now,
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        );
    }

    /// Phobos `TypeConvertGroup::Convert`
    /// (`New/Type/Affiliated/TypeConvertGroup.cpp:5-40`): the Foot `id`
    /// converts through the first applicable pair. A pair without `To` is
    /// skipped; with a source house, `AffectsHouse` must admit the target's
    /// owner; an empty `From` list converts any Foot, otherwise the first
    /// matching source type does. The scan stops after a conversion.
    pub(crate) fn phobos_apply_type_convert_pairs(
        &mut self,
        id: u64,
        pairs: &[crate::rules::type_convert_group::TypeConvertGroup],
        house: Option<InternedId>,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        if pairs.is_empty() {
            return;
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.category() == EntityCategory::Structure {
            return;
        }
        let target_owner = entity.owner();
        let target_name = self.interner.resolve(entity.type_ref()).to_owned();
        for pair in pairs {
            let Some(to) = &pair.to else {
                continue;
            };
            // Native resolves `To` at parse time; an unknown name leaves the
            // `Nullable` unset and ends the numbered loop
            // (`Utilities/TemplateDef.h:1834-1849`, `Template.h:172-195`).
            if rules.object(to).is_none() {
                break;
            }
            if let Some(house) = house
                && !crate::sim::combat::can_target_house(
                    pair.affected_houses,
                    house,
                    target_owner,
                    self.houses(),
                    self.house_alliances(),
                    self.interner(),
                )
            {
                continue;
            }
            // `ValueableVector<TechnoTypeClass*>` drops unresolvable tokens, so
            // an all-unknown `From` list is empty and converts any Foot
            // (`TemplateDef.h:1728-1744`); a partly resolvable list matches
            // only its known names.
            let mut from = pair.from.iter().filter(|from| rules.object(from).is_some());
            let convert = match from.next() {
                None => true,
                Some(first) => {
                    first.eq_ignore_ascii_case(&target_name)
                        || from.any(|from| from.eq_ignore_ascii_case(&target_name))
                }
            };
            if !convert {
                continue;
            }
            let Some(target) = rules.object(to) else {
                continue;
            };
            let target = DeliveryType {
                category: target.category,
                id: target.id.clone(),
            };
            self.convert_techno_type(id, &target, rules, effects);
            break;
        }
    }

    /// Phobos `FootExt::UpdateTypeData`'s capture-manager arm
    /// (`Ext/Foot/Body.cpp:470-540`): scan the new type's weapon slots for a
    /// `MindControl=` warhead, take the highest `Damage=` as the retained
    /// limit and any `InfiniteMindControl=`, then trim the tail of the node
    /// list, delete the manager when no mind-control weapon remains, or
    /// create one when the type gained such a weapon.
    fn phobos_reset_mind_control_on_convert(
        &mut self,
        id: u64,
        new_type: &crate::rules::object_type::ObjectType,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        let (mut max_capture, mut infinite) = (0, false);
        if let Some(entity) = self.substrate.entities.get(id) {
            for index in 0..crate::rules::object_type::WEAPON_SLOT_COUNT as i32 {
                if let Some(weapon) = crate::sim::combat::combat_weapon::get_weapon(
                    entity,
                    new_type,
                    index,
                    &self.substrate.entities,
                    rules,
                    &self.interner,
                ) && let Some(warhead) = rules.weapon_warhead(weapon)
                    && warhead.mind_control
                {
                    max_capture = max_capture.max(weapon.damage);
                    infinite |= weapon.infinite_mind_control;
                }
            }
        }
        let has_manager = self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.capture_manager.is_some());
        if max_capture > 0 {
            if has_manager {
                let freed = self
                    .substrate
                    .entities
                    .get_mut(id)
                    .and_then(|entity| entity.capture_manager.as_mut())
                    .map(|manager| manager.phobos_reset_for_new_type(max_capture, infinite))
                    .unwrap_or_default();
                for victim in freed {
                    self.free_unit(id, victim, rules, None, effects);
                }
            } else if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.capture_manager = Some(Box::new(
                    crate::sim::capture_manager::CaptureManagerState::constructed(
                        max_capture,
                        infinite,
                    ),
                ));
            }
        } else if has_manager {
            self.free_all_captures(id, rules, None, effects);
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.capture_manager = None;
            }
        }
    }
}

/// `TechnoExt::IsHealthInThreshold` (`Ext/Techno/Body.cpp:752-759`):
/// `(hp > 0 ? hp > min : hp >= min) && hp <= max`, with a zero/zero health
/// always inside. The ratio and its comparisons keep the native masked-x87
/// semantics through [`crate::sim::combat::Health::compare_ratio`].
fn health_in_threshold(
    health: crate::sim::combat::Health,
    strength: i32,
    min: f64,
    max: f64,
) -> bool {
    use crate::util::native_x87::MaskedX87Ordering;
    if health.current() == 0 && strength == 0 {
        return true;
    }
    let above_ok = if health.compare_ratio(strength, 0.0) == MaskedX87Ordering::Greater {
        health.compare_ratio(strength, min) == MaskedX87Ordering::Greater
    } else {
        matches!(
            health.compare_ratio(strength, min),
            MaskedX87Ordering::Greater | MaskedX87Ordering::Equal
        )
    };
    above_ok
        && matches!(
            health.compare_ratio(strength, max),
            MaskedX87Ordering::Less | MaskedX87Ordering::Equal
        )
}

#[cfg(all(test, tests_sim_c_g))]
#[path = "type_conversion_ammo_tests.rs"]
mod ammo_tests;
#[cfg(all(test, tests_sim_c_g))]
#[path = "type_conversion_health_tests.rs"]
mod health_tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "type_conversion_owner_tests.rs"]
mod owner_tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "type_conversion_mind_control_tests.rs"]
mod mind_control_tests;

#[cfg(all(test, tests_sim_c_g))]
#[path = "type_conversion_in_air_tests.rs"]
mod in_air_tests;
