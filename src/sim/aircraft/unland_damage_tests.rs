//! Phobos `AircraftClass_Update_UnlandableDamage` (`Ext/Aircraft/Hooks.cpp:
//! 735-794`, hook `0x414DA8`): an airport-bound aircraft that stays in the air
//! takes `ExtendedAircraftMissions.UnlandDamage` every fourth frame unless it
//! is already heading to a dock, and a negative value crashes it while the
//! master switch is on. The frames the step visits are the production
//! `advance_tick` AI pass.

#![cfg(test)]

use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::world::Simulation;

/// `{keys}` is filled with the ORCA's `ExtendedAircraftMissions.*` lines.
const RULES: &str = "\
[General]\nFlightLevel=1500\n\
[CombatDamage]\nC4Warhead=Super\n[Warheads]\n0=Super\n\
[Super]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
[AircraftTypes]\n0=ORCA\n[BuildingTypes]\n0=GAAIRC\n\
[ORCA]\nStrength=150\nSpeed=14\nAmmo=1\nLandable=yes\nAirportBound=yes\nFighter=yes\n\
Dock=GAAIRC\nLocomotor={4A582746-9839-11D1-B709-00A024DDAFD1}\n\
{keys}\
[GAAIRC]\nStrength=1000\nFoundation=2x2\nHelipad=yes\nUnitReload=yes\nNumberOfDocks=4\n";

fn rules(keys: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&RULES.replace("{keys}", keys))).expect("rules")
}

/// A human house's ORCA high in the air with its airfield far away, so it
/// neither lands nor idles on the ground during the measured frames.
fn airborne_orca(keys: &str) -> (Simulation, RuleSet, u64, u64) {
    let rules = rules(keys);
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    let pad = sim
        .spawn_object("GAAIRC", "Americans", 55, 55, 0, &rules)
        .expect("airfield");
    let orca = sim
        .spawn_object("ORCA", "Americans", 5, 5, 0, &rules)
        .expect("aircraft");
    sim.substrate
        .entities
        .get_mut(orca)
        .unwrap()
        .position
        .put_z(1500);
    (sim, rules, orca, pad)
}

fn health(sim: &Simulation, id: u64) -> i32 {
    sim.substrate.entities.get(id).unwrap().health.current()
}

#[test]
fn a_positive_unland_damage_hurts_an_airborne_aircraft_every_fourth_frame() {
    let (mut sim, rules, orca, _) = airborne_orca("ExtendedAircraftMissions.UnlandDamage=5\n");
    let initial = health(&sim, orca);
    for _ in 0..40 {
        let _ = sim.advance_tick(&[], Some(&rules), None, None, 33);
    }
    let lost = initial - health(&sim, orca);
    // `(CurrentFrame - LastFireBulletFrame + UniqueID) & 3`, so the 40 frames
    // hold about ten 5-point hits and nothing else damages the aircraft.
    assert_eq!(lost % 5, 0, "each hit is the configured 5 ({lost} lost)");
    assert!(lost >= 25, "it keeps taking hits in the air ({lost} lost)");
}

#[test]
fn a_docked_aircraft_takes_no_unland_damage() {
    // Phobos `AircraftClass_Update_UnlandableDamage` (`Ext/Aircraft/Hooks.cpp:
    // 762-770`): an aircraft that already holds a `DockNowHeadingTo` takes
    // neither damage arm.
    let (mut sim, rules, orca, pad) = airborne_orca("ExtendedAircraftMissions.UnlandDamage=5\n");
    sim.substrate.set_aircraft_dock(orca, Some(pad));
    let initial = health(&sim, orca);
    for _ in 0..40 {
        let _ = sim.advance_tick(&[], Some(&rules), None, None, 33);
    }
    assert_eq!(
        health(&sim, orca),
        initial,
        "a docked aircraft takes no hits"
    );
}

#[test]
fn a_negative_unland_damage_crashes_an_airborne_aircraft_with_the_master_on() {
    let (mut sim, rules, orca, _) =
        airborne_orca("ExtendedAircraftMissions=yes\nExtendedAircraftMissions.UnlandDamage=-1\n");
    let mut crashed = false;
    for _ in 0..240 {
        let _ = sim.advance_tick(&[], Some(&rules), None, None, 33);
        if sim
            .substrate
            .entities
            .get(orca)
            .is_none_or(|entity| entity.health.current() <= 0)
        {
            crashed = true;
            break;
        }
    }
    assert!(crashed, "the negative damage kills it instead of landing");
}
