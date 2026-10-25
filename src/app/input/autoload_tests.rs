//! VeraX AutoLoad assignment tests: grouping, capacity, range and the
//! carrier/passenger exclusion.

use super::autoload_orders;
use super::{PassengerFact, TransportFact, assign};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::world::Simulation;

const CELL: i32 = 256;

fn passenger(id: u64, type_key: u32, size: f64, infantry: bool, pos: [i32; 2]) -> PassengerFact {
    PassengerFact {
        id,
        type_key,
        size,
        infantry,
        pos,
    }
}

fn transport(id: u64, capacity: i32, size_limit: f64, pos: [i32; 2]) -> TransportFact {
    TransportFact {
        id,
        capacity,
        size_limit,
        cargo: crate::sim::passenger::PassengerCargo::new(capacity as u32, size_limit),
        pos,
    }
}

#[test]
fn balanced_types_fill_nearest_then_round_robin() {
    let passengers = [
        passenger(1, 1, 1.0, true, [0, 0]),
        passenger(2, 1, 1.0, true, [0, 0]),
        passenger(3, 2, 2.0, false, [0, 0]),
    ];
    let transports = [
        transport(10, 5, 2.0, [0, 0]),
        transport(11, 5, 2.0, [5 * CELL, 0]),
    ];
    assert_eq!(
        assign(&passengers, &transports, |_, _| true),
        vec![(1, 10), (3, 11), (2, 11)]
    );
}

#[test]
fn capacity_and_size_limit_refuse_extra_passengers() {
    let passengers = [
        passenger(1, 1, 1.0, true, [0, 0]),
        passenger(2, 1, 1.0, true, [0, 0]),
        passenger(3, 2, 2.0, false, [0, 0]),
        passenger(4, 3, 4.0, false, [0, 0]),
    ];
    let transports = [transport(10, 2, 3.0, [0, 0])];
    // Two Size=1 infantrymen fill Passengers=2; the Size=2 vehicle does not
    // fit the remaining total and the Size=4 vehicle exceeds SizeLimit=3.
    assert_eq!(
        assign(&passengers, &transports, |_, _| true),
        vec![(1, 10), (2, 10)]
    );
}

#[test]
fn range_boundary_and_carrier_exclusion() {
    let at_range = [30 * CELL, 0];
    let beyond_range = [31 * CELL, 0];
    let infantry = [passenger(1, 1, 1.0, true, [0, 0])];
    assert_eq!(
        assign(&infantry, &[transport(10, 5, 1.0, at_range)], |_, _| true).len(),
        1
    );
    assert!(
        assign(&infantry, &[transport(10, 5, 1.0, beyond_range)], |_, _| {
            true
        })
        .is_empty()
    );

    // Vehicle 20 is both passenger and carrier: the infantryman fills it
    // first, so it never boards another hull.
    let passengers = [
        passenger(1, 1, 1.0, true, [0, 0]),
        passenger(2, 1, 1.0, true, [0, 0]),
        passenger(20, 2, 1.0, false, [0, 0]),
    ];
    let transports = [
        transport(20, 5, 2.0, [0, 0]),
        transport(30, 5, 2.0, [CELL, 0]),
    ];
    assert_eq!(
        assign(&passengers, &transports, |_, _| true),
        vec![(1, 20), (2, 30)]
    );
}

fn scene() -> (Simulation, RuleSet) {
    // Authored Size allows the retail LCRF to board another selected hull.
    // Keep the real startup, mode and map reader passes for everything else.
    let patch = crate::rules::ini_parser::IniFile::from_str("[LCRF]\nSize=1\n");
    let rules = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "Hills.mmx",
        &patch,
    )
    .expect("retail rules")
    .rules;
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.intern("Americans");
    sim.houses_mut_for_test()
        .register(HouseState::new(owner, 0, None, true, 10_000, 10));
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    (sim, rules)
}

fn add(sim: &mut Simulation, rules: &RuleSet, id: u64, name: &str, x: u16) {
    let object = rules.object(name).unwrap();
    let owner = sim.intern("Americans");
    let type_ref = sim.intern(name);
    let category = if name == "E1" {
        EntityCategory::Infantry
    } else {
        EntityCategory::Unit
    };
    let entity = GameEntity::new_at_frame_zero_for_test(
        id,
        x,
        10,
        128,
        128,
        owner,
        Health::new(object.strength),
        type_ref,
        category,
        0,
        5,
        false,
    );
    sim.entities_mut_for_test().insert(entity);
}

fn pairs(sim: &Simulation, rules: &RuleSet, selected: &[u64]) -> Vec<(u64, u64)> {
    autoload_orders(sim, rules, selected)
        .into_iter()
        .map(|command| {
            let Command::MegaMission {
                entity_id,
                destination: Some(TargetKind::Entity(carrier)),
                ..
            } = command
            else {
                panic!("AutoLoad must emit ordinary targeted missions");
            };
            (entity_id, carrier)
        })
        .collect()
}

#[test]
fn production_assignments_keep_each_passenger_within_range() {
    let (mut sim, rules) = scene();
    add(&mut sim, &rules, 1, "E1", 10);
    add(&mut sim, &rules, 2, "E1", 10);
    add(&mut sim, &rules, 10, "LCRF", 11);
    add(&mut sim, &rules, 11, "LCRF", 50);
    assert_eq!(pairs(&sim, &rules, &[1, 2, 10, 11]), [(1, 10), (2, 10)]);
}

#[test]
fn production_admission_runs_before_reserving_capacity() {
    let (mut sim, rules) = scene();
    add(&mut sim, &rules, 1, "E1", 10);
    add(&mut sim, &rules, 2, "E1", 10);
    add(&mut sim, &rules, 10, "FV", 11);
    let enemy = sim.intern("Russians");
    sim.entities_mut_for_test()
        .get_mut(1)
        .unwrap()
        .set_owner_for_test(enemy);
    assert_eq!(pairs(&sim, &rules, &[1, 2, 10]), [(2, 10)]);

    // A nearer enemy carrier must not hide an eligible friendly one.
    add(&mut sim, &rules, 11, "FV", 10);
    sim.entities_mut_for_test()
        .get_mut(11)
        .unwrap()
        .set_owner_for_test(enemy);
    assert_eq!(pairs(&sim, &rules, &[2, 11, 10]), [(2, 10)]);
}

#[test]
fn production_carrier_already_ordered_aboard_receives_no_passengers() {
    let (mut sim, rules) = scene();
    add(&mut sim, &rules, 10, "LCRF", 10);
    add(&mut sim, &rules, 11, "LCRF", 12);
    add(&mut sim, &rules, 1, "E1", 10);
    assert!(crate::sim::passenger::can_enter_transport(
        &sim, &rules, 10, 11
    ));
    assert_eq!(pairs(&sim, &rules, &[10, 11, 1]), [(10, 11), (1, 11)]);
}
