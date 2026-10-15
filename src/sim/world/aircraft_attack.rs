//! World effects of Aircraft Mission_Attack417FE0 that run in the aircraft's
//! dispatch, in live actor order: states 0/1/3's fire-location search,
//! approach steering and destination transactions, and state 10's exit
//! (`aircraft::attack_mission::exit_visit`). State 1 consumes Scenario RNG;
//! state 3 returns a one-tick delay. States 4..9 run in the combat phase
//! (`combat::aircraft_release`).
use super::{FrameEffects, Simulation};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::aircraft::{attack_mission, enter_idle_mode_for};
use crate::sim::combat::TargetKind;
use crate::sim::combat::{combat_weapon, fire_coord};
use crate::sim::components::NavTargetRef;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::air_movement;
use crate::sim::world::edge_cell::Edge;

impl Simulation {
    ///418006..418030: raw Target presence chooses1/10, preserving pending ammo.
    /// The caller already cleared readiness. Search belongs to the next visit.
    /// Each visit returns Mission+0xBC and the frames it returns.
    pub(crate) fn aircraft_begin_attack(&mut self, id: u64) -> (u8, i32) {
        let present = crate::sim::combat::target_present(
            self.substrate
                .entities
                .get(id)
                .expect("aircraft dispatch")
                .attack_target
                .target(),
            &self.substrate.entities,
        );
        (if present { 1 } else { 10 }, 1)
    }

    ///418031..41809C, after enter_attack_state consumes pending ammo/clears
    /// readiness. A refused void destination setter can retain the OLD NavCom;
    /// neither FindFireLocation's result nor Fly MoveTo's adapter bool decides
    /// whether state3 is entered. Executable corpus: aircraft_reengagement.*.
    pub(crate) fn aircraft_reengage(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> (u8, i32) {
        let entity = self.substrate.entities.get(id).expect("aircraft dispatch");
        let target = entity
            .attack_target
            .target()
            .filter(|&target| {
                crate::sim::combat::target_present(Some(target), &self.substrate.entities)
            })
            .map(NavTargetRef::from);
        let ammo = entity.aircraft_ammo.as_ref().map_or(-1, |a| a.current());
        let state = if target.is_some() && ammo != 0 {
            let destination = self.aircraft_find_fire_location(id, target, rules);
            self.assign_aircraft_destination(id, destination, rules, effects);
            if self
                .substrate
                .entities
                .get(id)
                .unwrap()
                .navigation
                .nav_com()
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
        (
            state,
            self.mission_rate_epilogue(rules, MissionType::Attack),
        )
    }

    ///4180A1..4182A2 after the shared entry prefix. Strafe classification wins
    /// over Fighter. Other aircraft approach their retained firing position,
    /// not a freshly substituted target cell. Corpus: aircraft_approach.*.
    pub(crate) fn aircraft_approach(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> (u8, i32) {
        (self.advance_aircraft_approach(id, rules, effects), 1)
    }

    fn advance_aircraft_approach(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> u8 {
        let entity = self
            .substrate
            .entities
            .get(id)
            .expect("aircraft approach owner");
        let Some(target) = entity.attack_target.target().filter(|&target| {
            crate::sim::combat::target_present(Some(target), &self.substrate.entities)
        }) else {
            return 10;
        };
        if entity
            .aircraft_ammo
            .as_ref()
            .is_some_and(|a| a.current() == 0)
        {
            return 10;
        }
        let object = rules
            .object(self.interner.resolve(entity.type_ref()))
            .expect("aircraft type");
        if combat_weapon::aircraft_strafes(rules, object, entity.veterancy.rank()) {
            let weapon = combat_weapon::primary_for_tier(object, entity.veterancy.rank())
                .and_then(|name| rules.weapon(name))
                .expect("strafe classifier's weapon");
            let distance =
                crate::sim::combat::object_distance_to(entity, &target, &self.substrate.entities)
                    .expect("live aircraft Target");
            if distance < weapon.range_leptons {
                return 4;
            }
            self.assign_aircraft_destination(id, Some(target.into()), rules, effects);
        } else if object.fighter
            || !crate::sim::movement::motion_query::is_moving_now(
                entity,
                None,
                self.session.binary_frame,
            )
        {
            // The locomotor's Is_Moving_Now: Fly `0x004CCAC0` reads the speed
            // at +48, not the +34 request.
            return 4;
        }
        let entity = self.substrate.entities.get(id).unwrap();
        let Some(nav) = entity.navigation.nav_com() else {
            return 1;
        };
        let distance = crate::sim::combat::object_distance_to(
            entity,
            &TargetKind::from(nav),
            &self.substrate.entities,
        )
        .expect("live aircraft NavCom");
        let facing = if distance < 512 {
            crate::sim::movement::turret::facing_toward_target(
                entity,
                &target,
                &self.substrate.entities,
            )
            .expect("live approach Target")
        } else {
            let object = rules
                .object(self.interner.resolve(entity.type_ref()))
                .unwrap();
            let snap = crate::sim::combat::build_attacker_snapshot(entity, target, None);
            let origin = fire_coord::fire_coordinate(
                self,
                rules,
                &fire_coord::FireSource::from(&snap),
                object,
                0,
                entity.weapon_burst.index() as u8,
                crate::rules::flh::Flh::default(),
            )
            .coord;
            //4181F6..41828E: Nav+48 and GetFLH(weapon0, additive zero XYZ).
            // Reuse the muzzle owner, including its documented tilt residual.
            let destination = self
                .fire_location_center(nav)
                .expect("live approach NavCom");
            crate::util::direction_tables::facing16_between(
                [origin.x, origin.y],
                [destination.x, destination.y],
            )
        };
        let entity = self.substrate.entities.get_mut(id).unwrap();
        air_movement::ensure_fly_secondary_facing(entity);
        entity
            .barrel_facing
            .as_mut()
            .unwrap()
            .set(facing, self.session.binary_frame);
        if distance < 16 {
            self.assign_aircraft_destination(id, None, rules, effects);
            4
        } else {
            3
        }
    }

    /// [`attack_mission::strike_leaves`], taken here because VERA's combat
    /// phase visits only an object that holds a target: whether the combat
    /// phase runs the strike visit, else the aircraft leaves for state 10.
    /// `AircraftClass::Mission_Attack @ 0x00417FE0`'s strike-state test of TarCom (`+0x2B4`)
    /// and Ammo (`+0x2FC`, state 4 at `0x004182A3`).
    pub(crate) fn aircraft_strikes(&self, id: u64, state: u8) -> bool {
        let entity = self.substrate.entities.get(id).expect("aircraft dispatch");
        let attack = entity.attack_target.target();
        let target = crate::sim::combat::target_present(attack, &self.substrate.entities);
        let ammo = entity.aircraft_ammo.as_ref().map_or(-1, |a| a.current());
        attack.is_some() && !attack_mission::strike_leaves(state, target, ammo)
    }

    /// State 10 (`0x00418BEC`) after the entry prefix.
    pub(crate) fn aircraft_exit(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> (u8, i32) {
        let entity = self.substrate.entities.get(id).expect("aircraft dispatch");
        let facts = attack_mission::ExitFacts {
            ammo: entity.aircraft_ammo.as_ref().map_or(-1, |a| a.current()),
            target: crate::sim::combat::target_present(
                entity.attack_target.target(),
                &self.substrate.entities,
            ),
            leaves_map: entity.is_mission_only(),
            human: self.owner_is_human(entity.owner()),
            airstrike: entity.airstrike_link().is_some(),
        };
        let visit = attack_mission::exit_visit(
            &facts,
            &mut WorldExit {
                sim: self,
                id,
                rules,
                registry,
                effects,
            },
        );
        if let Some(latch) = visit.latch
            && let Some(entity) = self.substrate.entities.get_mut(id)
            && entity.mission_leaf.as_aircraft().is_some()
        {
            entity.mission_leaf.set_aircraft_action_latch(latch);
        }
        (visit.state, visit.delay)
    }
}

/// State 10's world: the target, the own-edge destination and the idle exit.
struct WorldExit<'a> {
    sim: &'a mut Simulation,
    id: u64,
    rules: &'a RuleSet,
    registry: Option<&'a OverlayTypeRegistry>,
    effects: FrameEffects<'a>,
}

impl attack_mission::ExitHost for WorldExit<'_> {
    fn clear_target(&mut self) {
        self.sim
            .assign_target_represented(
                self.id,
                None,
                Some(self.rules),
                None,
                FrameEffects::default(),
            )
            .expect("aircraft dispatch");
    }

    fn assign_edge_destination(&mut self) {
        let edge = Edge::own_edge(self.sim.aircraft_house_waypoint_edge(self.id));
        if let Some((rx, ry)) = self.sim.aircraft_edge_cell(edge) {
            self.sim.assign_aircraft_destination(
                self.id,
                Some(NavTargetRef::cell(rx, ry)),
                self.rules,
                self.effects,
            );
        }
    }

    /// 418CAD..418CB4: a linked plane with ammunition retreats when its
    /// designated target has gone away. The manager link remains until removal.
    fn retreat(&mut self) {
        let entity = self.sim.substrate.entities.get_mut(self.id).unwrap();
        crate::sim::mission::authority::queue_entity_mission_deferred(
            entity,
            MissionId::from_known(MissionType::Retreat),
        );
    }

    fn enter_idle_mode(&mut self) {
        enter_idle_mode_for(self.sim, self.id, self.rules, self.registry, self.effects);
    }
}
