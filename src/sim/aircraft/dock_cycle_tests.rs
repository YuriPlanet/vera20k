//! Production dock cycle through the live frame: an AirportBound aircraft
//! returns, publishes its airfield radio contact, lands through Fly
//! BeginLanding4CFA70/Process_Landing4CE840, reloads, and relaunches through
//! BeginTakeoff4CF950 back into the AirTracker.

use super::AircraftMission;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::movement::locomotor::AirMovePhase;
use crate::sim::world::Simulation;
use std::collections::BTreeMap;

const RULES: &str = "[General]\nFlightLevel=1500\n\
[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n0=ORCA\n[BuildingTypes]\n0=GAAIRC\n\
[ORCA]\nStrength=150\nSpeed=14\nAmmo=1\nLandable=yes\nAirportBound=yes\nFighter=yes\n\
Dock=GAAIRC\nLocomotor={4A582746-9839-11D1-B709-00A024DDAFD1}\n\
[GAAIRC]\nStrength=1000\nFoundation=2x2\nHelipad=yes\nUnitReload=yes\nNumberOfDocks=4\n";

#[derive(Default)]
struct Milestones {
    landed_with_contact: bool,
    reloaded: bool,
    relaunched_tracked: bool,
    contact_released: bool,
}

fn run_cycle(sim: &mut Simulation, rules: &RuleSet, orca: u64, airfield: u64) -> Milestones {
    let heights = BTreeMap::new();
    let mut seen = Milestones::default();
    for _ in 0..4000 {
        let _ = sim.advance_tick(&[], Some(rules), &heights, None, None, 33);
        let entity = sim.substrate.entities.get(orca).expect("aircraft survives");
        let phase = crate::sim::movement::air_movement::fly_mission_phase(
            entity,
            sim.resolved_terrain.as_ref(),
        );
        let docking_state = match entity.aircraft_mission {
            Some(AircraftMission::Docking { sub_state, .. }) => Some(sub_state),
            _ => None,
        };
        if phase == Some(AirMovePhase::Landed) && docking_state == Some(2) {
            seen.landed_with_contact |= entity.radio_contacts.contains(airfield)
                && sim
                    .substrate
                    .entities
                    .get(airfield)
                    .is_some_and(|af| af.radio_contacts.contains(orca));
        }
        if entity
            .aircraft_ammo
            .as_ref()
            .is_some_and(|a| a.current == a.max)
            && seen.landed_with_contact
        {
            seen.reloaded = true;
        }
        if seen.reloaded && docking_state == Some(3) {
            seen.contact_released |= !entity.radio_contacts.contains(airfield);
            let airborne = crate::sim::movement::air_movement::current_fly_height(
                entity,
                sim.resolved_terrain.as_ref(),
            ) > 0;
            if airborne {
                seen.relaunched_tracked |= entity.air_spatial_bucket.is_some();
            }
        }
        if seen.relaunched_tracked {
            break;
        }
    }
    seen
}

#[test]
fn airport_bound_aircraft_docks_reloads_and_relaunches_through_world_owners() {
    let rules = RuleSet::from_ini(&IniFile::from_str(RULES)).expect("rules");
    let mut sim = Simulation::new();
    let airfield = sim
        .spawn_object_at_height("GAAIRC", "Americans", 10, 10, 0, 0, &rules)
        .expect("airfield");
    let orca = sim
        .spawn_object_at_height("ORCA", "Americans", 24, 12, 0, 0, &rules)
        .expect("aircraft");
    // A spent strike craft: Guard with no ammo returns to its airfield.
    let entity = sim.substrate.entities.get_mut(orca).unwrap();
    entity.aircraft_ammo.as_mut().unwrap().current = 0;
    entity.aircraft_mission = Some(AircraftMission::Guard);

    let seen = run_cycle(&mut sim, &rules, orca, airfield);
    assert!(
        seen.landed_with_contact,
        "AirportBound landing needs the airfield's two-sided radio contact"
    );
    assert!(seen.reloaded, "a landed aircraft reloads on its pad");
    assert!(seen.contact_released, "the launch releases the pad contact");
    assert!(
        seen.relaunched_tracked,
        "BeginTakeoff4CF950 re-registers the relaunched aircraft in the AirTracker"
    );
}
