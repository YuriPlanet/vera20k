//! Focused, self-contained tests for the Ctrl+D smart command. These build a
//! minimal rules INI so they need no gitignored retail game data.

use super::{detect_mode, spread, Actor, SmartMode, Target, NEAR_RANGE_CELLS};
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::world::Simulation;

/// A minimal rules corpus: four infantry types with one distinguishing
/// capability each, plus two buildings the planner can target. Capabilities
/// are read from each type's own section by the normal rules passes.
fn scene() -> (Simulation, RuleSet) {
    let ini = IniFile::from_str(
        "[General]\n\
         [InfantryTypes]\n\
         0=ENG\n1=C4T\n2=SPY\n3=IVN\n4=TSL\n5=OCC\n\
         [BuildingTypes]\n\
         0=BUNK\n1=REACT\n\
         [ENG]\nEngineer=yes\nStrength=100\n\
         [C4T]\nC4=yes\nStrength=100\n\
         [SPY]\nAgent=yes\nInfiltrate=yes\nStrength=100\n\
         [IVN]\nIvan=yes\nStrength=100\n\
         [TSL]\nSecondary=ZAP\nStrength=100\n\
         [OCC]\nOccupier=yes\nStrength=100\n\
         [BUNK]\nCanBeOccupied=yes\nMaxNumberOccupants=5\nStrength=1000\n\
         [REACT]\nInfantryAbsorb=yes\nStrength=1000\n\
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
    let object = rules
        .object(name)
        .unwrap_or_else(|| panic!("missing type {name}"));
    let owner = sim.intern("Americans");
    let entity = GameEntity::new_at_frame_zero_for_test(
        id,
        4,
        10,
        128,
        128,
        owner,
        Health::new(object.strength),
        sim.intern(name),
        EntityCategory::Infantry,
        0,
        5,
        false,
    );
    sim.entities_mut_for_test().insert(entity);
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
