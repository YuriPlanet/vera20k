//! Phobos `Ammo.AutoConvert*` through the production Foot AI tail
//! (`Ext/Foot/Body.cpp:888-909`).

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::ammo::TechnoAmmo;
use crate::sim::world::Simulation;

fn rules(type_ammo: &str, keys: &str) -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[VehicleTypes]\n0=CONV\n1=NEXT\n\
         [CONV]\nStrength=100\nSpeed=4\n{type_ammo}{keys}\
         [NEXT]\nStrength=100\nSpeed=4\n"
    )))
    .expect("ammo convert rules")
}

/// One AI visit of a `CONV` with instance ammo `ammo`: whether it became
/// `NEXT`.
fn converted_after_one_visit(type_ammo: &str, keys: &str, ammo: i32) -> bool {
    let rules = rules(type_ammo, keys);
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let id = sim
        .spawn_object("CONV", "Americans", 10, 10, 0, &rules)
        .expect("CONV spawns");
    {
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        if entity.ammo().is_none() {
            entity.install_ammo_for_test(Some(TechnoAmmo::new(8)));
        }
        entity.ammo_mut().unwrap().set_current_for_test(ammo);
    }
    sim.object_ai_stage(Some(&rules));
    sim.interner
        .resolve(sim.entities().get(id).unwrap().type_ref())
        == "NEXT"
}

/// The band is `ammo >= min` and `ammo <= max`, a negative bound is
/// unbounded, and both negative disables the arm.
#[test]
fn converts_inside_the_ammo_band() {
    let keys = "Ammo.AutoConvertType=NEXT\nAmmo.AutoConvertMinimumAmount=2\n\
                Ammo.AutoConvertMaximumAmount=4\n";
    assert!(converted_after_one_visit("Ammo=8\n", keys, 2));
    assert!(converted_after_one_visit("Ammo=8\n", keys, 4));
    assert!(!converted_after_one_visit("Ammo=8\n", keys, 1));
    assert!(!converted_after_one_visit("Ammo=8\n", keys, 5));
    let min_only = "Ammo.AutoConvertType=NEXT\nAmmo.AutoConvertMinimumAmount=3\n";
    assert!(converted_after_one_visit("Ammo=8\n", min_only, 7));
    assert!(!converted_after_one_visit("Ammo=8\n", min_only, 2));
    let max_only = "Ammo.AutoConvertType=NEXT\nAmmo.AutoConvertMaximumAmount=2\n";
    assert!(converted_after_one_visit("Ammo=8\n", max_only, 1));
    assert!(!converted_after_one_visit("Ammo=8\n", max_only, 3));
    assert!(!converted_after_one_visit(
        "Ammo=8\n",
        "Ammo.AutoConvertType=NEXT\n",
        4
    ));
}

/// A type without `Ammo=` (capacity zero), a missing target and a reversed
/// pair never convert.
#[test]
fn disabled_ammo_conversion_arms() {
    let keys = "Ammo.AutoConvertType=NEXT\nAmmo.AutoConvertMinimumAmount=2\n\
                Ammo.AutoConvertMaximumAmount=4\n";
    assert!(!converted_after_one_visit("", keys, 3), "no type Ammo=");
    assert!(!converted_after_one_visit(
        "Ammo=8\n",
        "Ammo.AutoConvertMinimumAmount=2\nAmmo.AutoConvertMaximumAmount=4\n",
        3
    ));
    assert!(!converted_after_one_visit(
        "Ammo=8\n",
        "Ammo.AutoConvertType=NEXT\nAmmo.AutoConvertMinimumAmount=5\n\
         Ammo.AutoConvertMaximumAmount=2\n",
        3
    ));
    assert!(
        !converted_after_one_visit(
            "Ammo=8\n",
            "Ammo.AutoConvertType=NOSUCH\nAmmo.AutoConvertMinimumAmount=1\n",
            4
        ),
        "an unresolved target never converts"
    );
    assert!(
        !converted_after_one_visit(
            "Ammo=8\n",
            "Ammo.AutoConvertType=NEXT\nAmmo.AutoConvertMinimumAmount=abc\n",
            0
        ),
        "a failed bound read keeps the -1 default, leaving no band"
    );
}
