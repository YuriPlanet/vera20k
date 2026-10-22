//! Added_To_Game502A80 -> GetPowerDrain44E880 ->5018C0, before the next
//! House508C30 assessment. Physical retail rules/art supply these types;
//! the flat fixture supplies completed actors, not native map constructors.
use super::{FrameEffects, Simulation};
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::power_system;
use crate::sim::production::{self, Factory, ProductionCategory};
use crate::sim::scenario_session::ScenarioGameMode;

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, x: u16, y: u16) -> u64 {
    sim.spawn_object(kind, owner, x, y, 0, rules)
        .expect("stock object admitted through ordinary Unlimbo")
}

fn stock_scene() -> Option<(Simulation, RuleSet, InternedId, InternedId, u64)> {
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()?;
    for (kind, power) in [("GAPOWR", 200), ("GAWEAP", -25), ("ATESLA", -75)] {
        assert_eq!(rules.object(kind).unwrap().power, power, "retail {kind}");
    }
    assert!(rules.object("ATESLA").unwrap().powered);
    let mut sim = Simulation::with_seed(31);
    sim.session.set_game_mode(ScenarioGameMode::Skirmish);
    let owner = sim.interner.intern("Americans");
    let other = sim.interner.intern("Russians");
    for (house, side) in [(owner, 0), (other, 1)] {
        sim.houses
            .register(HouseState::new(house, side, Some(house), true, 50_000, 10));
    }
    sim.session.set_current_house_for_test(Some(owner));
    spawn(&mut sim, &rules, "GACNST", "Americans", 4, 4);
    spawn(&mut sim, &rules, "GAPOWR", "Americans", 10, 4);
    spawn(&mut sim, &rules, "GAWEAP", "Americans", 14, 4);
    let tower = spawn(&mut sim, &rules, "ATESLA", "Americans", 20, 4);
    spawn(&mut sim, &rules, "ATESLA", "Americans", 22, 4);
    sim.assess_house_derived_state(owner, &rules);
    assert_eq!(sim.power_states[&owner].total_output(), 200);
    assert_eq!(sim.power_states[&owner].total_drain(), 175);
    assert!(!sim.power_states[&owner].low_power());
    Some((sim, rules, owner, other, tower))
}

fn factory(sim: &Simulation, owner: InternedId) -> &Factory {
    sim.production
        .factories()
        .iter_insertion_ordered()
        .into_iter()
        .find(|factory| {
            factory.owner() == owner && factory.category() == ProductionCategory::Vehicle
        })
        .expect("actual held vehicle Factory")
}

fn live_rate(sim: &Simulation, rules: &RuleSet, owner: InternedId) -> u16 {
    sim.production
        .factories()
        .view(owner, ProductionCategory::Vehicle)
        .unwrap()
        .build_step_rate(sim, rules)
}

#[test]
fn stock_unlimbo_publishes_drain_before_output_assessment_and_factory_rate_rewrite() {
    let Some((mut sim, rules, owner, _, tower)) = stock_scene() else {
        return;
    };
    assert!(production::enqueue_by_type(
        &mut sim,
        &rules,
        "Americans",
        "MTNK",
        FrameEffects::default(),
        false,
    ));
    let cached = factory(&sim, owner).clone();
    let full_rate = live_rate(&sim, &rules, owner);
    assert_eq!(cached.step_rate_frames(), full_rate);

    spawn(&mut sim, &rules, "ATESLA", "Americans", 24, 4);
    assert_eq!(sim.power_states[&owner].total_output(), 200);
    assert_eq!(sim.power_states[&owner].total_drain(), 250);
    assert!(sim.power_states[&owner].low_power());
    assert!(!power_system::is_building_powered(
        sim.power_states.as_map(),
        &rules,
        sim.substrate.entities.get(tower).unwrap(),
        &sim.interner,
    ));
    let low_rate = live_rate(&sim, &rules, owner);
    assert!(
        low_rate > full_rate,
        "the actual Factory query sees the new drain"
    );
    assert_eq!(
        factory(&sim, owner),
        &cached,
        "admission does not rewrite Factory state"
    );

    sim.assess_house_derived_state(owner, &rules);
    assert_eq!(
        sim.power_states[&owner].total_drain(),
        250,
        "assessment does not double count"
    );
    assert_eq!(factory(&sim, owner).step_rate_frames(), low_rate);
    assert_eq!(factory(&sim, owner).step_timer(), cached.step_timer());
    assert_eq!(factory(&sim, owner).progress(), cached.progress());

    spawn(&mut sim, &rules, "GAPOWR", "Americans", 28, 4);
    assert_eq!(
        sim.power_states[&owner].total_output(),
        200,
        "Added does not publish generation"
    );
    assert!(sim.power_states[&owner].low_power());
    sim.assess_house_derived_state(owner, &rules);
    assert_eq!(sim.power_states[&owner].total_output(), 400);
    assert_eq!(sim.power_states[&owner].total_drain(), 250);
    assert!(!sim.power_states[&owner].low_power());
    assert_eq!(factory(&sim, owner).step_rate_frames(), full_rate);
    assert_eq!(factory(&sim, owner).step_timer(), cached.step_timer());
}

#[test]
fn stock_capture_adds_new_owner_drain_while_old_owner_waits_for_assessment() {
    let Some((mut sim, rules, owner, other, tower)) = stock_scene() else {
        return;
    };
    spawn(&mut sim, &rules, "NAPOWR", "Russians", 4, 24);
    let captured = spawn(&mut sim, &rules, "ATESLA", "Russians", 12, 24);
    sim.assess_house_derived_state(other, &rules);
    assert_eq!(sim.power_states[&other].total_drain(), 75);
    let other_output = sim.power_states[&other].total_output();

    sim.change_owner_with_rules(captured, owner, &rules, None, FrameEffects::default());
    assert_eq!(sim.substrate.entities.get(captured).unwrap().owner(), owner);
    assert_eq!(sim.power_states[&owner].total_output(), 200);
    assert_eq!(sim.power_states[&owner].total_drain(), 250);
    assert_eq!(sim.power_states[&other].total_output(), other_output);
    assert_eq!(
        sim.power_states[&other].total_drain(),
        75,
        "Removed only invalidates"
    );
    assert!(!power_system::is_building_powered(
        sim.power_states.as_map(),
        &rules,
        sim.substrate.entities.get(tower).unwrap(),
        &sim.interner,
    ));

    sim.assess_house_derived_state(other, &rules);
    sim.assess_house_derived_state(owner, &rules);
    assert_eq!(sim.power_states[&other].total_drain(), 0);
    assert_eq!(sim.power_states[&owner].total_drain(), 250);
    assert_eq!(sim.power_states[&owner].total_output(), 200);
}
