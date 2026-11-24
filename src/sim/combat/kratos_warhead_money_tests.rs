//! Kratos `ShowMeTheMoney` through the production area-damage path
//! (`MapClass::DamageArea @ 0x00489280`, hook `0x489280`).

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::{FrameEffects, Simulation};

/// Two rival houses on the flat arena, with `wh_ini` appended to the `[WH]`
/// body the firer's gun names.
fn scene(wh_ini: &str) -> (Simulation, RuleSet, u64, u64, InternedId) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=FIRER\n1=TARGET\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=1\nRange=5\nROF=20\nWarhead=WH\n\
         [Warheads]\n0=WH\n\
         [WH]\nCellSpread=1\n{wh_ini}\
         [FIRER]\nStrength=500\nArmor=none\nSpeed=6\nPrimary=GUN\n\
         [TARGET]\nStrength=500\nArmor=none\nSpeed=6\n"
    )))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x5A_AA_1E);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let firer = sim
        .spawn_object("FIRER", "Americans", 20, 20, 0, &rules)
        .expect("firer");
    let target = sim
        .spawn_object("TARGET", "Russians", 21, 20, 0, &rules)
        .expect("target");
    let owner = sim.entities().get(firer).expect("firer").owner();
    (sim, rules, firer, target, owner)
}

/// One area damage of `warhead` at the target's cell, by `firer`'s house.
fn detonate(sim: &mut Simulation, rules: &RuleSet, firer: u64, target: u64, warhead: &str) {
    let position = crate::sim::movement::ground_pose::position_world_coord(
        &sim.entities().get(target).unwrap().position,
    );
    let warhead_type = rules.warhead(warhead).expect("warhead");
    let warhead_id = sim.intern(warhead);
    let owner = sim.entities().get(firer).expect("firer").owner();
    crate::sim::combat::world_receiver::apply_area_damage(
        sim,
        rules,
        None,
        ProjectileCoord::new(position.x, position.y, position.z),
        1,
        warhead_type,
        false,
        (firer, Some(owner), warhead_id),
        &mut FrameEffects::empty(),
    );
}

fn credits(sim: &Simulation, owner: InternedId) -> i32 {
    sim.houses[&owner].economy().available_money()
}

/// A positive value pays the attacking house on every area damage, and the key
/// is also what turns the Kratos switch on.
#[test]
fn a_positive_value_pays_the_attacking_house() {
    let (mut sim, rules, firer, target, owner) = scene("ShowMeTheMoney=250\n");
    assert!(rules.kratos(), "the key turns the switch on");
    let before = credits(&sim, owner);

    detonate(&mut sim, &rules, firer, target, "WH");

    assert_eq!(credits(&sim, owner), before + 250);
}

/// A negative value takes credits away.
#[test]
fn a_negative_value_charges_the_attacking_house() {
    let (mut sim, rules, firer, target, owner) = scene("ShowMeTheMoney=-100\n");
    let before = credits(&sim, owner);

    detonate(&mut sim, &rules, firer, target, "WH");

    assert_eq!(credits(&sim, owner), before - 100);
}

/// Without the key nothing is paid, and the switch stays off.
#[test]
fn an_absent_key_pays_nothing() {
    let (mut sim, rules, firer, target, owner) = scene("");
    assert!(!rules.kratos());
    let before = credits(&sim, owner);

    detonate(&mut sim, &rules, firer, target, "WH");

    assert_eq!(credits(&sim, owner), before);
}
