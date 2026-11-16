//! `BuildingExt::DisplayIncomeString` (`Ext/Building/Body.cpp:12-36`): the
//! delay gate, the type/global filters and the unconditional accumulator
//! reset.
use super::display_income_string;
use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_shields::AffectedHouse;
use crate::rules::ruleset::RuleSet;
use crate::sim::flying_text::BountyText;
use crate::sim::world::{FrameEffects, Simulation};

fn rules(global_audio: &str, building: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\n[AudioVisual]\n{global_audio}\n\
         [BuildingTypes]\n0=NAREFN\n\
         [NAREFN]\nStrength=1000\nArmor=wood\n{building}\n"
    )))
    .expect("display-income rules parse")
}

fn house(sim: &mut Simulation, name: &str, human: bool) {
    let owner = sim.intern(name);
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, human, 5000, 10);
    if human {
        house.set_player_control_for_test(true);
    }
    sim.houses_mut_for_test().register(house);
}

fn fixture(global_audio: &str, building: &str, human: bool) -> (Simulation, RuleSet, u64) {
    let rules = rules(global_audio, building);
    let mut sim = Simulation::with_seed(0x006F_A301);
    house(&mut sim, "Americans", human);
    let building = sim
        .spawn_object("NAREFN", "Americans", 20, 20, 0, &rules)
        .expect("refinery spawns");
    (sim, rules, building)
}

fn run(sim: &mut Simulation, rules: &RuleSet, building: u64) -> Vec<BountyText> {
    let events = std::cell::RefCell::new(Vec::new());
    let observe = |_: &Simulation, _: &RuleSet, text: BountyText| {
        events.borrow_mut().push(text);
    };
    display_income_string(
        sim,
        building,
        rules,
        FrameEffects::empty().with_bounty_text(&observe),
    );
    events.into_inner()
}

/// The label waits for `frame % delay == 0`, carries the accumulated amount
/// and the type's houses/offset, and resets the accumulator; a frame in
/// between emits nothing and keeps the accumulation.
#[test]
fn income_prints_on_the_delay_and_resets() {
    let (mut sim, rules, building) = fixture(
        "DisplayIncome=yes\nDisplayIncome.Delay=30\n",
        "DisplayIncome=yes\nDisplayIncome.Houses=owner\nDisplayIncome.Offset=-3,2\n",
        true,
    );
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .set_phobos_accumulated_income(123);
    sim.session.set_binary_frame_for_test(30);
    let events = run(&mut sim, &rules, building);
    assert_eq!(events.len(), 1);
    let event = events[0];
    assert_eq!(event.entity_id, Some(building));
    assert_eq!(event.amount, 123);
    assert_eq!(event.display_to, Some(AffectedHouse::OWNER));
    assert_eq!(event.display_owner, None);
    assert_eq!(event.pixel_offset, [-3, 2]);
    assert_eq!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .phobos_accumulated_income(),
        0
    );

    // A non-delay frame keeps the accumulation for the next due frame.
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .set_phobos_accumulated_income(77);
    sim.session.set_binary_frame_for_test(29);
    assert!(run(&mut sim, &rules, building).is_empty());
    assert_eq!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .phobos_accumulated_income(),
        77
    );
}

/// A disabled type/global still consumes the due label: Phobos resets the
/// accumulator before the filter can refuse it.
#[test]
fn a_disabled_income_resets_without_a_label() {
    let (mut sim, rules, building) = fixture("DisplayIncome=yes\n", "DisplayIncome=no\n", true);
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .set_phobos_accumulated_income(55);
    sim.session.set_binary_frame_for_test(30);
    assert!(run(&mut sim, &rules, building).is_empty());
    assert_eq!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .phobos_accumulated_income(),
        0
    );
}

/// `DisplayIncome_AllowAI=no` keeps a computer house's label off, while a
/// human owner still gets it.
#[test]
fn ai_income_needs_allow_ai_unless_the_owner_is_human() {
    let (mut sim, rules, building) = fixture(
        "DisplayIncome=yes\nDisplayIncome.AllowAI=no\n",
        "DisplayIncome=yes\n",
        false,
    );
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .set_phobos_accumulated_income(9);
    sim.session.set_binary_frame_for_test(30);
    assert!(run(&mut sim, &rules, building).is_empty());

    let (mut sim, rules, building) = fixture(
        "DisplayIncome=yes\nDisplayIncome.AllowAI=no\n",
        "DisplayIncome=yes\n",
        true,
    );
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .set_phobos_accumulated_income(9);
    sim.session.set_binary_frame_for_test(30);
    let events = run(&mut sim, &rules, building);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].amount, 9);
    assert_eq!(
        events[0].display_to,
        Some(rules.phobos_globals.display_income_houses),
        "an unset type Houses takes the global filter"
    );
}

/// An authored `DisplayIncome.Delay=0` is forced to one, so every frame is
/// due.
#[test]
fn a_zero_delay_is_forced_to_one() {
    let (mut sim, rules, building) = fixture(
        "DisplayIncome=yes\n",
        "DisplayIncome=yes\nDisplayIncome.Delay=0\n",
        true,
    );
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .set_phobos_accumulated_income(4);
    sim.session.set_binary_frame_for_test(7);
    let events = run(&mut sim, &rules, building);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].amount, 4);
}
