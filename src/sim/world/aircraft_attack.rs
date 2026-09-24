//! World effects of Aircraft Mission_Attack417FE0 that run in the aircraft's
//! dispatch, in live actor order: states 0/1/3's fire-location search,
//! approach steering and destination transactions, and state 10's exit
//! (`aircraft::attack_mission::exit_visit`). State 1 consumes Scenario RNG;
//! state 3 returns a one-tick delay. States 4..9 run in the combat phase
//! (`combat::aircraft_release`).
use super::Simulation;
use crate::rules::ruleset::RuleSet;
use crate::sim::aircraft::{AircraftMission, attack_mission};
use crate::sim::combat::TargetKind;
use crate::sim::combat::{combat_weapon, fire_coord};
use crate::sim::components::NavTargetRef;
use crate::sim::mission::MissionType;
use crate::sim::movement::air_movement;
use crate::util::fixed_math::SIM_ZERO;

fn navigation_target(target: TargetKind) -> NavTargetRef {
    match target {
        TargetKind::Entity(id) => NavTargetRef::Entity { id },
        TargetKind::Cell(rx, ry) => NavTargetRef::cell(rx, ry),
    }
}

fn attack_target(target: NavTargetRef) -> TargetKind {
    match target {
        NavTargetRef::Cell { rx, ry } => TargetKind::Cell(rx, ry),
        NavTargetRef::Entity { id }
        | NavTargetRef::Object { id }
        | NavTargetRef::Building { id } => TargetKind::Entity(id),
    }
}

impl Simulation {
    ///418006..418030: raw Target presence chooses1/10, preserving pending ammo.
    /// The caller already cleared readiness. Search belongs to the next visit.
    pub(crate) fn aircraft_begin_attack(&mut self, id: u64) -> AircraftMission {
        let present = attack_mission::aircraft_target_present(
            self.substrate
                .entities
                .get(id)
                .expect("aircraft dispatch")
                .attack_target
                .as_ref(),
            &self.substrate.entities,
        );
        self.aircraft_attack_visit(id, if present { 1 } else { 10 }, 1)
    }

    ///418031..41809C, after enter_attack_state consumes pending ammo/clears
    /// readiness. A refused void destination setter can retain the OLD NavCom;
    /// neither FindFireLocation's result nor Fly MoveTo's adapter bool decides
    /// whether state3 is entered. Executable corpus: aircraft_reengagement.*.
    pub(crate) fn aircraft_reengage(&mut self, id: u64, rules: &RuleSet) -> AircraftMission {
        let entity = self.substrate.entities.get(id).expect("aircraft dispatch");
        let target = entity
            .attack_target
            .as_ref()
            .filter(|attack| {
                attack_mission::aircraft_target_present(Some(attack), &self.substrate.entities)
            })
            .map(|a| navigation_target(a.target));
        let ammo = entity.aircraft_ammo.as_ref().map_or(-1, |a| a.current);
        let state = if target.is_some() && ammo != 0 {
            let destination = self.aircraft_find_fire_location(id, target, rules);
            self.assign_aircraft_attack_destination(id, destination, rules);
            if self
                .substrate
                .entities
                .get(id)
                .unwrap()
                .navigation
                .nav_com
                .is_some()
            {
                3
            } else {
                10
            }
        } else {
            10
        };
        //418D1D: the Attack dispatch reads MissionControl Rate, then draws even
        // when there was no target/ammo or when the destination was refused.
        let delay = crate::sim::aircraft::attack_mission::mission_epilogue(
            rules,
            MissionType::Attack,
            &mut self.scenario_rng,
        );
        self.aircraft_attack_visit(id, state, delay)
    }

    ///4180A1..4182A2 after the shared entry prefix. Strafe classification wins
    /// over Fighter. Other aircraft approach their retained firing position,
    /// not a freshly substituted target cell. Corpus: aircraft_approach.*.
    pub(crate) fn aircraft_approach(&mut self, id: u64, rules: &RuleSet) -> AircraftMission {
        let state = self.advance_aircraft_approach(id, rules);
        self.aircraft_attack_visit(id, state, 1)
    }

    fn advance_aircraft_approach(&mut self, id: u64, rules: &RuleSet) -> u8 {
        let entity = self
            .substrate
            .entities
            .get(id)
            .expect("aircraft approach owner");
        let Some(target) = entity
            .attack_target
            .as_ref()
            .filter(|attack| {
                attack_mission::aircraft_target_present(Some(attack), &self.substrate.entities)
            })
            .map(|a| a.target)
        else {
            return 10;
        };
        if entity
            .aircraft_ammo
            .as_ref()
            .is_some_and(|a| a.current == 0)
        {
            return 10;
        }
        let object = rules
            .object(self.interner.resolve(entity.type_ref()))
            .expect("aircraft type");
        if combat_weapon::aircraft_strafes(rules, object, entity.veterancy) {
            let weapon = combat_weapon::primary_for_tier(object, entity.veterancy)
                .and_then(|name| rules.weapon(name))
                .expect("strafe classifier's weapon");
            let distance = crate::sim::combat::object_distance_to(
                entity,
                &target,
                &self.substrate.entities,
                rules,
                &self.interner,
            )
            .expect("live aircraft Target");
            if distance < weapon.range_leptons {
                return 4;
            }
            self.assign_aircraft_attack_destination(id, Some(navigation_target(target)), rules);
        } else if object.fighter
            || entity
                .locomotor
                .as_ref()
                .expect("Fly approach")
                .fly_current_speed
                == SIM_ZERO
        {
            // Fly IsMovingNow4CCAC0 reads actual speed+48, not request+34.
            return 4;
        }
        let entity = self.substrate.entities.get(id).unwrap();
        let Some(nav) = entity.navigation.nav_com else {
            return 1;
        };
        let distance = crate::sim::combat::object_distance_to(
            entity,
            &attack_target(nav),
            &self.substrate.entities,
            rules,
            &self.interner,
        )
        .expect("live aircraft NavCom");
        let facing = if distance < 512 {
            crate::sim::movement::turret::facing_toward_target(
                entity,
                &target,
                &self.substrate.entities,
                Some(rules),
                &self.interner,
            )
            .expect("live approach Target")
        } else {
            let object = rules
                .object(self.interner.resolve(entity.type_ref()))
                .unwrap();
            let snap =
                crate::sim::combat::build_attacker_snapshot(entity, target, None, None, None);
            let origin = fire_coord::fire_coordinate(
                self,
                rules,
                &fire_coord::FireSource::from(&snap),
                object,
                combat_weapon::WeaponSlot::Primary,
                entity.weapon_burst.index() as u8,
            )
            .coord;
            //4181F6..41828E: Nav+48 and GetFLH(weapon0, additive zero XYZ).
            // Reuse the muzzle owner, including its documented tilt residual.
            let destination = self
                .fire_location_center(nav, rules)
                .expect("live approach NavCom");
            crate::util::direction_tables::facing16_between(
                [origin.x, origin.y],
                [destination.x, destination.y],
            )
        };
        let entity = self.substrate.entities.get_mut(id).unwrap();
        air_movement::ensure_fly_facings(entity);
        entity
            .barrel_facing
            .as_mut()
            .unwrap()
            .set(facing, self.session.binary_frame);
        if distance < 16 {
            self.assign_aircraft_attack_destination(id, None, rules);
            4
        } else {
            3
        }
    }

    /// `AircraftMission::Attack` is the sole owner of Mission+BC for aircraft;
    /// MissionState::handler_state is not mirrored for this class. Writes the
    /// visit's mission delay and returns the mission holding its state.
    pub(crate) fn aircraft_attack_visit(
        &mut self,
        id: u64,
        state: u8,
        delay: i32,
    ) -> AircraftMission {
        let entity = self.substrate.entities.get_mut(id).unwrap();
        entity
            .mission
            .write_dispatch_epilogue(self.session.binary_frame as i32, delay);
        AircraftMission::Attack { sub_state: state }
    }

    /// The Target test each strike state opens with (`0x004182A3` also
    /// leaves on Ammo 0), taken here because VERA's combat phase visits only
    /// an object that holds a target. `Some` asks the combat phase for the
    /// visit; `None` leaves for state 10.
    pub(crate) fn aircraft_strike_target(&self, id: u64, state: u8) -> Option<TargetKind> {
        let entity = self.substrate.entities.get(id).expect("aircraft dispatch");
        let attack = entity.attack_target.as_ref()?;
        if !attack_mission::aircraft_target_present(Some(attack), &self.substrate.entities) {
            return None;
        }
        if state == 4
            && entity
                .aircraft_ammo
                .as_ref()
                .is_some_and(|a| a.current == 0)
        {
            return None;
        }
        Some(attack.target)
    }

    /// State 10 (`0x00418BEC`) after the entry prefix. Returns the mission
    /// and whether the visit ends in `Enter_Idle_Mode(0, 1)` (`vt+0x484`),
    /// which the dispatch applies in the same visit.
    pub(crate) fn aircraft_exit(&mut self, id: u64, rules: &RuleSet) -> (AircraftMission, bool) {
        let entity = self.substrate.entities.get(id).expect("aircraft dispatch");
        let facts = attack_mission::ExitFacts {
            ammo: entity.aircraft_ammo.as_ref().map_or(-1, |a| a.current),
            target: attack_mission::aircraft_target_present(
                entity.attack_target.as_ref(),
                &self.substrate.entities,
            ),
            leaves_map: entity.is_mission_only(),
            human: self
                .houses
                .get(&entity.owner())
                .is_some_and(|house| house.is_controlled_by_human(self.session.game_mode_nonzero)),
            airstrike: entity
                .mission_leaf
                .as_aircraft()
                .is_some_and(|leaf| leaf.airstrike_manager_present()),
        };
        let mut host = WorldExit {
            sim: self,
            id,
            rules,
            idle: false,
        };
        let visit = attack_mission::exit_visit(&facts, &mut host);
        let idle = host.idle;
        if let Some(latch) = visit.latch
            && let Some(entity) = self.substrate.entities.get_mut(id)
            && entity.mission_leaf.as_aircraft().is_some()
        {
            entity.mission_leaf.set_aircraft_action_latch(latch);
        }
        (
            self.aircraft_attack_visit(id, visit.state, visit.delay),
            idle,
        )
    }
}

/// State 10's world: the target, the own-edge destination and the idle exit.
struct WorldExit<'a> {
    sim: &'a mut Simulation,
    id: u64,
    rules: &'a RuleSet,
    idle: bool,
}

impl attack_mission::ExitHost for WorldExit<'_> {
    fn clear_target(&mut self) {
        if let Some(entity) = self.sim.substrate.entities.get_mut(self.id) {
            crate::sim::mission::concrete_effects::represented_assign_target(entity, None);
        }
    }

    fn assign_edge_destination(&mut self) {
        let sim = &mut *self.sim;
        let owner = sim.substrate.entities.get(self.id).map(|e| e.owner());
        let edge = crate::sim::world::edge_cell::Edge::own_edge(
            owner
                .and_then(|owner| sim.houses.get(&owner))
                .map_or(0, |house| house.waypoint_edge),
        );
        let cell = crate::sim::world::edge_cell::find_paradrop_edge_cell(
            sim.playfield_bounds,
            sim.resolved_terrain.as_ref(),
            edge,
            &mut sim.scenario_rng,
        );
        // RESIDUAL: with no playfield (headless fixtures) there is no edge to
        // pick and no draw; every loaded map has one.
        if let Some((rx, ry)) = cell {
            sim.assign_aircraft_attack_destination(
                self.id,
                Some(NavTargetRef::cell(rx, ry)),
                self.rules,
            );
        }
    }

    /// `vt+0x1E8 Queue_Mission(Retreat, 0)` for an Airstrike aircraft with
    /// ammo. RESIDUAL: VERA has no Airstrike (no producer sets the leaf's
    /// `+0x294` byte) and no aircraft Retreat mission, so this queues nothing.
    fn retreat(&mut self) {}

    fn enter_idle_mode(&mut self) {
        self.idle = true;
    }
}
