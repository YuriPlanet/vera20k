//! Optional Ares vehicle capture. Cargo, mind control, ownership and deferred
//! destruction keep their existing state owners.

use crate::map::entities::EntityCategory;
use crate::rules::{ares_driver::OperatorRequirement, ruleset::RuleSet};
use crate::sim::{mission::MissionType, world::Simulation};

mod driver_kill;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VehicleCaptureMode {
    Hijack,
    Drive,
}

/// Installed Ares TechnoExt+68/+6C/+70; +338 remains the existing captor
/// type owner. Copy only the accumulator, never a donor's rank cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct CapturedDriver {
    owner: Option<crate::sim::intern::InternedId>,
    health: i32,
    experience: crate::util::native_x87::NativeF32Bits,
}

impl Default for CapturedDriver {
    fn default() -> Self {
        // Techno ctor hook6F3260 ->10046F70 ->10042CA0; stores
        // 10042DB5/10042DC8/10042DCF initialize experience0, health-1, NULL.
        Self {
            owner: None,
            health: -1,
            experience: crate::util::native_x87::NativeF32Bits::from_bits(0),
        }
    }
}

/// TechnoExt constructor10042CA0 and capture10045D21..10045D55. The fatal
/// receiver1004650F marks the survivor pass once before its refusal gates.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct VehicleDriverState {
    captured: CapturedDriver,
    killed: bool,
    survivors_handled: bool,
    /// TechnoExt+B0 selects the Script67 variant of Foot's vt+34C seek.
    #[serde(default)]
    vehicle_seek: bool,
}

impl VehicleDriverState {
    pub(crate) fn killed(&self) -> bool {
        self.killed
    }

    /// Ares100456E8..100456F5 copies the new HouseType's MultiplayPassive,
    /// before even testing whether the owner itself changes.
    pub(crate) fn set_killed_from_owner(&mut self, passive: bool) {
        self.killed = passive;
    }

    pub(crate) fn set_vehicle_seek(&mut self, value: bool) {
        self.vehicle_seek = value;
    }

    pub(crate) fn vehicle_seek(&self) -> bool {
        self.vehicle_seek
    }
    pub(crate) fn record_captor(
        &mut self,
        owner: crate::sim::intern::InternedId,
        health: i32,
        experience: crate::util::native_x87::NativeF32Bits,
    ) {
        self.captured = CapturedDriver {
            owner: Some(owner),
            health,
            experience,
        };
    }

    pub(crate) fn restore_driver(&mut self) {
        self.killed = false;
    }

    pub(crate) fn begin_survivor_release(&mut self, ignore_defenses: bool) -> bool {
        let handled = self.survivors_handled;
        self.survivors_handled = true;
        !handled && !self.killed && !ignore_defenses
    }

    pub(crate) fn captured(&self) -> CapturedDriver {
        self.captured
    }
}

impl CapturedDriver {
    pub(crate) fn owner(self) -> Option<crate::sim::intern::InternedId> {
        self.owner
    }
    pub(crate) fn health(self) -> i32 {
        self.health
    }
    pub(crate) fn experience(self) -> crate::util::native_x87::NativeF32Bits {
        self.experience
    }
}

impl Simulation {
    /// Infantry WhatAction51E7A6's human-house prefix, then the installed
    /// Ares hook10049690: successful admission returns Capture9 at51E84B.
    /// Command producers and cursor feedback share this read-only action.
    pub fn ares_vehicle_capture_action(
        &self,
        actor_id: u64,
        target_id: u64,
        rules: &RuleSet,
    ) -> bool {
        self.substrate.entities.get(actor_id).is_some_and(|actor| {
            self.house_is_human_player(actor.owner())
                && self
                    .ares_vehicle_capture_mode(actor_id, target_id, rules)
                    .is_some()
        })
    }

    /// Installed Ares TechnoExt::FindHijackTarget10046950..10046AE0,
    /// reached by Foot4DFE00's hook10049130 and Script67's dispatcher.
    pub(crate) fn ares_enter_nearest_vehicle(
        &mut self,
        actor_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: crate::sim::world::FrameEffects<'_>,
    ) -> bool {
        use crate::sim::components::NavTargetRef;
        use crate::sim::movement::ground_pose::position_world_coord;
        let Some(actor) = self.substrate.entities.get(actor_id) else {
            return false;
        };
        if actor.category() != EntityCategory::Infantry
            || !self
                .entity_object_type(actor, rules)
                .is_some_and(|ty| ty.vehicle_thief || ty.driver_rules().can_drive())
        {
            return false;
        }
        //100469DC..10046A1E reads Object+9C/A0/A4, not GetCoords.
        // Map-sized integer squares are exact in the native double domain;
        // keep the comparison integral and deterministic across platforms.
        let from = position_world_coord(&actor.position);
        let mut best: Option<(u128, i32, u64)> = None;
        for index in 0..self.substrate.entities.foot_registry_len() {
            let Some(id) = self.substrate.entities.foot_registry_at(index) else {
                continue;
            };
            let Some(candidate) = self.substrate.entities.get(id) else {
                continue;
            };
            let to = position_world_coord(&candidate.position);
            let distance = [(from.x, to.x), (from.y, to.y), (from.z, to.z)]
                .into_iter()
                .map(|(from, to)| {
                    let delta = i128::from(from.wrapping_sub(to));
                    (delta * delta) as u128
                })
                .sum::<u128>();
            let identity = candidate.native_unique_id();
            if best.is_some_and(|(old_distance, old_identity, _)| {
                distance > old_distance || distance == old_distance && identity >= old_identity
            }) || self
                .ares_vehicle_capture_mode(actor_id, id, rules)
                .is_none()
            {
                continue;
            }
            best = Some((distance, identity, id));
        }
        let Some(actor) = self.substrate.entities.get_mut(actor_id) else {
            return false;
        };
        actor
            .building_entry
            .set_nearest_occupiable_seek(best.is_some());
        actor.vehicle_driver.set_vehicle_seek(best.is_some());
        let Some((_, _, target)) = best else {
            return false;
        };
        if actor.navigation.nav_com() == Some(NavTargetRef::object(target))
            && actor.mission.current().known() == Some(MissionType::Capture)
        {
            return true;
        }
        let _ = self.assign_destination_represented(
            actor_id,
            Some(NavTargetRef::object(target)),
            rules,
            registry,
            effects,
        );
        let _ = self.mission_queue_exact(
            actor_id,
            crate::sim::mission::MissionId::from_known(MissionType::Capture),
            1,
            self.session.binary_frame(),
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        );
        true
    }

    /// Installed Ares TechnoExt capture10045B60..10045E2B. AI hook
    /// 5203F7->100499A0 and PerCell hook519675->10049820 share this owner;
    /// their callers perform UnInit only when the returned flag says so.
    pub(crate) fn ares_capture_vehicle(
        &mut self,
        actor_id: u64,
        target_id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: crate::sim::world::FrameEffects<'_>,
    ) -> Result<crate::sim::world::VehicleCaptureOutcome, String> {
        use crate::sim::world::{UninitContext, VehicleCaptureOutcome};
        let Some(actor) = self
            .substrate
            .entities
            .get(actor_id)
            .filter(|actor| actor.category() == EntityCategory::Infantry)
        else {
            return Ok(VehicleCaptureOutcome::default());
        };
        //10045B8A retains the entry type through every later callback.
        let actor_type = self
            .entity_object_type(actor, rules)
            .ok_or("capture actor has no InfantryType")?;
        let captor_index = rules
            .type_array_index(
                crate::rules::object_type::ObjectCategory::Infantry,
                &actor_type.id,
            )
            .ok_or("capture actor has no InfantryType array index")?;
        let enter_sound = actor_type.vehicle_thief_sound(false).map(str::to_owned);
        let context = UninitContext::new(rules, registry).with_effects(effects);
        let Some(mode) = self.ares_vehicle_capture_mode(actor_id, target_id, rules) else {
            self.assign_destination_represented(actor_id, None, rules, registry, effects)
                .map_err(|e| format!("refused vehicle capture destination: {e}"))?;
            let from = self.substrate.entities.get(target_id).map(|target| {
                crate::sim::movement::ground_pose::object_get_coords(
                    target,
                    self.resolved_terrain.as_ref(),
                )
            });
            let changed = self.scatter(
                actor_id,
                from,
                crate::sim::movement::ScatterFlags::new(true, false),
                rules,
                registry,
                effects,
            )?;
            return Ok(VehicleCaptureOutcome {
                bridge_state_changed: changed,
                ..Default::default()
            });
        };
        if let Some(controller) = self
            .substrate
            .entities
            .get(target_id)
            .and_then(|target| target.mind_control.controller())
        {
            self.free_unit(controller, target_id, rules, registry, effects);
        }
        self.clear_permanent_control_and_ring(target_id, rules);
        let board_driver = mode == VehicleCaptureMode::Drive
            && self
                .substrate
                .entities
                .get(target_id)
                .and_then(|target| self.entity_object_type(target, rules))
                .is_some_and(|ty| {
                    !matches!(ty.driver_rules().operator(), OperatorRequirement::None)
                });
        self.dispatch_object_tag_event(
            target_id,
            if board_driver { 1 } else { 48 },
            Some(actor_id),
            None,
            context,
        );
        if !board_driver {
            if let Some(owner) = self
                .substrate
                .entities
                .get(target_id)
                .map(|target| target.owner())
                && let Some(house) = self.houses.get_mut(&owner)
            {
                house.notify_building_capture();
            }
            self.trigger_runtime
                .transfer_object_tag(actor_id, target_id);
        }
        let controller = self
            .substrate
            .entities
            .get(actor_id)
            .and_then(|actor| actor.mind_control.controller());
        if let Some(controller) = controller {
            self.with_object_placement_scope(|sim| {
                sim.free_unit(controller, actor_id, rules, registry, effects);
            });
        }
        let owner = self
            .substrate
            .entities
            .get(actor_id)
            .map(|actor| actor.owner())
            .ok_or("vehicle captor retired during capture callbacks")?;
        self.change_owner_with_rules(target_id, owner, rules, registry, effects);
        // Unit/Aircraft+2E0=70D980 is a no-op; Building+2E0=4576F0
        // scatters infantry synchronously through its existing owner.
        let changed = if self
            .substrate
            .entities
            .get(target_id)
            .is_some_and(|target| target.category() == EntityCategory::Structure)
        {
            self.scatter_building_infantry(target_id, rules, registry, effects)
                .map_err(|e| format!("captured building evacuation: {e}"))?
        } else {
            false
        };
        if let Some(sound_id) = enter_sound
            && let Some(target) = self.substrate.entities.get(target_id)
        {
            let coordinate =
                crate::sim::movement::ground_pose::position_world_coord(&target.position);
            self.sound_events
                .emit(crate::sim::world::SimSoundEvent::VocAtCoord {
                    sound_id,
                    coordinate,
                    flags: 0,
                });
            effects.flush_sound(self, rules);
        }
        let captor = self
            .substrate
            .entities
            .get(actor_id)
            .map(|actor| (actor.owner(), actor.health.current(), actor.veterancy.raw()))
            .ok_or("vehicle captor retired before metadata commit")?;
        if let Some(target) = self.substrate.entities.get_mut(target_id) {
            target.vehicle_driver.restore_driver();
            if mode == VehicleCaptureMode::Hijack {
                target.capture_infantry_type_index.record(captor_index);
                target
                    .vehicle_driver
                    .record_captor(captor.0, captor.1, captor.2);
            }
        }
        if let Some(controller) = controller {
            self.with_object_placement_scope(|sim| {
                sim.capture_unit(controller, target_id, rules, registry, effects);
            });
        }
        let frame = self.session.binary_frame();
        if let Some(manager) = self
            .substrate
            .entities
            .get_mut(target_id)
            .and_then(|target| target.spawn_manager.as_mut())
        {
            manager.resume_update_timer(frame as i32);
        }
        if board_driver {
            let size = self
                .substrate
                .entities
                .get(actor_id)
                .and_then(|actor| self.entity_object_type(actor, rules))
                .map_or(1.0, |ty| ty.size);
            if !self.add_passenger_with_context(target_id, &[(actor_id, size)], context) {
                return Err("captured vehicle retired during driver boarding".into());
            }
            if let Some(actor) = self.substrate.entities.get_mut(actor_id) {
                actor.navigation.stop_moving();
            }
        }
        let owner = self
            .substrate
            .entities
            .get(actor_id)
            .map(|actor| actor.owner())
            .ok_or("vehicle captor retired before mission publication")?;
        let human = self
            .houses
            .get(&owner)
            .is_some_and(|house| house.is_controlled_by_human(self.session.game_mode_nonzero()));
        self.mission_queue_exact(
            target_id,
            crate::sim::mission::MissionId::from_known(if human {
                MissionType::Guard
            } else {
                MissionType::Hunt
            }),
            1,
            frame,
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        )
        .map_err(|e| format!("captured vehicle mission: {e}"))?;
        // Tag6E53A0 arguments: affected vehicle, then distinct live source.
        self.dispatch_object_tag_event(target_id, 70, Some(target_id), Some(actor_id), context);
        if self
            .substrate
            .entities
            .get(target_id)
            .is_some_and(|target| target.lifecycle.object_alive())
        {
            self.dispatch_object_tag_event(target_id, 69, Some(target_id), None, context);
        }
        Ok(VehicleCaptureOutcome {
            consumed: !board_driver,
            bridge_state_changed: changed,
        })
    }

    /// IsOperated10045440, inlined in admission10045097 (installed Ares).
    /// Ext+9D caches only the unrestricted case. Recompute this derived query
    /// from the authoritative type and Cargo instead of adding another latch.
    pub(crate) fn ares_is_operated(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(ty) = self.entity_object_type(actor, rules) else {
            return false;
        };
        let cargo = actor.passenger_role.cargo();
        match ty.driver_rules().operator() {
            OperatorRequirement::None => true,
            OperatorRequirement::Any => cargo.is_some_and(|cargo| !cargo.passengers().is_empty()),
            OperatorRequirement::Types(types) => cargo.is_some_and(|cargo| {
                cargo.passengers().iter().any(|&id| {
                    self.substrate
                        .entities
                        .get(id)
                        .and_then(|passenger| self.entity_object_type(passenger, rules))
                        .is_some_and(|passenger| {
                            types.iter().any(|required| {
                                required.category == passenger.category
                                    && required.id.eq_ignore_ascii_case(&passenger.id)
                            })
                        })
                })
            }),
        }
    }

    /// Admission10044FF0..10045311 (installed Ares), shared by the cursor,
    /// per-cell capture and Script67 search. ObjectAlive is not Health;
    /// the captor's directional alliance relation decides hostility.
    pub(crate) fn ares_vehicle_capture_mode(
        &self,
        actor_id: u64,
        target_id: u64,
        rules: &RuleSet,
    ) -> Option<VehicleCaptureMode> {
        let actor = self.substrate.entities.get(actor_id)?;
        let target = self.substrate.entities.get(target_id)?;
        if actor.category() != EntityCategory::Infantry
            || !actor.lifecycle.object_alive()
            || !target.lifecycle.object_alive()
        {
            return None;
        }
        let actor_type = self.entity_object_type(actor, rules)?;
        let target_type = self.entity_object_type(target, rules)?;
        let driver = actor_type.driver_rules();
        if (!actor_type.vehicle_thief && !driver.can_drive())
            // Infantry5228D0 also requires the type's Deployer byte.
            || actor_type.deployer && actor.infantry_deploy_doing()
            || !self.ares_is_operated(actor_id, rules)
        {
            return None;
        }
        match target.category() {
            EntityCategory::Unit | EntityCategory::Aircraft => {}
            EntityCategory::Structure if actor_type.vehicle_thief => {}
            EntityCategory::Infantry | EntityCategory::Structure => return None,
        }
        if target.mission.current().known() == Some(MissionType::Selling)
            || target.parasite.eater().is_some()
            || target.is_warped_out()
            || target_type.is_train
            || target_type.balloon_hover
        {
            return None;
        }
        // Isolated lookups keep cursor queries from changing the simulation's
        // shared Dummy cell. This admission consumes no map pointer identity.
        let cells = self
            .resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
        let low_flying = || match cells.as_ref() {
            Some(cells) => crate::sim::movement::air_movement::is_low_flying_in_query(
                target,
                cells,
                Some((rules, &self.interner)),
            ),
            None => crate::sim::movement::air_movement::is_low_flying(
                target,
                None,
                Some((rules, &self.interner)),
            ),
        };
        //100451A8 skips vt+80 for a Unit, including a NonVehicle Unit.
        if target.category() != EntityCategory::Unit
            && !crate::sim::combat::vehicle_like(
                target.category(),
                target_type.non_vehicle,
                target_type.is_1x1_with_undeploy(),
                low_flying,
            )
            || !low_flying()
            || crate::sim::superweapon::invulnerability::is_invulnerable(
                target.invulnerability.as_ref(),
                self.session.binary_frame(),
            )
            || actor_type.vehicle_thief
                && target.mind_control.is_mind_controlled()
                && !driver.break_mind_control()
        {
            return None;
        }
        if target.category() == EntityCategory::Unit
            && self.session.harvester_immune()
            && rules
                .harvester_unit_types
                .iter()
                .any(|id| id.eq_ignore_ascii_case(&target_type.id))
        {
            return None;
        }
        let country = self.houses.get(&target.owner())?.house_type_id();
        let can_drive_country = rules.country_can_be_driven(self.interner.resolve(country));
        if can_drive_country && driver.can_drive() && target_type.driver_rules().can_be_driven() {
            return Some(VehicleCaptureMode::Drive);
        }
        if !actor_type.vehicle_thief
            || can_drive_country
            || crate::sim::combat::combat_weapon::is_ally_by_object(
                Some(self.house_alliances.as_map()),
                &self.interner,
                actor.owner(),
                target.owner(),
            )
            || !target_type.driver_rules().theft_allowed()
        {
            return None;
        }
        Some(VehicleCaptureMode::Hijack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{house_state::HouseState, world::FrameEffects};

    #[test]
    fn ares_vehicle_capture_admission_distinguishes_theft_and_civilian_driving() {
        let (mut sim, rules, _) = crate::sim::world::entry_test_fixture::fixture_with_rules(
            "[Countries]\n0=Americans\n1=Russians\n2=Neutral\n3=NoDrive\n\
             [Neutral]\nMultiplayPassive=yes\n[NoDrive]\nMultiplayPassive=yes\nCanBeDriven=no\n\
             [InfantryTypes]\n2=HIJACK\n3=DRIVER\n\
             [VehicleTypes]\n0=HOST\n1=LOCKED\n\
             [HIJACK]\nVehicleThief=yes\nCanDrive=yes\nStrength=75\nSpeed=4\nSpeedType=Foot\n\
             Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             [DRIVER]\nCanDrive=yes\nStrength=75\nSpeed=4\nSpeedType=Foot\n\
             Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             [HOST]\nStrength=300\nSpeed=6\nNonVehicle=yes\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             [LOCKED]\nStrength=300\nSpeed=6\nCanBeDriven=no\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
        );
        let owner = sim.interner.intern("Americans");
        let other = sim.interner.intern("Russians");
        let neutral = sim.interner.intern("Neutral");
        let no_drive = sim.interner.intern("NoDrive");
        sim.houses
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        sim.houses
            .register(HouseState::new(other, 1, None, false, 0, 10));
        sim.houses
            .register(HouseState::new(neutral, 0, None, false, 0, 10));
        sim.houses
            .register(HouseState::new(no_drive, 0, None, false, 0, 10));
        let hijacker = sim
            .spawn_object_at_height("HIJACK", "Americans", 15, 15, 0, 0, &rules)
            .unwrap();
        let driver = sim
            .spawn_object_at_height("DRIVER", "Americans", 17, 15, 0, 0, &rules)
            .unwrap();
        let host = sim
            .spawn_object_at_height("HOST", "Russians", 16, 15, 0, 0, &rules)
            .unwrap();
        let locked = sim
            .spawn_object_at_height("LOCKED", "Russians", 17, 16, 0, 0, &rules)
            .unwrap();
        let rng = sim.scenario_rng.state();
        assert_eq!(
            sim.ares_vehicle_capture_mode(hijacker, host, &rules),
            Some(VehicleCaptureMode::Hijack)
        );
        assert_eq!(sim.ares_vehicle_capture_mode(driver, host, &rules), None);
        assert_eq!(sim.scenario_rng.state(), rng);
        sim.change_owner_with_rules(host, owner, &rules, None, FrameEffects::default());
        assert_eq!(sim.ares_vehicle_capture_mode(hijacker, host, &rules), None);
        sim.change_owner_with_rules(host, neutral, &rules, None, FrameEffects::default());
        sim.change_owner_with_rules(locked, neutral, &rules, None, FrameEffects::default());
        assert_eq!(
            sim.ares_vehicle_capture_mode(hijacker, host, &rules),
            Some(VehicleCaptureMode::Drive)
        );
        assert_eq!(
            sim.ares_vehicle_capture_mode(driver, host, &rules),
            Some(VehicleCaptureMode::Drive)
        );
        assert_eq!(sim.ares_vehicle_capture_mode(driver, locked, &rules), None);
        assert!(sim.ares_enter_nearest_vehicle(driver, &rules, None, FrameEffects::default()));
        let seeking = sim.substrate.entities.get(driver).unwrap();
        assert_eq!(
            seeking.navigation.nav_com(),
            Some(crate::sim::components::NavTargetRef::object(host))
        );
        assert!(seeking.building_entry.should_enter_nearest_occupiable());
        assert!(seeking.vehicle_driver.vehicle_seek());
        sim.change_owner_with_rules(host, no_drive, &rules, None, FrameEffects::default());
        assert_eq!(sim.ares_vehicle_capture_mode(driver, host, &rules), None);
        assert_eq!(
            sim.ares_vehicle_capture_mode(hijacker, host, &rules),
            Some(VehicleCaptureMode::Hijack)
        );
        sim.substrate
            .entities
            .get_mut(host)
            .unwrap()
            .lifecycle
            .set_object_alive_for_test(false);
        assert_eq!(sim.ares_vehicle_capture_mode(driver, host, &rules), None);
        assert!(!sim.ares_enter_nearest_vehicle(driver, &rules, None, FrameEffects::default()));
        let stopped = sim.substrate.entities.get(driver).unwrap();
        assert!(!stopped.building_entry.should_enter_nearest_occupiable());
        assert!(!stopped.vehicle_driver.vehicle_seek());
    }
}
