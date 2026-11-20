//! Phobos `Convert.HumanToComputer` / `Convert.ComputerToHuman` and the
//! owner-change mission fix through the production `ChangeOwner` path.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{FrameEffects, Simulation};

fn rules() -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=HW\n1=H2C\n2=C2H\n3=MINER\n\
         [HW]\nStrength=100\nSpeed=4\nConvert.HumanToComputer=H2C\n\
         [H2C]\nStrength=100\nSpeed=4\nConvert.ComputerToHuman=HW\n\
         [C2H]\nStrength=100\nSpeed=4\nConvert.ComputerToHuman=HW\nDefaultToGuardArea=yes\n\
         [MINER]\nStrength=100\nSpeed=4\nResourceGatherer=yes\nDefaultToGuardArea=yes\n\
         Convert.ComputerToHuman=HW\n",
    ))
    .expect("convert owner rules")
}

fn world(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    for (name, side, human) in [
        ("Americans", 0, true),
        ("Russians", 1, false),
        ("French", 2, true),
    ] {
        let id = sim.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, human, 0, 10));
    }
    sim
}

fn kind(sim: &Simulation, id: u64) -> String {
    sim.interner
        .resolve(sim.entities().get(id).unwrap().type_ref())
        .to_owned()
}

fn owner(sim: &Simulation, name: &str) -> crate::sim::intern::InternedId {
    sim.interner().get(name).expect("registered house")
}

/// `Ext/House/Hooks.cpp:203-300`: a human-to-computer change converts through
/// the old type's `Convert.HumanToComputer`; a same-kind change does not.
#[test]
fn a_human_to_computer_change_converts_the_foot() {
    let rules = rules();
    let mut sim = world(&rules);
    let unit = sim
        .spawn_object("HW", "Americans", 10, 10, 0, &rules)
        .expect("HW spawns");
    let ai = owner(&sim, "Russians");
    sim.change_owner_with_rules(unit, ai, &rules, None, FrameEffects::default());
    assert_eq!(kind(&sim, unit), "H2C");

    // Human to human keeps the type even with the key set.
    let rules = self::rules();
    let mut sim = world(&rules);
    let unit = sim
        .spawn_object("HW", "Americans", 10, 10, 0, &rules)
        .expect("HW spawns");
    let french = owner(&sim, "French");
    sim.change_owner_with_rules(unit, french, &rules, None, FrameEffects::default());
    assert_eq!(kind(&sim, unit), "HW");
}

/// A computer-to-human change converts through the old type's
/// `Convert.ComputerToHuman` and then runs `ChangeOwnerMissionFix`, which
/// queues `AreaGuard` because the old type sets `DefaultToGuardArea=yes`.
#[test]
fn a_computer_to_human_change_converts_and_fixes_the_mission() {
    let rules = rules();
    let mut sim = world(&rules);
    let unit = sim
        .spawn_object("C2H", "Russians", 10, 10, 0, &rules)
        .expect("C2H spawns");
    sim.substrate
        .entities
        .get_mut(unit)
        .unwrap()
        .mission
        .set_current_for_test(MissionId::from_known(MissionType::Guard), 0);
    let human = owner(&sim, "Americans");
    sim.change_owner_with_rules(unit, human, &rules, None, FrameEffects::default());
    assert_eq!(kind(&sim, unit), "HW");
    let actor = sim.entities().get(unit).unwrap();
    let area_guard = MissionId::from_known(MissionType::AreaGuard);
    assert!(
        [actor.mission.current(), actor.mission.queued()].contains(&area_guard),
        "the mission fix queues AreaGuard: {:?}/{:?}",
        actor.mission.current(),
        actor.mission.queued()
    );
}

/// The mission fix returns early for a `ResourceGatherer=` old type: the
/// conversion still happens, but no AreaGuard is queued.
#[test]
fn a_resource_gatherer_skips_the_mission_fix() {
    let rules = rules();
    let mut sim = world(&rules);
    let miner = sim
        .spawn_object("MINER", "Russians", 10, 10, 0, &rules)
        .expect("MINER spawns");
    sim.substrate
        .entities
        .get_mut(miner)
        .unwrap()
        .mission
        .set_current_for_test(MissionId::from_known(MissionType::Harvest), 0);
    let human = owner(&sim, "Americans");
    sim.change_owner_with_rules(miner, human, &rules, None, FrameEffects::default());
    assert_eq!(kind(&sim, miner), "HW");
    let actor = sim.entities().get(miner).unwrap();
    assert!(
        ![actor.mission.current(), actor.mission.queued()]
            .contains(&MissionId::from_known(MissionType::AreaGuard)),
        "the resource gatherer skips the mission fix"
    );
}
