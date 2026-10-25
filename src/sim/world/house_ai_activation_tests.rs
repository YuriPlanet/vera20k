//! Focused House-tail integration tests for anger decay and AI activation.

use super::{Simulation, TickLane};
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;

fn insert_house(
    sim: &mut Simulation,
    name: &str,
    is_human: bool,
    current_iq: i32,
) -> crate::sim::intern::InternedId {
    let owner = sim.interner.intern(name);
    let mut house = HouseState::new(owner, 0, None, is_human, 0, 10);
    house.set_current_iq_for_test(current_iq);
    sim.houses.register(house);
    owner
}

fn advance(sim: &mut Simulation, rules: Option<&RuleSet>, lane: TickLane) {
    let result = sim
        .advance_master_frame(
            &[],
            rules.unwrap_or(crate::rules::ruleset::RuleSet::empty_for_test()),
            None,
            67,
            lane,
            false,
            crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete")
        .into_tick();
    assert!(result.frame_committed);
}

#[test]
fn house_anger_decay_uses_binary_frame_not_rust_tick() {
    fn fixture(tick: u64, binary_frame: u32) -> (Simulation, crate::sim::intern::InternedId) {
        let mut sim = Simulation::new();
        sim.session.set_tick_for_test(tick);
        sim.session.set_binary_frame_for_test(binary_frame);
        let owner = insert_house(&mut sim, "Computer1", false, 5);
        let peer = insert_house(&mut sim, "Computer2", false, 5);
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .tracking_mut()
            .set_buildings_for_test(1);
        sim.houses
            .get_mut(&peer)
            .unwrap()
            .tracking_mut()
            .set_buildings_for_test(1);
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .grudge_scores_mut_for_test()
            .insert(peer, 2);
        (sim, owner)
    }

    let (mut tick_only, tick_only_owner) = fixture(100, 99);
    advance(&mut tick_only, None, TickLane::Ordinary);
    assert_eq!(
        tick_only.houses[&tick_only_owner]
            .grudge_scores()
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
            .grudge_scores()
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
            .grudge_scores()
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
            .grudge_scores()
            .values()
            .copied()
            .next(),
        Some(2),
        "signed frame -1 does not decay"
    );
    advance(&mut wrapped_zero, None, TickLane::Ordinary);
    assert_eq!(
        wrapped_zero.houses[&wrapped_zero_owner]
            .grudge_scores()
            .values()
            .copied()
            .next(),
        Some(1),
        "the first update after native u32 wrap executes under frame zero"
    );
}
