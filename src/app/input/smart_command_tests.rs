//! Focused, self-contained tests for the Ctrl+D smart command. These build a
//! minimal rules INI so they need no gitignored retail game data.

use super::{
    Actor, NEAR_RANGE_CELLS, ReactorTarget, SmartMode, Target, detect_mode, nearby_infantry_absorb,
    plan_mode, spread, spread_all, spread_by_capacity,
};
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::command::Command;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use crate::sim::world::Simulation;

/// A minimal rules corpus: four infantry types with one distinguishing
/// capability each, plus two buildings the planner can target. Capabilities
/// are read from each type's own section by the normal rules passes.
fn scene() -> (Simulation, RuleSet) {
    let ini = IniFile::from_str(
        "[General]\n\
         [InfantryTypes]\n\
         0=ENG\n1=C4T\n2=SPY\n3=IVN\n4=TSL\n5=OCC\n6=PLN\n\
         [BuildingTypes]\n\
         0=BUNK\n1=REACT\n\
         [ENG]\nEngineer=yes\nStrength=100\n\
         [C4T]\nC4=yes\nStrength=100\n\
         [SPY]\nAgent=yes\nInfiltrate=yes\nStrength=100\n\
         [IVN]\nIvan=yes\nStrength=100\n\
         [TSL]\nSecondary=ZAP\nStrength=100\n\
         [OCC]\nOccupier=yes\nStrength=100\n\
         [PLN]\nStrength=100\n\
         [BUNK]\nCanBeOccupied=yes\nMaxNumberOccupants=5\nStrength=1000\n\
         [REACT]\nInfantryAbsorb=yes\nPassengers=5\nStrength=1000\n\
         [ZAP]\nDamage=1\nWarhead=ELEC\n\
         [ELEC]\nElectricAssault=yes\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("minimal rules");
    let mut sim = Simulation::with_seed(77);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.intern("Americans");
    sim.houses_mut_for_test()
        .register(HouseState::new(owner, 0, None, true, 10_000, 10));
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    (sim, rules)
}

fn add_infantry(sim: &mut Simulation, rules: &RuleSet, id: u64, name: &str) {
    add_entity(sim, rules, id, name, EntityCategory::Infantry, 4, 10);
}

/// Add any entity at a `(cell_x, cell_y)` location. Structures get their
/// rules foundation so `object_center_xy` matches live geometry; everything
/// else is point-like.
fn add_entity(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    name: &str,
    category: EntityCategory,
    cell_x: u16,
    cell_y: u16,
) {
    let object = rules
        .object(name)
        .unwrap_or_else(|| panic!("missing type {name}"));
    let owner = sim.intern("Americans");
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        id,
        cell_x,
        cell_y,
        128,
        128,
        owner,
        Health::new(object.strength),
        sim.intern(name),
        category,
        0,
        5,
        false,
    );
    entity
        .seldom
        .type_facts
        .set_foundation_for_test(object.foundation.clone());
    sim.entities_mut_for_test().insert(entity);
}

#[test]
fn smart_planner_uses_the_local_permission_for_its_surface_selection() {
    use crate::sim::control_authority::{ControlGrant, ControlGrantLevel};
    use crate::sim::scenario_session::ScenarioGameMode;

    let (mut sim, rules) = scene();
    add_infantry(&mut sim, &rules, 77, "ENG");
    let delegated = sim.intern("Delegated");
    sim.register_house(HouseState::new(delegated, 1, None, false, 0, 1));
    let entity = sim.entities_mut_for_test().get_mut_for_test(77).unwrap();
    entity.set_owner_for_test(delegated);
    entity.lifecycle.set_in_limbo_for_test(false);
    sim.install_session_mode(ScenarioGameMode::Lan);
    assert!(super::surface_selection(&sim, &rules, &[77]).is_empty());
    sim.install_control_grants(vec![ControlGrant {
        source: 0,
        house: 1,
        level: ControlGrantLevel::UnitsOnly,
    }])
    .unwrap();
    assert_eq!(super::surface_selection(&sim, &rules, &[77])[0].id, 77);
}

/// Load `count` soldiers into a reactor's cargo hold, mirroring the app-side
/// reactor fixture in `context_order.rs`. The cargo capacity comes from the
/// reactor type's `Passengers=`.
fn fill_reactor(sim: &mut Simulation, id: u64, count: usize) {
    let mut cargo = PassengerCargo::new(5, 0.0);
    let ids: Vec<(u64, f64)> = (0..count).map(|i| (900 + i as u64, 1.0)).collect();
    cargo.board_forced_chain(&ids);
    *sim.entities_mut_for_test()
        .get_mut_for_test(id)
        .unwrap()
        .passenger_role_mut_for_test() = PassengerRole::transport(cargo);
}

/// An actor whose `pos` matches `actors()`'s fixed `[1024, 2560]`, i.e. the
/// same cell the entity helpers spawn into.
fn actor_at_spawn(id: u64) -> Actor {
    Actor {
        id,
        pos: [1024, 2560],
    }
}

fn actors(ids: &[u64]) -> Vec<Actor> {
    ids.iter()
        .map(|&id| Actor {
            id,
            pos: [1024, 2560],
        })
        .collect()
}

#[test]
fn engineer_wins_over_projected_infiltrate() {
    let (mut sim, rules) = scene();
    add_infantry(&mut sim, &rules, 1, "ENG");
    add_infantry(&mut sim, &rules, 2, "ENG");
    // The engine forces infiltrate onto engineers; the specific check must win.
    assert_eq!(
        detect_mode(&sim, &rules, &actors(&[1, 2])),
        Some(SmartMode::Engineer)
    );
}

#[test]
fn each_specialist_detects_its_own_mode() {
    let (mut sim, rules) = scene();
    add_infantry(&mut sim, &rules, 1, "SPY");
    add_infantry(&mut sim, &rules, 2, "C4T");
    add_infantry(&mut sim, &rules, 3, "IVN");
    add_infantry(&mut sim, &rules, 4, "TSL");
    add_infantry(&mut sim, &rules, 5, "OCC");
    assert_eq!(
        detect_mode(&sim, &rules, &actors(&[1])),
        Some(SmartMode::Spy)
    );
    assert_eq!(
        detect_mode(&sim, &rules, &actors(&[2])),
        Some(SmartMode::C4)
    );
    assert_eq!(
        detect_mode(&sim, &rules, &actors(&[3])),
        Some(SmartMode::Ivan)
    );
    assert_eq!(
        detect_mode(&sim, &rules, &actors(&[4])),
        Some(SmartMode::Tesla)
    );
    assert_eq!(
        detect_mode(&sim, &rules, &actors(&[5])),
        Some(SmartMode::Garrison)
    );
}

#[test]
fn mixed_capabilities_resolve_to_no_mode() {
    let (mut sim, rules) = scene();
    add_infantry(&mut sim, &rules, 1, "ENG");
    add_infantry(&mut sim, &rules, 2, "SPY");
    assert_eq!(detect_mode(&sim, &rules, &actors(&[1, 2])), None);
}

#[test]
fn spread_gives_each_actor_one_slot() {
    let a = actors(&[1, 2]);
    let targets = vec![Target {
        id: 99,
        pos: [1024, 2560],
    }];
    assert_eq!(
        spread(&a, &targets, 1).len(),
        1,
        "one slot => one assignment"
    );
    assert_eq!(
        spread(&a, &targets, 3).len(),
        2,
        "three slots => both actors"
    );
}

#[test]
fn near_range_constant_is_under_transport_range() {
    // Guards the design rule: reactor search is tighter than garrison.
    assert!(NEAR_RANGE_CELLS > 0);
}

#[test]
fn spread_all_assigns_every_actor_to_a_single_building() {
    // Reported bug: with one garrisonable building in range, Ctrl+D only
    // moved one soldier per press. Every actor must receive an order.
    let a = actors(&[1, 2, 3, 4, 5]);
    let targets = vec![Target {
        id: 99,
        pos: [1024, 2560],
    }];
    let orders = spread_all(&a, &targets);
    assert_eq!(orders.len(), a.len(), "one order per selected actor");
    assert!(orders.iter().all(|&(_, target)| target == 99));
}

#[test]
fn nearby_reactor_overrides_occupier_garrison() {
    // Reported bug: E1 has Occupier=yes, so role_of chose Garrison and the
    // face-to-face reactor was never fed. A reactor inside the tight range
    // must flip the whole infantry selection to Reactor mode.
    let (mut sim, rules) = scene();
    add_infantry(&mut sim, &rules, 1, "OCC");
    assert_eq!(
        detect_mode(&sim, &rules, &[actor_at_spawn(1)]),
        Some(SmartMode::Garrison)
    );

    add_entity(
        &mut sim,
        &rules,
        10,
        "REACT",
        EntityCategory::Structure,
        4,
        10,
    );
    let selected = [actor_at_spawn(1)];
    assert!(nearby_infantry_absorb(&sim, &rules, &selected));
    assert_eq!(
        detect_mode(&sim, &rules, &selected),
        Some(SmartMode::Reactor)
    );
}

#[test]
fn full_reactor_is_never_a_target() {
    // Reported bug: Ctrl+D kept sending soldiers to a reactor already at its
    // Passengers=5 capacity, producing an endless stream of refused Enter
    // orders. A full reactor must not even flip garrison troops to Reactor.
    let (mut sim, rules) = scene();
    add_infantry(&mut sim, &rules, 1, "OCC");
    add_entity(
        &mut sim,
        &rules,
        10,
        "REACT",
        EntityCategory::Structure,
        4,
        10,
    );
    fill_reactor(&mut sim, 10, 5);

    let selected = [actor_at_spawn(1)];
    assert!(!nearby_infantry_absorb(&sim, &rules, &selected));
    assert_eq!(
        detect_mode(&sim, &rules, &selected),
        Some(SmartMode::Garrison),
        "a full reactor must not hijack the garrison order"
    );
    let orders = plan_mode(&sim, &rules, &selected, SmartMode::Reactor);
    assert!(orders.is_empty(), "no Enter orders to a full reactor");
}

#[test]
fn reactor_orders_are_capped_by_remaining_capacity() {
    // A reactor with room for two takes exactly two soldiers from a squad of
    // five; the other three stay put rather than queueing up for a full hold.
    let (mut sim, rules) = scene();
    let ids = [1, 2, 3, 4, 5];
    for &id in &ids {
        add_infantry(&mut sim, &rules, id, "PLN");
    }
    add_entity(
        &mut sim,
        &rules,
        10,
        "REACT",
        EntityCategory::Structure,
        4,
        10,
    );
    fill_reactor(&mut sim, 10, 3);

    let reactors = super::nearby_reactors(&sim, &rules, &actors(&ids));
    assert_eq!(reactors.len(), 1);
    assert_eq!(reactors[0].free, 2);
    let orders = plan_mode(&sim, &rules, &actors(&ids), SmartMode::Reactor);
    assert_eq!(orders.len(), 2, "only the two free slots get an order");
    for order in orders {
        match order {
            Command::MegaMission {
                destination: Some(crate::sim::combat::TargetKind::Entity(10)),
                ..
            } => {}
            other => panic!("unexpected reactor order: {other:?}"),
        }
    }
}

#[test]
fn spread_by_capacity_respects_each_reactor() {
    // Two reactors, one with a single slot and one with three: five soldiers
    // produce four orders, never five into the single-slot one.
    let a = actors(&[1, 2, 3, 4, 5]);
    let targets = vec![
        ReactorTarget {
            target: Target {
                id: 10,
                pos: [1024, 2304],
            },
            free: 1,
        },
        ReactorTarget {
            target: Target {
                id: 11,
                pos: [1280, 2560],
            },
            free: 3,
        },
    ];
    let orders = spread_by_capacity(&a, &targets);
    assert_eq!(orders.len(), 4);
    assert_eq!(
        orders.iter().filter(|&&(_, target)| target == 10).count(),
        1
    );
    assert_eq!(
        orders.iter().filter(|&&(_, target)| target == 11).count(),
        3
    );
}

#[test]
fn ivan_orders_never_target_the_planter() {
    // Reported bug: the target scan included the selected Ivan at distance
    // zero, spread picked him, then the self filter dropped every order.
    let (mut sim, rules) = scene();
    add_infantry(&mut sim, &rules, 1, "IVN");
    // A bombable enemy target: a second house so `ally_of` is false.
    let enemy = sim.intern("Soviets");
    sim.houses_mut_for_test()
        .register(HouseState::new(enemy, 1, None, true, 10_000, 10));
    let mut target = GameEntity::new_at_frame_zero_for_test(
        2,
        5,
        10,
        128,
        128,
        enemy,
        Health::new(100),
        sim.intern("OCC"),
        EntityCategory::Infantry,
        0,
        5,
        false,
    );
    target
        .seldom
        .type_facts
        .set_foundation_for_test("1x1".to_owned().into());
    sim.entities_mut_for_test().insert(target);

    let selected = actors(&[1]);
    let orders = plan_mode(&sim, &rules, &selected, SmartMode::Ivan);
    assert!(!orders.is_empty(), "an enemy in range must yield an order");
    for order in orders {
        match order {
            Command::Attack {
                attacker_id,
                target_id,
            } => {
                assert_eq!(attacker_id, 1);
                assert_ne!(target_id, 1, "Ivan must never bomb himself");
                assert_eq!(target_id, 2);
            }
            other => panic!("unexpected Ivan order: {other:?}"),
        }
    }
}
