//! Phobos `Convert.Health*` through the production Foot AI tail.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

fn rules(keys: &str) -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[VehicleTypes]\n0=CONV\n1=NEXT\n\
         [CONV]\nStrength=100\nSpeed=4\n{keys}\
         [NEXT]\nStrength=100\nSpeed=4\n"
    )))
    .expect("convert health rules")
}

/// One AI visit of a `CONV` at `health`: whether it became `NEXT`.
fn converted_after_one_visit(keys: &str, health: i32) -> bool {
    let rules = rules(keys);
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let id = sim
        .spawn_object("CONV", "Americans", 10, 10, 0, &rules)
        .expect("CONV spawns");
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .health
        .set_for_test(health);
    sim.object_ai_stage(Some(&rules));
    sim.interner
        .resolve(sim.entities().get(id).unwrap().type_ref())
        == "NEXT"
}

/// `Ext/Foot/Body.cpp:909-927`: the band is `hp > AbovePercent` (or `>=`
/// when the ratio is not positive) and `hp <= BelowPercent`; both negative
/// bounds disable the arm.
#[test]
fn converts_inside_the_health_band() {
    let keys = "Convert.Health=NEXT\nConvert.Health.AbovePercent=50%\n\
                Convert.Health.BelowPercent=80%\n";
    assert!(converted_after_one_visit(keys, 60));
    assert!(
        converted_after_one_visit(keys, 80),
        "the upper bound is inclusive"
    );
    assert!(!converted_after_one_visit(keys, 90));
    assert!(
        !converted_after_one_visit(keys, 50),
        "the lower bound is exclusive"
    );
    assert!(!converted_after_one_visit(keys, 40));
}

/// `Ext/TechnoType/Body.cpp:1197` swaps a reversed pair; an absent target or
/// two negative bounds leave the object alone.
#[test]
fn disabled_and_reversed_bands() {
    assert!(!converted_after_one_visit("", 60), "no key, no conversion");
    assert!(
        !converted_after_one_visit("Convert.Health=NEXT\n", 60),
        "both bounds negative disables the arm"
    );
    assert!(
        converted_after_one_visit(
            "Convert.Health=NEXT\nConvert.Health.AbovePercent=90%\n\
             Convert.Health.BelowPercent=10%\n",
            60
        ),
        "a reversed pair is swapped and still converts inside the band"
    );
}
