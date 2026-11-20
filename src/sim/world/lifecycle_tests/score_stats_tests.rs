//! Score statistics tests: kills and losses booked from the destruction
//! record, self-kills, DontScore victims and the one shared score.

use super::*;

#[test]
fn score_stats_credit_the_killer_and_charge_the_victim_once() {
    let rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::rules::ini_parser::IniFile::from_str("[VehicleTypes]\n0=TEST\n[TEST]\nCost=900\n"),
    )
    .unwrap();
    let mut sim = Simulation::new();
    let victim_owner = sim.interner.intern("Americans");
    let killer_owner = sim.interner.intern("Russians");
    sim.houses
        .register(HouseState::new(victim_owner, 0, None, true, 0, 10));
    sim.houses
        .register(HouseState::new(killer_owner, 1, None, false, 0, 10));
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    let victim = sim.substrate.entities.get_mut(1).unwrap();
    victim.health.set_for_test(0);
    sim.record_the_kill(
        1,
        crate::sim::combat::KillSource::House(Some(killer_owner)),
        crate::sim::combat::KillCallback::Terminal,
        crate::sim::world::UninitContext::new(&rules),
    );
    assert_eq!(sim.houses[&victim_owner].stats().losses(), 1);
    assert_eq!(sim.houses[&killer_owner].stats().kills(), 1);

    // A repeated uninit must not double-count: the destruction is recorded
    // exactly once.
    sim.uninit(1);
    sim.uninit(1);

    assert_eq!(sim.houses.get(&victim_owner).unwrap().stats().losses(), 1);
    assert_eq!(sim.houses.get(&victim_owner).unwrap().stats().kills(), 0);
    let killer = sim.houses.get(&killer_owner).unwrap();
    assert_eq!(killer.stats().kills(), 1);
    assert_eq!(
        killer.stats().units_killed(),
        1,
        "a unit victim must land in the unit bucket, not the building one"
    );
    assert_eq!(killer.economy().score(), 900);
}

#[test]
fn score_stats_come_from_the_kill_record_captured_at_destruction() {
    // Dying infantry linger in the logic vector before they are removed. The
    // native accounting is already visible at the killing callback.
    let rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::rules::ini_parser::IniFile::from_str("[InfantryTypes]\n0=TEST\n[TEST]\nCost=200\n"),
    )
    .unwrap();
    let mut sim = Simulation::new();
    let victim_owner = sim.interner.intern("Americans");
    let killer_owner = sim.interner.intern("Russians");
    sim.houses
        .register(HouseState::new(victim_owner, 0, None, true, 0, 10));
    sim.houses
        .register(HouseState::new(killer_owner, 1, None, false, 0, 10));
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    let victim = sim.substrate.entities.get_mut(1).unwrap();
    victim.health.set_for_test(0);
    sim.record_the_kill(
        1,
        crate::sim::combat::KillSource::House(Some(killer_owner)),
        crate::sim::combat::KillCallback::Terminal,
        crate::sim::world::UninitContext::new(&rules),
    );
    assert_eq!(sim.houses[&killer_owner].economy().score(), 200);

    sim.uninit(1);

    assert_eq!(sim.houses.get(&killer_owner).unwrap().stats().kills(), 1);
    assert_eq!(
        sim.houses.get(&killer_owner).unwrap().economy().score(),
        200
    );
}

#[test]
fn score_stats_ignore_a_removal_that_was_not_a_destruction() {
    // Selling or otherwise despawning a healthy object is not a loss.
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    insert_entity(&mut sim, 1, EntityCategory::Structure);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .health
        .set_for_test(400);

    sim.uninit(1);

    assert_eq!(sim.houses.get(&owner).unwrap().stats().losses(), 0);
}

#[test]
fn score_stats_count_a_self_inflicted_kill_but_award_no_points() {
    // Self-inflicted destruction (own death weapon, own splash) is both a loss
    // and a kill for the house: native increments the kill table regardless of
    // relation, and Record_The_Kill's award is zero for an allied victim.
    let rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::rules::ini_parser::IniFile::from_str("[BuildingTypes]\n0=TEST\n[TEST]\nCost=800\n"),
    )
    .unwrap();
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    insert_entity(&mut sim, 1, EntityCategory::Structure);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .health
        .set_for_test(0);
    sim.record_the_kill(
        1,
        crate::sim::combat::KillSource::House(Some(owner)),
        crate::sim::combat::KillCallback::Terminal,
        crate::sim::world::UninitContext::new(&rules),
    );

    sim.uninit(1);

    let house = sim.houses.get(&owner).unwrap();
    assert_eq!(house.stats().buildings_lost(), 1);
    assert_eq!(house.stats().buildings_killed(), 1);
    assert_eq!(
        house.economy().score(),
        0,
        "an allied or self-inflicted victim is worth no score"
    );
}

#[test]
fn dont_score_victims_book_no_kill_no_loss_and_no_points() {
    // Stock `DontScore=yes` covers SLAV and the three spawner missiles
    // (V3ROCKET/DMISL/CMISL). Slaves die and respawn all match and AA intercepts
    // are routine, so a victim that native ignores entirely must not show up in
    // any of the three columns.
    let mut sim = Simulation::new();
    let victim_owner = sim.interner.intern("Americans");
    let killer_owner = sim.interner.intern("Russians");
    sim.houses
        .register(HouseState::new(victim_owner, 0, None, true, 0, 10));
    sim.houses
        .register(HouseState::new(killer_owner, 1, None, false, 0, 10));
    insert_entity(&mut sim, 1, EntityCategory::Infantry);
    let victim = sim.substrate.entities.get_mut(1).unwrap();
    victim.health.set_for_test(0);
    victim
        .tracking_facts_mut_for_test()
        .set_dont_score_for_test(true);
    // Retained attribution does not bypass the victim's DontScore gate.
    victim
        .seldom
        .kill_record
        .set_killed_by_for_test(Some(killer_owner));

    sim.uninit(1);

    let victim_house = sim.houses.get(&victim_owner).unwrap();
    assert_eq!(victim_house.stats().losses(), 0, "no phantom loss");
    let killer = sim.houses.get(&killer_owner).unwrap();
    assert_eq!(killer.stats().kills(), 0, "no phantom kill");
    assert_eq!(killer.economy().score(), 0, "no phantom points");
}

#[test]
fn deposits_and_kills_feed_the_one_score() {
    // House+0x54E8 has two feeders. Drive the kills through the real award
    // helper on stock costs rather than a hand-picked total: one Rhino
    // (Cost=900) plus one veteran GI (Cost=200, doubled).
    use crate::rules::ini_parser::IniFile;
    use crate::rules::object_type::{ObjectCategory, ObjectType};
    use crate::sim::combat::veterancy::{VeterancyRank, kill_award_points};
    use crate::sim::economy::Economy;
    use crate::util::native_x87::NativeF32Bits;

    let of = |body: &str, category| {
        let ini = IniFile::from_str(&format!(
            "[T]
{body}"
        ));
        ObjectType::from_ini_section("T", ini.section("T").expect("section"), category)
    };
    let rhino = of(
        "Cost=900
",
        ObjectCategory::Vehicle,
    );
    let gi = of(
        "Cost=200
",
        ObjectCategory::Infantry,
    );

    let kill_points = kill_award_points(rhino.cost, VeterancyRank::Rookie, false)
        + kill_award_points(gi.cost, VeterancyRank::Veteran, false);
    assert_eq!(kill_points, 1_300);

    let mut economy = Economy::new(0);
    // 240 bales of ore deposited at the x5.0 score rate.
    economy.add_tiberium_credits(
        25,
        NativeF32Bits::from_bits(240_f32.to_bits()),
        NativeF32Bits::ONE,
    );
    economy.add_score(kill_points);
    assert_eq!(economy.score(), 2_500);
}
