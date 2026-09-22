//! World effects of Aircraft Mission_Attack417FE0. The remaining attack states
//! are being migrated from aircraft::attack_mission; this owns state1's search,
//! destination transaction and Scenario RNG, in live actor order.
use super::Simulation;
use crate::rules::ruleset::RuleSet;
use crate::sim::aircraft::AircraftMission;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::mission::MissionType;

impl Simulation {
    ///418031..41809C, after enter_attack_state consumes pending ammo/clears
    /// readiness. A refused void destination setter can retain the OLD NavCom;
    /// neither FindFireLocation's result nor Fly MoveTo's adapter bool decides
    /// whether state3 is entered. Executable corpus: aircraft_reengagement.*.
    pub(crate) fn aircraft_reengage(&mut self, id: u64, rules: &RuleSet) -> AircraftMission {
        let entity = self.substrate.entities.get(id).expect("aircraft dispatch");
        let target = entity.attack_target.as_ref().map(|a| match a.target {
            TargetKind::Entity(id) => NavTargetRef::Entity { id },
            TargetKind::Cell(rx, ry) => NavTargetRef::cell(rx, ry),
        });
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
        let entity = self.substrate.entities.get_mut(id).unwrap();
        entity.mission.set_handler_state(state);
        entity
            .mission
            .write_dispatch_epilogue(self.session.binary_frame as i32, delay);
        AircraftMission::Attack {
            sub_state: state as u8,
        }
    }
}
