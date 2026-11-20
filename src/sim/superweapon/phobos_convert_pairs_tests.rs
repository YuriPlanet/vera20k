//! Phobos `TypeConvertGroup` through the production superweapon launch.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, Simulation};

fn rules(sw_keys: &str) -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[VehicleTypes]\n0=GRUNT\n1=UPGRADED\n\
         [GRUNT]\nStrength=100\nSpeed=4\n\
         [UPGRADED]\nStrength=100\nSpeed=4\n\
         [SuperWeaponTypes]\n0=ConvSW\n\
         [ConvSW]\nUIName=ConvSW\n{sw_keys}"
    )))
    .expect("convert pairs rules")
}

fn world(rules: &RuleSet) -> (Simulation, InternedId, Vec<u64>, u64) {
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    for (name, side) in [("Americans", 0), ("Russians", 1)] {
        let id = sim.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, false, 0, 10));
    }
    let americans = sim.intern("Americans");
    let ours = vec![
        sim.spawn_object("GRUNT", "Americans", 10, 10, 0, rules)
            .expect("GRUNT spawns"),
        sim.spawn_object("GRUNT", "Americans", 11, 10, 0, rules)
            .expect("GRUNT spawns"),
    ];
    let theirs = sim
        .spawn_object("GRUNT", "Russians", 12, 10, 0, rules)
        .expect("GRUNT spawns");
    (sim, americans, ours, theirs)
}

fn kind(sim: &Simulation, id: u64) -> String {
    sim.interner
        .resolve(sim.entities().get(id).unwrap().type_ref())
        .to_owned()
}

fn launch(sim: &mut Simulation, rules: &RuleSet, owner: InternedId) {
    let sw_type = crate::sim::superweapon::chronosphere_tests::charge_super(sim, owner, "ConvSW");
    assert!(sim.fire_super_weapon(
        rules,
        owner,
        sw_type,
        (10, 10),
        None,
        FrameEffects::default(),
        false,
    ));
}

/// `SWTypeExt::ApplyTypeConversion` (`Ext/SWType/FireSuperWeapon.cpp:350-354`):
/// every Foot of the launching house converts; an enemy Foot is filtered by
/// the default `AffectsHouse=Owner`.
#[test]
fn a_superweapon_converts_its_owners_feet() {
    let rules = rules("Convert0.To=UPGRADED\n");
    let (mut sim, americans, ours, theirs) = world(&rules);
    launch(&mut sim, &rules, americans);
    for id in ours {
        assert_eq!(kind(&sim, id), "UPGRADED");
    }
    assert_eq!(
        kind(&sim, theirs),
        "GRUNT",
        "the default Owner filter skips an enemy Foot"
    );
}

/// The numbered list continues past a non-matching `From` row, and
/// `AffectsHouse=All` admits the enemy Foot too.
#[test]
fn numbered_rows_match_by_source_type_and_house_filter() {
    let rules = rules(
        "Convert0.From=UPGRADED\nConvert0.To=UPGRADED\n\
         Convert1.From=GRUNT\nConvert1.To=UPGRADED\nConvert1.AffectsHouse=all\n",
    );
    let (mut sim, americans, ours, theirs) = world(&rules);
    launch(&mut sim, &rules, americans);
    for id in ours {
        assert_eq!(kind(&sim, id), "UPGRADED");
    }
    assert_eq!(kind(&sim, theirs), "UPGRADED");
}
