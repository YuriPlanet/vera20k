//! Fly MoveTo4CCC80 and BeginTakeoff4CF950 share the world-owned spatial and
//! sound transaction. Foot destination timing is an optional enclosing caller;
//! locomotor retries must not reset those timers. Stop_Moving4CCFD0 re-targets
//! a moving aircraft through its own class setter.
use crate::sim::world::FrameEffects;
use std::ops::ControlFlow;

use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::{DestinationTiming, air_movement, ground_pose};
use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};

/// The object a destination names; a cell names none.
fn nav_object(target: NavTargetRef) -> Option<u64> {
    match target {
        NavTargetRef::Cell { .. } => None,
        NavTargetRef::Entity { id }
        | NavTargetRef::Building { id }
        | NavTargetRef::Object { id } => Some(id),
    }
}

impl Simulation {
    /// `AircraftClass::Assign_Destination @ 0x0041AA80` (vt+0x480), the
    /// class setter every aircraft destination goes through, ending in the
    /// Foot setter (`0x004D94B0`):
    /// - NULL goes straight to the Foot setter, and a target high in the air
    ///   (vt+0x54) hands it NULL instead (`0x0041AA8B..0x0041AAAE`).
    /// - A Building, while Enter is the aircraft's mission (vt+0x184) or
    ///   queued (`+0xB4`), first runs the dock handshake
    ///   ([`Self::aircraft_enter_destination`]), which can change the
    ///   destination or end the call.
    /// - Then, off a bridge, a `UnitRepair=` or `UnitReload=` Building in the
    ///   aircraft's own cell (`0x0041ACC0..0x0041ADA6`) powers its locomotor
    ///   on (`0x0053A130` is the native false stub) and, when it is contact 0
    ///   and not the destination, gets OVER_OUT (vt+0x274).
    ///
    /// The Foot setter's NULL arm is the shared gate (`0x004D9672`,
    /// [`Simulation::foot_null_destination`]). RESIDUAL: its other work
    /// (linked-lift detach +2AC/+2B0, the retained fire particle +304 and the
    /// Unit-produced +6AC latch) has no aircraft port; none of it applies to
    /// a retail aircraft's destinations.
    pub(crate) fn assign_aircraft_destination(
        &mut self,
        id: u64,
        requested: Option<NavTargetRef>,
        rules: &RuleSet,
        frame_effects: FrameEffects<'_>,
    ) {
        let Some(requested) = requested else {
            self.foot_null_destination(id, Some(rules), None, frame_effects);
            return;
        };
        let high = nav_object(requested).is_some_and(|target| {
            self.substrate.entities.get(target).is_some_and(|e| {
                air_movement::is_high_flying(
                    e,
                    self.resolved_terrain.as_ref(),
                    Some((rules, &self.interner)),
                )
            })
        });
        if high {
            self.foot_null_destination(id, Some(rules), None, frame_effects);
            return;
        }
        let entering = self.substrate.entities.get(id).is_some_and(|entity| {
            let enter = MissionId::from_known(MissionType::Enter);
            entity.mission.effective() == enter || entity.mission.queued() == enter
        });
        let building = nav_object(requested).filter(|&target| {
            self.substrate
                .entities
                .get(target)
                .is_some_and(|e| e.category == EntityCategory::Structure)
        });
        let destination = match building.filter(|_| entering) {
            Some(building) => {
                match self.aircraft_enter_destination(id, building, rules, frame_effects) {
                    ControlFlow::Break(()) => return,
                    ControlFlow::Continue(destination) => destination,
                }
            }
            None => Some(requested),
        };
        self.aircraft_pad_departure(id, destination, rules, frame_effects);
        let Some(destination) = destination else {
            self.foot_null_destination(id, Some(rules), None, frame_effects);
            return;
        };
        if !self.begin_foot_destination(id, true, Some(rules), None, frame_effects) {
            return;
        }
        let entity = self.substrate.entities.get_mut(id).unwrap();
        crate::sim::mission::concrete_effects::represented_assign_destination(
            entity,
            Some(destination),
        );
        let coord = crate::sim::movement::nav_target_coordinate(
            destination,
            Some(id),
            &self.substrate.entities,
            self.resolved_terrain.as_ref(),
            Some((rules, &self.interner)),
        )
        .expect("live aircraft NavCom coordinate");
        self.aircraft_locomotor_move_to(id, coord, rules);
        // Accepted Foot setter resets both timers even when Fly MoveTo refuses
        // (e.g. powered off). Retry count is preserved.
        DestinationTiming::from_rules(self.session.binary_frame, Some(rules))
            .accept(self.substrate.entities.get_mut(id).unwrap());
    }

    /// `0x0041AAE0..0x0041ACBE`: the class setter's dock handshake for
    /// `building` while the aircraft is entering. `Break` ends the setter
    /// before the Foot setter; `Continue` carries the destination it takes.
    /// Archiving is `TechnoClass::Set_ArchiveTarget @ 0x0070C610`.
    ///
    /// - In radio contact (`0x0065AE30`): a building with no slot free or
    ///   its own (`0x0065ADF0`) is archived; one already holding the
    ///   aircraft (`0x0065AD50`) is left alone; otherwise DOCKING to contact
    ///   0 (vt+0x274) answered ROGER ends the call, and any other answer
    ///   sends OVER_OUT to contact 0 and, at a `UnitRepair=`/`UnitReload=`
    ///   building, archives it and drops the destination.
    /// - Out of contact at a building with a slot: DOCKING to it (vt+0x278),
    ///   which HELLOs the aircraft back. Unanswered, OVER_OUT to contact 0
    ///   and, at a repair or reload building, archive it and drop the
    ///   destination. Then a repair or reload building archives the NavCom
    ///   when there is one, any other the building.
    /// - Out of contact at a full building: archive it. A `Helipad=` asks for
    ///   the dock (vt+0x528), drops the destination (vt+0x480 with NULL) and
    ///   queues Enter on that dock after its CAN_LOAD ROGER and a HELLO, or
    ///   Move on the nearest friendly airfield otherwise, commencing when
    ///   ready. A `UnitRepair=` building becomes the pending entry
    ///   (`+0x500`) and the destination drops.
    fn aircraft_enter_destination(
        &mut self,
        id: u64,
        building: u64,
        rules: &RuleSet,
        frame_effects: FrameEffects<'_>,
    ) -> ControlFlow<(), Option<NavTargetRef>> {
        let dock = NavTargetRef::Building { id: building };
        let Some(object) = self
            .substrate
            .entities
            .get(building)
            .and_then(|entity| rules.object(self.interner.resolve(entity.type_ref())))
        else {
            return ControlFlow::Continue(Some(dock));
        };
        let (repair, services, helipad) = (
            object.unit_repair,
            object.unit_repair || object.unit_reload,
            object.helipad,
        );
        let (Some(aircraft), Some(target)) = (
            self.substrate.entities.get(id),
            self.substrate.entities.get(building),
        ) else {
            return ControlFlow::Continue(Some(dock));
        };
        let in_contact = !aircraft.radio_contacts.is_empty();
        let has_slot = target.radio_contacts.has_free_or(id);
        let holds_aircraft = target.radio_contacts.contains(id);
        let archive = |sim: &mut Self, target: Option<NavTargetRef>| {
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                entity.set_archive_target(target.map(TargetKind::from));
            }
        };
        if in_contact {
            if !has_slot {
                archive(self, Some(dock));
                return ControlFlow::Continue(Some(dock));
            }
            if holds_aircraft {
                return ControlFlow::Continue(Some(dock));
            }
            if radio::transmit_to_contact(
                self,
                id,
                RadioMessage::CanDock,
                Some(rules),
                frame_effects,
            ) == RadioResponse::Roger
            {
                return ControlFlow::Break(());
            }
            radio::transmit_to_contact(self, id, RadioMessage::Break, Some(rules), frame_effects);
            if services {
                archive(self, Some(dock));
                return ControlFlow::Continue(None);
            }
            return ControlFlow::Continue(Some(dock));
        }
        if has_slot {
            let mut destination = Some(dock);
            let answer = radio::transmit(
                self,
                id,
                building,
                RadioMessage::CanDock,
                RadioPayload::default(),
                Some(rules),
                frame_effects,
            );
            if answer != RadioResponse::Roger {
                radio::transmit_to_contact(
                    self,
                    id,
                    RadioMessage::Break,
                    Some(rules),
                    frame_effects,
                );
                if services {
                    archive(self, Some(dock));
                    destination = None;
                }
            }
            // `0x0041AC38` reads the NavCom after DOCKING, whose MOVE_HERE
            // may have set it.
            let nav_com = self
                .substrate
                .entities
                .get(id)
                .and_then(|aircraft| aircraft.navigation.nav_com);
            if !services {
                archive(self, Some(dock));
            } else if nav_com.is_some() {
                archive(self, nav_com);
            }
            return ControlFlow::Continue(destination);
        }
        archive(self, Some(dock));
        let mut destination = Some(dock);
        if helipad {
            let pad = self.aircraft_find_docking_bay(id, rules, frame_effects);
            self.assign_aircraft_destination(id, None, rules, frame_effects);
            let accepted = pad.filter(|&pad| {
                radio::transmit(
                    self,
                    id,
                    pad,
                    RadioMessage::CanEnter,
                    RadioPayload::default(),
                    Some(rules),
                    frame_effects,
                ) == RadioResponse::Roger
            });
            let mission = match accepted {
                Some(pad) => {
                    radio::transmit(
                        self,
                        id,
                        pad,
                        RadioMessage::Hello,
                        RadioPayload::default(),
                        Some(rules),
                        frame_effects,
                    );
                    destination = Some(NavTargetRef::Building { id: pad });
                    MissionType::Enter
                }
                None => {
                    destination = Some(self.aircraft_nearest_friendly_airfield_cell(id, rules));
                    MissionType::Move
                }
            };
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                crate::sim::mission::authority::queue_entity_mission_deferred(
                    entity,
                    MissionId::from_known(mission),
                );
            }
            if self.mission_ready_to_commence(id, rules) {
                let now = self.session.binary_frame;
                let _ = self.mission_commence_exact(id, now);
            }
        }
        if repair {
            crate::sim::docking::building_dock::set_pending_entry(self, id, Some(building));
            destination = None;
        }
        ControlFlow::Continue(destination)
    }

    /// `0x0041ACC0..0x0041ADA6`: the class setter's tail for an aircraft on
    /// a `UnitRepair=` or `UnitReload=` Building's cell, off a bridge
    /// (cell flag `0x100`): its locomotor powers on, and the building, when
    /// it is contact 0 and not `destination`, gets OVER_OUT (vt+0x274).
    fn aircraft_pad_departure(
        &mut self,
        id: u64,
        destination: Option<NavTargetRef>,
        rules: &RuleSet,
        frame_effects: FrameEffects<'_>,
    ) {
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
        let Some(pad) = pad else {
            return;
        };
        let entity = self.substrate.entities.get_mut(id).unwrap();
        let locomotor = entity
            .locomotor
            .as_mut()
            .expect("aircraft destination owner has a locomotor");
        if !locomotor.is_powered() {
            locomotor.power_on();
        }
        if entity.radio_contacts.slot(0) == Some(pad)
            && destination.and_then(nav_object) != Some(pad)
        {
            radio::transmit_to_contact(self, id, RadioMessage::Break, Some(rules), frame_effects);
        }
    }

    /// An aircraft's `ILocomotion::Move_To` (`+0x44`), as Foot4D94B0 and
    /// Mission_Move state 1 call it: a spawned missile's Rocket
    /// (`0x006632E0`), every other aircraft's Fly (`0x004CCC80`).
    pub(crate) fn aircraft_locomotor_move_to(
        &mut self,
        id: u64,
        coord: DriveCoord,
        rules: &RuleSet,
    ) {
        let entity = self.substrate.entities.get(id).unwrap();
        if entity
            .locomotor
            .as_ref()
            .is_some_and(|locomotor| locomotor.rocket_runtime().is_some())
        {
            self.rocket_move_to(id, coord, rules);
        } else {
            self.move_air_coordinate(id, coord, None, Some(rules));
        }
    }

    /// `MapClass::PickCellOnEdge @ 0x004AA440` as the aircraft missions call
    /// it, with the empty cell `0x00889E68` as both references and criterion
    /// 4 ([`edge_cell::find_paradrop_edge_cell`](super::edge_cell::find_paradrop_edge_cell)):
    /// Mission_Attack's state 10, Mission_Retreat and the Spy Plane missions.
    /// `None` with no playfield (headless fixtures), where there is no edge
    /// to pick and no draw.
    pub(crate) fn aircraft_edge_cell(
        &mut self,
        edge: super::edge_cell::Edge,
    ) -> Option<(u16, u16)> {
        super::edge_cell::find_paradrop_edge_cell(
            self.playfield_bounds,
            self.resolved_terrain.as_ref(),
            edge,
            &mut self.scenario_rng,
        )
    }

    /// The waypoint edge (`HouseClass+0x577C`) of aircraft `id`'s house.
    pub(crate) fn aircraft_house_waypoint_edge(&self, id: u64) -> u8 {
        self.substrate
            .entities
            .get(id)
            .and_then(|entity| self.houses.get(&entity.owner()))
            .map_or(0, |house| house.waypoint_edge)
    }

    /// Fly `Move_To @ 0x004CCC80` with a non-null coordinate. Answers whether
    /// it took the destination (native MoveTo is void): its refusals are a
    /// landing toward the same cell, the empty coordinate and an unpowered or
    /// warping owner. `timing` is an enclosing Foot setter's tail, run on
    /// acceptance.
    pub(crate) fn move_air_coordinate(
        &mut self,
        id: u64,
        request: DriveCoord,
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
        let flight_level =
            air_movement::type_flight_level(entity, rules.map(|rules| (rules, &self.interner)));
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
        let level =
            air_movement::type_flight_level(entity, rules.map(|rules| (rules, &self.interner)));
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

    /// `FlyLocomotionClass::Begin_Landing @ 0x004CFA70`. Its first gate
    /// (Mission != Enter and `0x006385C0`) only refuses while Techno `+0x514`
    /// holds a planning path; VERA has no planning mode, so it always passes.
    /// A live-type AirportBound Aircraft must be in radio contact with the
    /// building in its current cell (`+0x1BC`); otherwise native calls
    /// Enter_Idle_Mode(0, 1) (vt+0x484, `0x004176F0`) and does not land.
    /// Callers: Horizontal_Step's arrival (`0x004CF520`), Process's landing
    /// trigger (`0x004CE43C`), null MoveTo (`0x004CCDDB`).
    pub(crate) fn begin_fly_landing(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
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
                if let Some(rules) = rules {
                    crate::sim::aircraft::enter_idle_mode_for(
                        self,
                        id,
                        rules,
                        registry,
                        frame_effects,
                    );
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

    /// `FlyLocomotionClass::Stop_Moving @ 0x004CCFD0` (ILocomotion `+0x48`),
    /// [`Self::locomotor_stop_moving`]'s Fly arm: [`Self::fly_stop_order`],
    /// then its effect. A self-destruct is `ReceiveDamage` of the owner's own
    /// Health with `C4Warhead=` ([`Self::receive_own_health_c4`]), after which
    /// the Fly loses its destination (`0x004CD273`); a destination goes
    /// through the owner's class setter (vt+0x480,
    /// [`Self::assign_aircraft_destination`]). Answers false for a moving
    /// Aircraft without rules.
    pub(crate) fn fly_stop_moving(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(rules) = rules else {
            return !self.substrate.entities.get(id).is_some_and(|entity| {
                entity.category == crate::map::entities::EntityCategory::Aircraft
                    && crate::sim::movement::motion_query::is_moving(entity).unwrap_or(false)
            });
        };
        let order = self.fly_stop_order(id, rules);
        #[cfg(test)]
        if let Some(order) = order {
            self.trace_lifecycle_for_test(super::LifecycleTestEvent::FlyStopOrdered { id, order });
        }
        match order {
            None => {}
            Some(FlyStopOrder::SelfDestruct) => {
                self.receive_own_health_c4(id, rules, registry, frame_effects);
                if let Some(state) = self
                    .substrate
                    .entities
                    .get_mut(id)
                    .and_then(|entity| entity.locomotor.as_mut())
                    .and_then(|locomotor| locomotor.fly_runtime_mut())
                {
                    state.clear_destination();
                }
            }
            Some(FlyStopOrder::Destination(target)) => {
                self.assign_aircraft_destination(id, target, rules, frame_effects);
            }
        }
        true
    }

    /// What Fly's `Stop_Moving` (`0x004CCFD0`) does to Aircraft `id`. Nothing
    /// while the Fly is not moving (Is_Moving, `0x004CCA90`). Otherwise it
    /// takes the cell of the owner's Location (over 256 toward zero); an owner
    /// that is no loaner (`+0x3D4`), lies outside the playfield
    /// (`MapClass::IsCellInPlayfield @ 0x00578460`, mode 1) and may not leave
    /// the map (vt+0x4DC, [`Self::aircraft_may_leave_map`]) takes the LocalSize
    /// edge cell instead
    /// ([`playfield_edge_cell`](crate::map::playfield::playfield_edge_cell),
    /// inset 1). Then:
    /// - cell (0, 0) is a self-destruct;
    /// - with Attack its current mission (vt+0x184), the destination is the
    ///   cell [`Self::aircraft_find_nearest_friendly_airfield`] answers;
    /// - otherwise it is the cell Find_Attack_Cell picks from that cell
    ///   ([`Self::aircraft_find_attack_cell`]), which draws on the Scenario
    ///   RNG when the cell will not do.
    ///
    /// Both cells come through `MapClass::GetCellAt`. Native execution:
    /// `tools/spatial_oracle/fly_stop`, replayed in
    /// `fly_process_tests::fly_stop_matches_native_rows`.
    ///
    /// RESIDUAL: a cell beyond the MapClass cell array is the shared dummy
    /// cell, which every later miss restamps. Native's Find_Attack_Cell then
    /// centres its rings on the dummy's latest coordinate and answers the
    /// dummy itself, read again by the setter; VERA holds the cell it asked
    /// for (`aircraft::leave_map`'s RESIDUAL). Trigger: Stop on an aircraft
    /// whose cell, or edge cell, lies beyond the array; only an aircraft the
    /// map edge lets past the Size diamond (vt+0x4DC false, or FlyBy) gets
    /// there. Effect: another destination and Scenario draw count.
    ///
    /// RESIDUAL: the arm for an owner that is not an Aircraft
    /// (`0x004CD132..0x004CD28F`): a 1x1 Track/Fly passable cell, given as its
    /// ground coordinate through the owner's vt+0x480 twice unless the first
    /// makes it land, else the C4 self-destruct of a live owner. No retail
    /// Unit or Infantry type has the Fly locomotor
    /// (`fly_process_tests::retail_fly_owners_are_aircraft`); VERA leaves such
    /// an owner flying as it was.
    pub(crate) fn fly_stop_order(&mut self, id: u64, rules: &RuleSet) -> Option<FlyStopOrder> {
        use crate::sim::mission::{MissionId, MissionType};
        let entity = self.substrate.entities.get(id)?;
        if entity.category != crate::map::entities::EntityCategory::Aircraft
            || !crate::sim::movement::motion_query::is_moving(entity).unwrap_or(false)
        {
            return None;
        }
        let location = ground_pose::position_world_coord(&entity.position);
        let mut cell = ((location.x / 256) as i16, (location.y / 256) as i16);
        if let Some(bounds) = self.playfield_bounds
            && !entity.is_mission_only()
            && !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                (i32::from(cell.0), i32::from(cell.1)),
                Some(bounds),
                self.resolved_terrain.as_ref(),
            )
            && !self.aircraft_may_leave_map(id)
        {
            cell = crate::map::playfield::playfield_edge_cell(bounds, cell, true);
        }
        if cell == (0, 0) {
            return Some(FlyStopOrder::SelfDestruct);
        }
        // Get_Mission (vt+0x184, `0x004CD0DD`).
        let entity = self.substrate.entities.get(id)?;
        let attacking = entity.mission.effective() == MissionId::from_known(MissionType::Attack);
        Some(FlyStopOrder::Destination(if attacking {
            Some(self.aircraft_nearest_friendly_airfield_cell(id, rules))
        } else {
            let (x, y) = self
                .resolved_terrain
                .as_ref()
                .map_or(cell, |t| t.native_cell_coord(t.native_cell_identity(cell)));
            let under = NavTargetRef::cell(x as u16, y as u16);
            self.aircraft_find_attack_cell(id, Some(under), rules)
        }))
    }
}

/// What Fly's `Stop_Moving` does to a moving Aircraft
/// ([`Simulation::fly_stop_order`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlyStopOrder {
    /// `ReceiveDamage` with `C4Warhead=` (`0x004CD0CD`).
    SelfDestruct,
    /// The owner's class setter (vt+0x480) with this target (`0x004CD0F7`,
    /// `0x004CD124`).
    Destination(Option<NavTargetRef>),
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
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/fly_instance_link.json",
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
