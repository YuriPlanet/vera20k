//! Focused House-tail integration tests for anger decay and AI activation.

use std::collections::BTreeMap;

use super::{HouseAiActivationOrderTestEvent, Simulation, TickLane};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::{HouseDifficulty, HouseState};

fn activation_rules(iq_production: i32) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nFixtureOnly=1\n[IQ]\nProduction={iq_production}\n"
    )))
    .expect("House activation rules fixture parses")
}

fn insert_house(
    sim: &mut Simulation,
    name: &str,
    is_human: bool,
    current_iq: i32,
) -> crate::sim::intern::InternedId {
    let owner = sim.interner.intern(name);
    let mut house = HouseState::new(owner, 0, None, is_human, 0, 10);
    house.current_iq = current_iq;
    sim.houses.insert(owner, house);
    owner
}

fn advance(sim: &mut Simulation, rules: Option<&RuleSet>, lane: TickLane) {
    let result = sim
        .advance_master_frame(&[], rules, &BTreeMap::new(), None, None, 67, lane, None)
        .expect("fixture frame must complete");
    assert!(result.frame_committed);
}

#[test]
fn house_ai_activation_forward_house_order_reaches_computer_and_passive_houses() {
    let rules = activation_rules(5);
    let mut sim = Simulation::new();
    sim.session.game_mode_nonzero = true;

    let human = insert_house(&mut sim, "CurrentPlayer", true, 9);
    let missing = sim.interner.intern("MissingHouse");
    let computer = insert_house(&mut sim, "Computer1", false, 5);
    let neutral = insert_house(&mut sim, "Neutral", false, 5);
    {
        let house = sim.houses.get_mut(&neutral).unwrap();
        house.multiplay_passive = true;
        house.is_defeated = true;
        house.difficulty = HouseDifficulty::Easy;
    }
    sim.session.house_order = vec![human, missing, computer, neutral];

    advance(&mut sim, Some(&rules), TickLane::Ordinary);

    assert_eq!(
        sim.houses[&human].ai_activation,
        Default::default(),
        "CurrentPlayer is the only neighboring state that gates this fixture"
    );
    for owner in [computer, neutral] {
        let latches = sim.houses[&owner].ai_activation;
        assert!(latches.production);
        assert!(latches.autocreate_allowed);
        assert!(!latches.ai_triggers_active);
        assert!(latches.auto_base_building);
    }
}

#[test]
fn house_ai_activation_full_frame_order_is_production_then_activation_defeat_and_ai() {
    let rules = activation_rules(5);
    let mut sim = Simulation::new();
    sim.session.game_mode_nonzero = true;
    sim.session.tick = 1;
    let owner = insert_house(&mut sim, "Computer1", false, 5);
    sim.session.house_order.push(owner);
    sim.ai_players
        .push(crate::sim::ai::AiPlayerState::new(owner));

    advance(&mut sim, Some(&rules), TickLane::Ordinary);

    assert_eq!(
        sim.take_house_ai_activation_order_test_trace(),
        vec![
            HouseAiActivationOrderTestEvent::ProductionCompleted,
            HouseAiActivationOrderTestEvent::HouseAngerDecay(owner),
            HouseAiActivationOrderTestEvent::HouseActivation(owner),
            HouseAiActivationOrderTestEvent::DefeatProcessed,
            HouseAiActivationOrderTestEvent::AiGenerated,
        ],
        "moving activation across production, defeat, or actual tick_ai dispatch must fail"
    );
}

#[test]
fn house_ai_activation_empty_network_modal_runs_full_tail_but_rulesless_skips() {
    let rules = activation_rules(5);
    let mut modal = Simulation::new();
    modal.session.game_mode_nonzero = true;
    let modal_owner = insert_house(&mut modal, "Computer1", false, 5);
    modal.session.house_order.push(modal_owner);

    advance(&mut modal, Some(&rules), TickLane::NetworkModal);

    assert!(modal.houses[&modal_owner].ai_activation.production);
    assert!(modal.houses[&modal_owner].ai_activation.autocreate_allowed);
    assert!(modal.houses[&modal_owner].ai_activation.auto_base_building);

    let mut rulesless = Simulation::new();
    rulesless.session.game_mode_nonzero = true;
    let rulesless_owner = insert_house(&mut rulesless, "Computer1", false, i32::MAX);
    let rulesless_peer = insert_house(&mut rulesless, "Computer2", false, i32::MAX);
    rulesless
        .houses
        .get_mut(&rulesless_owner)
        .unwrap()
        .ai_activation
        .auto_base_building = true;
    rulesless
        .houses
        .get_mut(&rulesless_owner)
        .unwrap()
        .grudge_scores
        .insert(rulesless_peer, 2);
    rulesless
        .session
        .house_order
        .extend([rulesless_owner, rulesless_peer]);

    advance(&mut rulesless, None, TickLane::Ordinary);

    let latches = rulesless.houses[&rulesless_owner].ai_activation;
    assert!(!latches.production);
    assert!(!latches.autocreate_allowed);
    assert!(!latches.ai_triggers_active);
    assert!(latches.auto_base_building);
    assert_eq!(
        rulesless.houses[&rulesless_owner].grudge_scores[&rulesless_peer],
        1
    );
    assert_eq!(
        rulesless.take_house_ai_activation_order_test_trace(),
        vec![
            HouseAiActivationOrderTestEvent::HouseAngerDecay(rulesless_owner),
            HouseAiActivationOrderTestEvent::HouseAngerDecay(rulesless_peer),
        ],
        "rules-less House updates retain unconditional decay but skip activation"
    );
}

#[test]
fn house_anger_decay_uses_binary_frame_not_rust_tick() {
    fn fixture(tick: u64, binary_frame: u32) -> (Simulation, crate::sim::intern::InternedId) {
        let mut sim = Simulation::new();
        sim.session.tick = tick;
        sim.session.binary_frame = binary_frame;
        let owner = insert_house(&mut sim, "Computer1", false, 5);
        let peer = insert_house(&mut sim, "Computer2", false, 5);
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .tracking
            .set_buildings_for_test(1);
        sim.houses
            .get_mut(&peer)
            .unwrap()
            .tracking
            .set_buildings_for_test(1);
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .grudge_scores
            .insert(peer, 2);
        sim.session.house_order.extend([owner, peer]);
        (sim, owner)
    }

    let (mut tick_only, tick_only_owner) = fixture(100, 99);
    advance(&mut tick_only, None, TickLane::Ordinary);
    assert_eq!(
        tick_only.houses[&tick_only_owner]
            .grudge_scores
            .values()
            .copied()
            .next(),
        Some(2),
        "Rust tick 100 must not decay native frame 99"
    );

    let (mut native_frame, native_frame_owner) = fixture(99, 100);
    advance(&mut native_frame, None, TickLane::Ordinary);
    assert_eq!(
        native_frame.houses[&native_frame_owner]
            .grudge_scores
            .values()
            .copied()
            .next(),
        Some(1),
        "native binary frame 100 decays regardless of the Rust ordinal"
    );

    let (mut signed_negative, signed_negative_owner) = fixture(0, (-100_i32) as u32);
    advance(&mut signed_negative, None, TickLane::Ordinary);
    assert_eq!(
        signed_negative.houses[&signed_negative_owner]
            .grudge_scores
            .values()
            .copied()
            .next(),
        Some(1),
        "the native low dword is interpreted as signed before remainder"
    );

    let (mut wrapped_zero, wrapped_zero_owner) = fixture(0, u32::MAX);
    advance(&mut wrapped_zero, None, TickLane::Ordinary);
    assert_eq!(
        wrapped_zero.houses[&wrapped_zero_owner]
            .grudge_scores
            .values()
            .copied()
            .next(),
        Some(2),
        "signed frame -1 does not decay"
    );
    advance(&mut wrapped_zero, None, TickLane::Ordinary);
    assert_eq!(
        wrapped_zero.houses[&wrapped_zero_owner]
            .grudge_scores
            .values()
            .copied()
            .next(),
        Some(1),
        "the first update after native u32 wrap executes under frame zero"
    );
}

#[test]
fn house_update_reloads_live_order_and_interleaves_decay_activation_per_owner() {
    let rules = activation_rules(5);
    let mut sim = Simulation::new();
    sim.session.game_mode_nonzero = true;
    let first = insert_house(&mut sim, "Computer1", false, 5);
    let missing = sim.interner.intern("MissingHouse");
    let second = insert_house(&mut sim, "Computer2", false, 5);
    let appended = insert_house(&mut sim, "Computer3", false, 5);
    sim.session.house_order.extend([first, missing, second]);
    sim.append_house_order_after_for_test(first, appended);

    advance(&mut sim, Some(&rules), TickLane::Ordinary);

    assert_eq!(sim.session.house_order, [first, missing, second, appended]);
    assert_eq!(
        sim.take_house_ai_activation_order_test_trace(),
        vec![
            HouseAiActivationOrderTestEvent::ProductionCompleted,
            HouseAiActivationOrderTestEvent::HouseAngerDecay(first),
            HouseAiActivationOrderTestEvent::HouseActivation(first),
            HouseAiActivationOrderTestEvent::HouseAngerDecay(second),
            HouseAiActivationOrderTestEvent::HouseActivation(second),
            HouseAiActivationOrderTestEvent::HouseAngerDecay(appended),
            HouseAiActivationOrderTestEvent::HouseActivation(appended),
        ],
        "the live loop must skip null slots, finish each House, and visit an appended tail"
    );
}
