//! Ares refinery storage through its production owners: the refinery
//! payment (DepositTiberium 10044230 into GiveTiberium 10024290), the
//! wallet's Spend_Money drain (0x004F9790) and Available_Money (0x004F6990),
//! and the store's removal at Limbo, sale, death (10023DA0) and capture.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;
use crate::sim::world::FrameEffects;

/// Riparius worth 25; a 10-unit `UseStorage` refinery, a 10-unit cash
/// refinery and a 20-unit silo.
fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[Tiberiums]\n0=Riparius\n[Riparius]\nValue=25\n\
         [BuildingTypes]\n0=REFN\n1=SILO\n2=CASHREFN\n\
         [REFN]\nStrength=900\nRefinery=yes\nStorage=10\nRefinery.UseStorage=yes\n\
         [SILO]\nStrength=300\nStorage=20\n\
         [CASHREFN]\nStrength=900\nRefinery=yes\nStorage=10\n",
    ))
    .unwrap()
}

/// A human House holding `credits` with a storage refinery, then a silo.
fn scene(rules: &RuleSet, credits: i32) -> (Simulation, InternedId, u64, u64) {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, Some(owner), true, credits, 10));
    let refinery = sim
        .spawn_object("REFN", "Americans", 4, 4, 0, rules)
        .unwrap();
    let silo = sim
        .spawn_object("SILO", "Americans", 10, 10, 0, rules)
        .unwrap();
    (sim, owner, refinery, silo)
}

fn deposit(sim: &mut Simulation, rules: &RuleSet, refinery: u64, bales: i32) {
    crate::sim::miner::pay_refinery_owner(sim, rules, refinery, TiberiumTypeId(0), bales);
}

fn stored(sim: &Simulation, id: u64) -> i32 {
    sim.ares_ledger_store(id)
        .map_or(0, |store| store.total_amount())
}

fn house_stored(sim: &Simulation, owner: InternedId) -> i32 {
    sim.houses[&owner]
        .economy()
        .ares_storage()
        .house_store()
        .total_amount()
}

/// Raw ore into a `Refinery.UseStorage` refinery fills the House's Storage
/// buildings in order (refinery first, then the silo), scores 5 a unit and
/// pays nothing; Available_Money counts its value. Past the capacity the
/// rest is lost. A refinery without the key pays cash and no score.
// Runs again once the miner's deposit moves to the switch.
#[cfg(feature = "ares")]
#[test]
fn use_storage_refinery_fills_its_house_storage_in_order() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules();
    let (mut sim, owner, refinery, silo) = scene(&rules, 0);
    deposit(&mut sim, &rules, refinery, 15);
    assert_eq!((stored(&sim, refinery), stored(&sim, silo)), (10, 5));
    assert_eq!(house_stored(&sim, owner), 15);
    let economy = sim.houses[&owner].economy();
    assert_eq!((economy.credits(), economy.score()), (0, 75));
    assert_eq!(economy.available_money(), 15 * 25);

    deposit(&mut sim, &rules, refinery, 20);
    assert_eq!((stored(&sim, refinery), stored(&sim, silo)), (10, 20));
    assert_eq!(house_stored(&sim, owner), 30);
    assert_eq!(sim.houses[&owner].economy().credits(), 0);

    let cash = sim
        .spawn_object("CASHREFN", "Americans", 14, 4, 0, &rules)
        .unwrap();
    let score = sim.houses[&owner].economy().score();
    deposit(&mut sim, &rules, cash, 4);
    let economy = sim.houses[&owner].economy();
    assert_eq!((economy.credits(), economy.score()), (100, score));
    assert_eq!(stored(&sim, cash), 0);
}

/// Spending past the balance drains stored units one at a time, 25 each,
/// from the first building; the last unit's excess becomes the balance.
// Runs again once the miner's deposit moves to the switch.
#[cfg(feature = "ares")]
#[test]
fn spending_past_the_balance_drains_storage_and_returns_change() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules();
    let (mut sim, owner, refinery, silo) = scene(&rules, 10);
    deposit(&mut sim, &rules, refinery, 15);
    crate::sim::credit_income::spend_money(&mut sim.houses, owner, 50);
    let economy = sim.houses[&owner].economy();
    assert_eq!((economy.credits(), economy.spent_credits()), (10, 50));
    assert_eq!((stored(&sim, refinery), stored(&sim, silo)), (8, 5));
    assert_eq!(house_stored(&sim, owner), 13);
    assert_eq!(economy.available_money(), 13 * 25 + 10);
}

/// A sold silo's Limbo takes its share out of the House and the sale's
/// payout stores it in the remaining room; a destroyed refinery loses its
/// share; a captured silo carries its share to the new House.
// Runs again once the miner's deposit moves to the switch.
#[cfg(feature = "ares")]
#[test]
fn selling_destroying_or_capturing_a_storage_building_moves_its_share() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules();
    let (mut sim, owner, refinery, silo) = scene(&rules, 0);
    let spare = sim
        .spawn_object("SILO", "Americans", 14, 14, 0, &rules)
        .unwrap();
    deposit(&mut sim, &rules, refinery, 30);
    assert_eq!(
        (
            stored(&sim, refinery),
            stored(&sim, silo),
            stored(&sim, spare)
        ),
        (10, 20, 0)
    );

    // Mission_Selling's last stage: Limbo, then the store payout.
    sim.house_removed_from_game(silo, &rules, FrameEffects::default());
    assert_eq!(house_stored(&sim, owner), 10);
    sim.ares_sale_storage_payout(silo, &rules);
    assert_eq!(stored(&sim, spare), 20);
    assert_eq!(house_stored(&sim, owner), 30);

    sim.ares_destroy_refinery_storage(refinery, &rules, crate::sim::world::FrameEffects::default());
    assert_eq!(stored(&sim, refinery), 0);
    assert_eq!(house_stored(&sim, owner), 20);

    // ChangeOwner: Removed_From_Game on the old House, Added_To_Game on the
    // new one.
    let rival = sim.interner.intern("Russians");
    sim.houses
        .register(HouseState::new(rival, 1, Some(rival), false, 0, 10));
    sim.house_removed_from_game(spare, &rules, FrameEffects::default());
    sim.substrate
        .entities
        .get_mut(spare)
        .unwrap()
        .set_owner_for_test(rival);
    sim.house_added_to_game(spare, &rules);
    assert_eq!(house_stored(&sim, owner), 0);
    assert_eq!(house_stored(&sim, rival), 20);
    assert_eq!(stored(&sim, spare), 20);
}
