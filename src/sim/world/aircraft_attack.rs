//! World effects of Aircraft Mission_Attack417FE0. The remaining attack states
//! are being migrated from aircraft::attack_mission; this owns states0/1/3's
//! fire-location search, approach steering and destination transactions, in
//! live actor order. State1 consumes Scenario RNG; state3 returns a one-tick delay.
use super::Simulation;
use crate::rules::ruleset::RuleSet;
use crate::sim::aircraft::AircraftMission;
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
        let present = self
            .substrate
            .entities
            .get(id)
            .expect("aircraft dispatch")
            .attack_target
            .is_some();
        self.commit_aircraft_attack_visit(id, if present { 1 } else { 10 }, 1)
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
        let delay = (rules.mission_control.rate_frames(MissionType::Attack) as i32)
            .wrapping_add(self.scenario_rng.next_range_i32_inclusive(0, 2));
        self.commit_aircraft_attack_visit(id, state, delay)
    }

    ///4180A1..4182A2 after the shared entry prefix. Strafe classification wins
    /// over Fighter. Other aircraft approach their retained firing position,
    /// not a freshly substituted target cell. Corpus: aircraft_approach.*.
    pub(crate) fn aircraft_approach(&mut self, id: u64, rules: &RuleSet) -> AircraftMission {
        let state = self.advance_aircraft_approach(id, rules);
        self.commit_aircraft_attack_visit(id, state, 1)
    }

    fn advance_aircraft_approach(&mut self, id: u64, rules: &RuleSet) -> u32 {
        let entity = self
            .substrate
            .entities
            .get(id)
            .expect("aircraft approach owner");
        let Some(target) = entity.attack_target.as_ref().map(|a| a.target) else {
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
                crate::sim::combat::build_attacker_snapshot(entity, target, 0, 0, None, None, None);
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
    /// MissionState::handler_state is not mirrored for this class.
    fn commit_aircraft_attack_visit(&mut self, id: u64, state: u32, delay: i32) -> AircraftMission {
        let entity = self.substrate.entities.get_mut(id).unwrap();
        entity
            .mission
            .write_dispatch_epilogue(self.session.binary_frame as i32, delay);
        AircraftMission::Attack {
            sub_state: state as u8,
        }
    }
}
