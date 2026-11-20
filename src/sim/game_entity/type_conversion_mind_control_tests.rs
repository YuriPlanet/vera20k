//! Phobos `Convert.ResetMindControl` through a production conversion.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::capture_manager::CaptureManagerState;
use crate::sim::house_state::HouseState;
use crate::sim::world::{FrameEffects, Simulation};

fn rules(target: &str, reset: &str, second_damage: i32, second_infinite: bool) -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[VehicleTypes]\n0=PSI\n1=PLAIN\n2=PSI2\n3=VICTIM\n\
         [PSI]\nStrength=100\nSpeed=4\nPrimary=MindGun\n\
         Convert.Health={target}\nConvert.Health.AbovePercent=-1\n\
         Convert.Health.BelowPercent=100%\n{reset}\
         [PLAIN]\nStrength=100\nSpeed=4\n\
         [PSI2]\nStrength=100\nSpeed=4\nPrimary=MindGun2\n\
         [VICTIM]\nStrength=100\nSpeed=4\n\
         [MindGun]\nDamage=1\nRange=6\nROF=20\nWarhead=MINDWH\n\
         [MindGun2]\nDamage={second_damage}\nRange=6\nROF=20\nWarhead=MIND2WH\n\
         InfiniteMindControl={}\n\
         [Warheads]\n0=MINDWH\n1=MIND2WH\n2=GUARDWH\n\
         [MINDWH]\nMindControl=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [MIND2WH]\nMindControl=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [GUARDWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        if second_infinite { "yes" } else { "no" }
    )))
    .expect("convert reset-mind-control rules")
}

fn world(rules: &RuleSet) -> (Simulation, u64) {
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    for (name, side) in [("Americans", 0), ("Russians", 1)] {
        let id = sim.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, false, 0, 10));
    }
    let psi = sim
        .spawn_object("PSI", "Americans", 10, 10, 0, rules)
        .expect("PSI spawns");
    (sim, psi)
}

fn convert(sim: &mut Simulation, rules: &RuleSet) {
    sim.object_ai_stage(Some(rules));
}

/// With the key and a target that has no mind-control weapon, the manager is
/// freed and deleted (`Ext/Foot/Body.cpp:524-533`).
#[test]
fn a_disarmed_target_deletes_the_manager() {
    let rules = rules("PLAIN", "Convert.ResetMindControl=yes\n", 1, false);
    let (mut sim, psi) = world(&rules);
    assert_eq!(
        sim.entities().get(psi).unwrap().capture_manager,
        Some(Box::new(CaptureManagerState::constructed(1, false))),
        "the controller starts with its primary mind-control limit"
    );
    convert(&mut sim, &rules);
    assert_eq!(sim.entities().get(psi).unwrap().type_ref(), {
        sim.intern("PLAIN")
    });
    assert!(
        sim.entities().get(psi).unwrap().capture_manager.is_none(),
        "no mind-control weapon remains: the manager is deleted"
    );
}

/// The new type's strongest mind-control weapon and its infinite flag become
/// the manager's limits (`:511-523`), and without the key nothing changes.
#[test]
fn the_new_limit_and_infinite_flag_are_adopted() {
    let rules = rules("PSI2", "Convert.ResetMindControl=yes\n", 3, true);
    let (mut sim, psi) = world(&rules);
    convert(&mut sim, &rules);
    assert_eq!(
        sim.entities().get(psi).unwrap().capture_manager,
        Some(Box::new(CaptureManagerState::constructed(3, true))),
        "max capture 3 and InfiniteMindControl=yes are adopted"
    );

    let rules = self::rules("PLAIN", "", 1, false);
    let (mut sim, psi) = world(&rules);
    convert(&mut sim, &rules);
    assert_eq!(
        sim.entities().get(psi).unwrap().capture_manager,
        Some(Box::new(CaptureManagerState::constructed(1, false))),
        "without the key the old manager survives"
    );
}

/// With the node count over the new finite limit, the newest nodes are freed
/// and their houses restored (`:511-520`).
#[test]
fn excess_nodes_are_freed_newest_first() {
    let rules = rules("PSI2", "Convert.ResetMindControl=yes\n", 1, false);
    let (mut sim, psi) = world(&rules);
    let mut victims = Vec::new();
    for at in [12u16, 13] {
        let victim = sim
            .spawn_object("VICTIM", "Americans", at, 10, 0, &rules)
            .expect("VICTIM spawns");
        victims.push(victim);
    }
    let russians = sim.intern("Russians");
    {
        let manager = sim
            .substrate
            .entities
            .get_mut(psi)
            .unwrap()
            .capture_manager
            .as_mut()
            .expect("manager");
        for victim in &victims {
            manager.seed_node_for_test(*victim, russians);
        }
    }
    convert(&mut sim, &rules);
    let manager = sim
        .entities()
        .get(psi)
        .unwrap()
        .capture_manager
        .as_ref()
        .expect("manager retained");
    assert_eq!(manager.count(), 1, "the limit keeps the oldest node");
    assert_eq!(
        sim.entities().get(victims[1]).unwrap().owner(),
        russians,
        "the newest node was freed and its house restored"
    );
    assert_eq!(
        sim.entities().get(victims[0]).unwrap().owner(),
        sim.intern("Americans"),
        "the kept node stays controlled"
    );
}
