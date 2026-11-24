//! VERA's own computer player: the orders it gives, and the ones it does not.
//!
//! The player is off unless a game's rules carry a `[VeraAI]` section, so the
//! first case is the stock one: a house the player does not take keeps the
//! orders it had. The other two are the player's own: it sends its army onto
//! the enemy's base unit, and it waits at home until it has the army its
//! policy asks for.

use super::*;

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::vera_ai::VeraAiPolicy;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::world::FrameEffects;

/// A foot type with a weapon, a building that is the enemy's base unit, and
/// a `[General] BaseUnit=` list that names it.
fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[General]\nBaseUnit=none,YARD\n\
         [InfantryTypes]\n0=FOOT\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=YARD\n\
         [FOOT]\nPrimary=GUN\nStrength=100\n\
         [YARD]\nStrength=1000\n",
    ))
    .unwrap()
}

/// Two computer houses, `H0` and `H1`, with the frames at zero.
fn houses(count: usize) -> (Simulation, Vec<InternedId>) {
    let mut sim = Simulation::new();
    let names: Vec<InternedId> = (0..count)
        .map(|index| sim.interner.intern(&format!("H{index}")))
        .collect();
    for (index, &name) in names.iter().enumerate() {
        sim.houses
            .register(HouseState::new(name, index as i32, None, false, 0, 10));
    }
    sim.houses.reorder_for_test(&names);
    (sim, names)
}

/// An object on the map, out of limbo, with a cell of its own.
fn spawn(
    sim: &mut Simulation,
    owner: InternedId,
    type_id: &str,
    category: EntityCategory,
    health: i32,
) -> u64 {
    let id = sim.next_stable_object_id();
    let type_ref = sim.interner.intern(type_id);
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        id,
        10,
        10,
        0,
        0,
        owner,
        Health::new(health),
        type_ref,
        category,
        0,
        5,
        false,
    );
    entity.lifecycle.set_in_limbo_for_test(false);
    entity.lifecycle.set_cell_marked_for_test(true);
    sim.substrate.entities.insert(entity);
    sim.set_next_stable_object_id_for_test(id + 1);
    id
}

/// The mission the object holds, and the target it holds.
fn orders(sim: &Simulation, id: u64) -> (Option<MissionType>, Option<TargetKind>) {
    let entity = sim.substrate.entities.get(id).unwrap();
    (
        entity.mission.current().known(),
        entity.attack_target.target(),
    )
}

/// The queued mission the object holds.
fn queued(sim: &Simulation, id: u64) -> Option<MissionType> {
    sim.substrate
        .entities
        .get(id)
        .and_then(|entity| entity.mission.queued().known())
}

/// A world with two houses, an army of `army` foot objects at `H0` and the
/// enemy's base unit at `H1`.
fn world(
    rules: &RuleSet,
    policy: VeraAiPolicy,
    army: usize,
) -> (Simulation, Vec<InternedId>, u64, Vec<u64>, RuleSet) {
    let (mut sim, names) = houses(2);
    let owner = names[0];
    let enemy = names[1];
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_base_center_for_test(Some((10, 10)));
    sim.houses
        .get_mut(&enemy)
        .unwrap()
        .set_base_center_for_test(Some((60, 60)));
    sim.session.set_binary_frame_for_test(0);
    let target = spawn(&mut sim, enemy, "YARD", EntityCategory::Structure, 1000);
    let mut units = Vec::new();
    for _ in 0..army {
        units.push(spawn(
            &mut sim,
            owner,
            "FOOT",
            EntityCategory::Infantry,
            100,
        ));
    }
    let mut rules = rules.clone();
    rules.set_vera_ai(policy);
    (sim, names, target, units, rules)
}

#[test]
fn a_house_the_player_does_not_take_keeps_the_orders_it_had() {
    let rules = rules();
    let (mut sim, names, _target, units, mut rules) = world(
        &rules,
        VeraAiPolicy {
            enabled: true,
            max_houses: 0,
            ..VeraAiPolicy::default()
        },
        2,
    );
    // The policy is off in the rules the player reads, so nothing moves.
    rules.set_vera_ai(VeraAiPolicy::default());
    update_vera_ai(&mut sim, &rules, names[0], &mut FrameEffects::default());
    for unit in &units {
        assert_eq!(orders(&sim, *unit), (None, None));
        assert_eq!(queued(&sim, *unit), None);
    }
}

#[test]
fn the_player_skips_a_limbo_building_and_attacks_the_human_base() {
    let rules = rules();
    let (mut sim, names, limbo_target, units, rules) = world(
        &rules,
        VeraAiPolicy {
            enabled: true,
            max_houses: 1,
            attack_threshold: 1,
            home_guard: 0,
            attack_waves: 8,
            opening_delay: 0,
            decision_period: 30,
            ..VeraAiPolicy::default()
        },
        2,
    );
    sim.houses
        .get_mut(&names[1])
        .unwrap()
        .set_is_human_for_test(true);
    sim.substrate
        .entities
        .get_mut(limbo_target)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(true);
    let target = spawn(&mut sim, names[1], "YARD", EntityCategory::Structure, 1000);

    update_vera_ai(&mut sim, &rules, names[0], &mut FrameEffects::default());
    for unit in &units {
        assert_eq!(queued(&sim, *unit), Some(MissionType::Attack));
        assert_eq!(
            orders(&sim, *unit).1,
            Some(TargetKind::Entity(target)),
            "the unit attacks the enemy's base unit"
        );
    }
}

#[test]
fn the_army_waits_at_home_until_it_has_the_size_the_policy_asks_for() {
    let rules = rules();
    let (mut sim, names, _target, units, rules) = world(
        &rules,
        VeraAiPolicy {
            enabled: true,
            max_houses: 1,
            attack_threshold: 8,
            home_guard: 0,
            attack_waves: 8,
            opening_delay: 0,
            decision_period: 30,
            ..VeraAiPolicy::default()
        },
        2,
    );
    update_vera_ai(&mut sim, &rules, names[0], &mut FrameEffects::default());
    for unit in &units {
        assert_eq!(queued(&sim, *unit), Some(MissionType::AreaGuard));
        assert_eq!(orders(&sim, *unit).1, None);
    }
}
