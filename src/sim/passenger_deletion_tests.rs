//! `TechnoExt::EatPassengers` through the production owner.
use super::eat_passengers;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::test_interner;
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use crate::sim::world::{FrameEffects, Simulation};

fn make_rules(rate: &str, passenger: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=TRUCK\n1=PASS\n\n\
         [TRUCK]\nStrength=500\nSpeed=6\nPassengers=4\nCost=500\n{rate}\n\n\
         [PASS]\nStrength=100\nSpeed=5\nCost=1000\n{passenger}\n"
    )))
    .expect("passenger-deletion rules")
}

/// A transport (id 1, Americans) with the given `(id, owner)` passengers in
/// cargo order; both houses start at zero credits.
fn scene(rules: &RuleSet, passengers: &[(u64, &str)]) -> Simulation {
    let mut sim = Simulation::with_seed(1);
    sim.interner = test_interner();
    let mut owners = Vec::new();
    for name in passengers
        .iter()
        .map(|(_, owner)| *owner)
        .chain(["Americans"])
    {
        let id = sim.intern(name);
        if !owners.contains(&id) {
            owners.push(id);
            sim.houses
                .register(HouseState::new(id, 0, None, false, 0, 0));
        }
    }
    let truck_type = sim.intern("TRUCK");
    let mut truck = GameEntity::test_default(1, "TRUCK", "Americans", 5, 5);
    truck.set_type_ref_for_test(truck_type);
    truck.set_owner_for_test(sim.intern("Americans"));
    truck.lifecycle.set_in_limbo_for_test(false);
    truck.passenger_role = PassengerRole::transport(PassengerCargo::new(4, 0.0));
    sim.substrate.entities.insert(truck);
    let mut ids = Vec::new();
    for &(id, owner) in passengers {
        let type_ref = sim.intern("PASS");
        let mut passenger = GameEntity::test_default(id, "PASS", owner, 5, 5);
        passenger.set_type_ref_for_test(type_ref);
        passenger.set_owner_for_test(sim.intern(owner));
        passenger.lifecycle.set_in_limbo_for_test(false);
        sim.substrate.entities.insert(passenger);
        ids.push(id);
    }
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .passenger_role
        .cargo_mut()
        .unwrap()
        .board_forced_chain(&ids.iter().map(|&id| (id, 1.0)).collect::<Vec<_>>());
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim
}

fn eat(sim: &mut Simulation, rules: &RuleSet, frame: u32) {
    sim.session_mut_for_test().set_binary_frame_for_test(frame);
    eat_passengers(sim, rules, 1, FrameEffects::empty());
}

fn timer(sim: &Simulation) -> crate::sim::timer::CdTimer {
    sim.entities().get(1).unwrap().passenger_deletion_timer()
}

/// The rate arm: the clock starts on the first visit and the last eligible
/// passenger (owner-filtered, list order) is deleted when it runs out.
#[test]
fn the_rate_timer_deletes_the_last_eligible_passenger() {
    let rules = make_rules(
        "PassengerDeletion.Rate=3\nPassengerDeletion.AllowedHouses=enemies",
        "",
    );
    // List order 2 (enemy), 3 (owner), 4 (enemy): 4 is the last eligible.
    let mut sim = scene(&rules, &[(2, "Soviet"), (3, "Americans"), (4, "Soviet")]);
    eat(&mut sim, &rules, 0);
    assert_eq!(timer(&sim).start_frame(), 0, "the visit starts the clock");
    assert_eq!(timer(&sim).duration(), 3);
    eat(&mut sim, &rules, 2);
    assert!(
        sim.entities().get(4).is_some(),
        "the passenger survives until the clock runs out"
    );
    eat(&mut sim, &rules, 3);
    let cargo = sim
        .entities()
        .get(1)
        .unwrap()
        .passenger_role
        .cargo()
        .unwrap();
    assert_eq!(cargo.passengers(), &[2, 3], "only the last eligible leaves");
    assert!(
        sim.entities()
            .get(4)
            .is_none_or(|entity| !entity.lifecycle.object_alive()),
        "the deleted passenger is uninitialized"
    );
    assert_eq!(
        timer(&sim).start_frame(),
        crate::sim::timer::PAUSED_START_FRAME,
        "the deletion stops the clock"
    );
}

/// Cost-as-rate and the Size multiplier both shape the countdown; a cap
/// clamps the cost-derived one.
#[test]
fn cost_and_size_shape_the_countdown() {
    let cost = make_rules(
        "PassengerDeletion.UseCostAsRate=yes\nPassengerDeletion.CostMultiplier=2.0",
        "Size=1.0",
    );
    let mut sim = scene(&cost, &[(2, "Soviet")]);
    eat(&mut sim, &cost, 0);
    assert_eq!(timer(&sim).duration(), 2000, "cost * multiplier");

    let capped = make_rules(
        "PassengerDeletion.UseCostAsRate=yes\nPassengerDeletion.CostMultiplier=2.0\n\
         PassengerDeletion.CostRateCap=150",
        "Size=1.0",
    );
    let mut sim = scene(&capped, &[(2, "Soviet")]);
    eat(&mut sim, &capped, 0);
    assert_eq!(timer(&sim).duration(), 150, "the cap clamps the cost rate");

    let sized = make_rules(
        "PassengerDeletion.Rate=4\nPassengerDeletion.Rate.SizeMultiply=yes",
        "Size=2.0",
    );
    let mut sim = scene(&sized, &[(2, "Soviet")]);
    eat(&mut sim, &sized, 0);
    assert_eq!(timer(&sim).duration(), 8, "Rate * int(Size + 0.5)");
}

/// Soylent pays the transport's House the deleted passenger's full refund
/// times the multiplier, but only for the allowed Houses.
#[test]
fn soylent_pays_the_transport_owner_for_allowed_houses() {
    let rules = make_rules(
        "PassengerDeletion.Rate=1\nPassengerDeletion.Soylent=yes\n\
         PassengerDeletion.SoylentMultiplier=0.5\nPassengerDeletion.SoylentAllowedHouses=enemies",
        "",
    );
    let mut sim = scene(&rules, &[(2, "Soviet")]);
    eat(&mut sim, &rules, 0);
    eat(&mut sim, &rules, 1);
    let americans = sim.intern("Americans");
    assert_eq!(
        sim.houses.get(&americans).unwrap().economy().credits(),
        500,
        "full refund 1000 * 0.5"
    );

    // The same owner is not an enemy of itself: no refund.
    let mut sim = scene(&rules, &[(2, "Americans")]);
    eat(&mut sim, &rules, 0);
    eat(&mut sim, &rules, 1);
    let americans = sim.intern("Americans");
    assert_eq!(sim.houses.get(&americans).unwrap().economy().credits(), 0);
}

/// Limbo and EMP pause the clock by moving its start forward; `UnderEMP=yes`
/// keeps it running; no eligible passenger stops it.
#[test]
fn limbo_emp_and_an_empty_selection_handle_the_clock() {
    let rules = make_rules("PassengerDeletion.Rate=10", "");
    let mut sim = scene(&rules, &[(2, "Americans")]);
    eat(&mut sim, &rules, 0);
    assert_eq!(timer(&sim).start_frame(), 0);
    sim.entities_mut_for_test()
        .get_mut(1)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(true);
    eat(&mut sim, &rules, 1);
    assert_eq!(timer(&sim).start_frame(), 1, "limbo moves the start");
    sim.entities_mut_for_test()
        .get_mut(1)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(false);
    let under_emp = make_rules(
        "PassengerDeletion.Rate=10\nPassengerDeletion.UnderEMP=yes",
        "",
    );
    let mut sim = scene(&under_emp, &[(2, "Americans")]);
    eat(&mut sim, &under_emp, 0);
    sim.entities_mut_for_test()
        .get_mut(1)
        .unwrap()
        .set_emp_remaining_for_test(30);
    eat(&mut sim, &under_emp, 1);
    assert_eq!(timer(&sim).start_frame(), 0, "UnderEMP keeps the clock");

    let none = make_rules(
        "PassengerDeletion.Rate=10\nPassengerDeletion.AllowedHouses=enemies",
        "",
    );
    let mut sim = scene(&none, &[(2, "Americans")]);
    eat(&mut sim, &none, 0);
    eat(&mut sim, &none, 1);
    assert_eq!(
        timer(&sim).start_frame(),
        crate::sim::timer::PAUSED_START_FRAME,
        "no eligible passenger stops the clock"
    );
}
