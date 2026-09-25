//! Fly MoveTo4CCC80 and BeginTakeoff4CF950 share the world-owned spatial and
//! sound transaction. Foot destination timing is an optional enclosing caller;
//! locomotor retries must not reset those timers.
use super::Simulation;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{DriveCoord, MovementTarget, NavTargetRef};
use crate::sim::movement::{
    DestinationTiming, air_movement, ground_pose, locomotor::MovementLayer,
};
use crate::util::fixed_math::SimFixed;

impl Simulation {
    /// Aircraft41AA80 -> Foot4D94B0, as reached by an Attack with a live Target.
    /// NULL skips Stop in this caller, retaining the Fly request. This does not
    /// replace the general destination setter: queued Enter preprocessing,
    /// linked-lift detach (+2AC/+2B0), retained fire-particle cleanup (+304), and
    /// the Unit-produced +6AC latch still need their native owner migrations.
    pub(crate) fn assign_aircraft_attack_destination(
        &mut self,
        id: u64,
        requested: Option<NavTargetRef>,
        rules: &RuleSet,
    ) {
        let target_id = |target| match target {
            NavTargetRef::Cell { .. } => None,
            NavTargetRef::Entity { id }
            | NavTargetRef::Building { id }
            | NavTargetRef::Object { id } => Some(id),
        };
        //41AA8C: Target+54 is marked && physical GetHeight >= 2*104.
        // Cells implement the false stub. High targets take the NULL Foot
        // entry immediately, before departure power/radio work.
        let high = requested.and_then(target_id).is_some_and(|target| {
            self.substrate.entities.get(target).is_some_and(|e| {
                e.lifecycle.object_alive
                    && air_movement::current_fly_height(e, self.resolved_terrain.as_ref()) >= 208
            })
        });
        let requested = if high { None } else { requested };
        if requested.is_some() {
            let entity = self
                .substrate
                .entities
                .get(id)
                .expect("aircraft destination owner");
            let coord = ground_pose::position_world_coord(&entity.position);
            let bridge = self.resolved_terrain.as_ref().is_some_and(|terrain| {
                let cell =
                    terrain.native_cell_identity(((coord.x / 256) as i16, (coord.y / 256) as i16));
                terrain.native_cell_flags(cell) & 0x100 != 0
            });
            let pad = (!bridge)
                .then(|| self.fly_building_at(coord))
                .flatten()
                .filter(|&building| {
                    self.substrate
                        .entities
                        .get(building)
                        .and_then(|e| rules.object(self.interner.resolve(e.type_ref())))
                        .is_some_and(|o| o.unit_repair || o.unit_reload)
                });
            if let Some(pad) = pad {
                //41AD39..41AD80;53A130 is the native false stub.
                let entity = self.substrate.entities.get_mut(id).unwrap();
                entity
                    .locomotor
                    .as_mut()
                    .expect("Fly destination")
                    .power_on();
                let detach = entity.radio_contacts.slot(0) == Some(pad)
                    && requested.and_then(target_id) != Some(pad);
                if detach {
                    crate::sim::radio::broadcast_break(self, id, Some(rules));
                }
            }
        }
        if !self.begin_foot_destination(id, requested.is_some(), rules) {
            return;
        }
        let entity = self.substrate.entities.get_mut(id).unwrap();
        crate::sim::mission::concrete_effects::represented_assign_destination_mode_one(
            entity, requested,
        );
        if let Some(destination) = requested {
            let coord = crate::sim::movement::nav_target_coordinate(
                destination,
                Some(id),
                &self.substrate.entities,
                self.resolved_terrain.as_ref(),
                Some((rules, &self.interner)),
            )
            .expect("live aircraft NavCom coordinate");
            let entity = self.substrate.entities.get(id).unwrap();
            let speed = rules
                .object(self.interner.resolve(entity.type_ref()))
                .map_or(SimFixed::from_num(8), |o| {
                    crate::util::fixed_math::ra2_speed_to_leptons_per_second(o.speed.max(1))
                });
            self.move_air_coordinate(id, coord, speed, None, Some(rules));
        }
        // Accepted Foot setter resets both timers even when Fly MoveTo refuses
        // (e.g. powered off). Retry count is preserved.
        DestinationTiming::from_rules(self.session.binary_frame, Some(rules))
            .accept(self.substrate.entities.get_mut(id).unwrap());
    }

    /// Non-null coordinate entry. The bool describes the remaining movement
    /// adapter, not the native void MoveTo or Foot AssignDestination result.
    pub(crate) fn move_air_coordinate(
        &mut self,
        id: u64,
        request: DriveCoord,
        speed: SimFixed,
        timing: Option<DestinationTiming>,
        rules: Option<&RuleSet>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity
            .locomotor
            .as_ref()
            .and_then(|l| l.fly_runtime())
            .is_some_and(|state| state.ignores_destination(request))
            || request == (DriveCoord { x: 0, y: 0, z: 0 })
            || !air_movement::fly_coordinate_admitted(entity)
        {
            return false;
        }
        let flight_level = rules.map_or(500, |rules| {
            rules
                .object(self.interner.resolve(entity.type_ref()))
                .map_or(rules.general.flight_level, |o| {
                    o.flight_level(rules.general.flight_level)
                })
        });
        let armed_flight_level = (entity.attack_target.is_some()
            && entity
                .aircraft_ammo
                .as_ref()
                .is_some_and(|a| a.current != 0))
        .then_some(flight_level);
        //4CCE1C..4CCE6E stores destination before landing-base/height reads.
        if let Some(state) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|e| e.locomotor.as_mut())
            .and_then(|l| l.fly_runtime_mut())
        {
            state.retain_destination(request, armed_flight_level, || {
                ground_pose::ground_surface_z_at(
                    [request.x, request.y],
                    false,
                    self.resolved_terrain.as_ref(),
                    None,
                )
                .unwrap_or(0)
            });
        }
        let entity = self.substrate.entities.get(id).unwrap();
        let begin_takeoff = entity
            .locomotor
            .as_ref()
            .and_then(|l| l.fly_runtime())
            .is_some_and(|state| {
                let base = crate::sim::aircraft::landing_base::landing_base(
                    entity,
                    &self.substrate.entities,
                    rules.map(|r| (r, &self.interner)),
                );
                state.should_begin_takeoff(
                    entity.health.current,
                    || air_movement::current_fly_height(entity, self.resolved_terrain.as_ref()),
                    base,
                )
            });
        if begin_takeoff {
            self.begin_fly_takeoff(id, rules);
        }
        //4CCED9 follows ALL BeginTakeoff effects. Its stored-destination ground
        // query must be last, including when both lookups stamp the Cell Dummy.
        let entity = self.substrate.entities.get_mut(id).unwrap();
        let aircraft = entity.category == crate::map::entities::EntityCategory::Aircraft;
        let ready = aircraft
            && entity
                .mission_leaf
                .as_aircraft()
                .is_some_and(|l| l.action_latch() != 0);
        let non_landable = aircraft
            && rules
                .and_then(|r| r.object(self.interner.resolve(entity.type_ref())))
                .is_some_and(|o| !o.landable);
        if let Some(state) = entity.locomotor.as_mut().and_then(|l| l.fly_runtime_mut()) {
            let destination = state.destination();
            let ground = ground_pose::ground_surface_z_at(
                [destination.x, destination.y],
                false,
                self.resolved_terrain.as_ref(),
                None,
            )
            .unwrap_or(0);
            state.select_destination_mode(
                ground,
                armed_flight_level.is_some(),
                ready,
                non_landable,
            );
        }
        // Derived path projection retained for the pending full Process/Stop
        // migration. Fly XYZ and moving+34 remain the locomotor authority.
        let target = (
            (request.x / 256) as i16 as u16,
            (request.y / 256) as i16 as u16,
        );
        entity.movement_target = Some(MovementTarget {
            path: vec![target],
            path_layers: vec![MovementLayer::Air],
            next_index: 0,
            speed,
            final_goal: Some(target),
            ..Default::default()
        });
        if let Some(timing) = timing {
            timing.accept(entity);
        }
        true
    }

    ///4CF950: owner refusal gates precede flags, AirTracker admission, live type
    /// FlightLevel, ground-facing snap and AuxSound1. Power is a MoveTo gate,
    /// not a second BeginTakeoff gate.
    pub(crate) fn begin_fly_takeoff(&mut self, id: u64, rules: Option<&RuleSet>) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if crate::sim::movement::locomotor_owner::owner_is_warping(entity)
            || entity
                .locomotor
                .as_ref()
                .and_then(|l| l.fly_runtime())
                .is_none()
        {
            return false;
        }
        let level = rules.map_or(500, |rules| {
            rules
                .object(self.interner.resolve(entity.type_ref()))
                .map_or(rules.general.flight_level, |o| {
                    o.flight_level(rules.general.flight_level)
                })
        });
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .begin_fly_takeoff(level);
        self.finish_fly_takeoff_entry(id, rules);
        true
    }

    ///4CFA70. Its first gate (Mission!=Enter and6385C0) only refuses while
    /// Techno+514 holds a planning path; VERA has no planning mode, so it
    /// always passes. A live-type AirportBound Aircraft must be in radio
    /// contact with the building in its current cell (+1BC); otherwise native
    /// calls Enter_Idle_Mode(0,1) (vtable+484 ->4176F0) and does not land.
    /// Callers: Horizontal_Step4CF520 arrival, Process4CE43C, null MoveTo4CCDDB.
    pub(crate) fn begin_fly_landing(&mut self, id: u64, rules: Option<&RuleSet>) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity
            .locomotor
            .as_ref()
            .and_then(|l| l.fly_runtime())
            .is_none()
        {
            return false;
        }
        let airport_bound = entity.category == crate::map::entities::EntityCategory::Aircraft
            && rules
                .and_then(|r| r.object(self.interner.resolve(entity.type_ref())))
                .is_some_and(|o| o.airport_bound);
        if airport_bound {
            let cell = ground_pose::position_world_coord(&entity.position);
            let contact = self
                .fly_building_at(cell)
                .is_some_and(|building| entity.radio_contacts.contains(building));
            if !contact {
                let entity = self.substrate.entities.get_mut(id).unwrap();
                if entity.aircraft_mission.is_some() {
                    entity.aircraft_mission = Some(crate::sim::aircraft::AircraftMission::Idle);
                }
                return false;
            }
        }
        self.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .begin_fly_landing();
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::rules::{
        ini_parser::IniFile,
        object_type::{ObjectCategory, ObjectType},
    };
    use crate::sim::movement::locomotor::LocomotorState;

    #[test]
    fn fly_link_retains_native_aircraft_only_airport_binding() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/fly_instance_link.json"
        ))
        .unwrap();
        assert_eq!(rows.len(), 4);
        for row in rows {
            let ini = IniFile::from_str(&format!(
                "[TEST]\nAirportBound={}\nLocomotor={{4A582746-9839-11D1-B709-00A024DDAFD1}}\n",
                row["type_airport_bound"].as_bool().unwrap()
            ));
            let category = if row["aircraft"].as_bool().unwrap() {
                ObjectCategory::Aircraft
            } else {
                ObjectCategory::Vehicle
            };
            let mut object =
                ObjectType::from_ini_section("TEST", ini.section("TEST").unwrap(), category);
            let loco = LocomotorState::from_object_type(&object, 0);
            assert_eq!(
                loco.fly_runtime().unwrap().airport_bound(),
                row["linked"].as_bool().unwrap(),
                "{row}"
            );
            object.airport_bound = !object.airport_bound;
            assert_eq!(
                loco.fly_runtime().unwrap().airport_bound(),
                row["after_type_change"].as_bool().unwrap(),
                "{row}"
            );
        }
    }
}
