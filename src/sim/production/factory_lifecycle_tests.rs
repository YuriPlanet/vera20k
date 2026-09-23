//! Production lifecycle scenarios: registry decisions must finish their held
//! object graph, accounting and successor work before returning to the caller.

use super::tests::spawn_structure;
use super::{
    ProductionCategory, cancel_by_type_for_owner, cancel_last_for_owner, enqueue_by_type,
    tick_production,
};
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::{intern::InternedId, rng::SimRng, world::Simulation};
use std::collections::BTreeMap;

pub(super) fn manager_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n1=SLAV\n\
         [VehicleTypes]\n0=MTNK\n1=SMIN\n\
         [AircraftTypes]\n0=HORN\n1=ORCA\n\
         [BuildingTypes]\n0=GAPILE\n1=GAWEAP\n2=GACNST\n3=YAREFN\n4=TECH\n5=GAAIRC\n6=GAPOWR\n\
         [E1]\nName=GI\nCost=200\nStrength=100\nArmor=flak\nSpeed=4\nSight=5\nTechLevel=1\nOwner=Americans\n\
         [SLAV]\nStrength=125\nSpeed=4\nStorage=4\n\
         [MTNK]\nName=Tank\nCost=700\nStrength=300\nArmor=heavy\nSpeed=6\nSight=6\nTechLevel=1\nOwner=Americans\nSpawns=HORN\nSpawnsNumber=3\nSpawnRegenRate=600\nSpawnReloadRate=25\n\
         [SMIN]\nCost=900\nStrength=2000\nSpeed=3\nTechLevel=1\nOwner=Americans\nPrerequisite=TECH\nEnslaves=SLAV\nSlavesNumber=2\nSlaveRegenRate=500\nSlaveReloadRate=25\n\
         [HORN]\nStrength=75\nSpeed=14\nAmmo=1\n\
         [ORCA]\nCost=1000\nStrength=200\nSpeed=8\nTechLevel=1\nOwner=Americans\n\
         [GAPILE]\nFactory=InfantryType\n\
         [GAWEAP]\nFactory=UnitType\n\
         [GACNST]\nFactory=BuildingType\nConstructionYard=yes\n\
         [YAREFN]\nCost=1000\nStrength=2000\nFoundation=1x1\nTechLevel=1\nOwner=Americans\nEnslaves=SLAV\nSlavesNumber=2\nSlaveRegenRate=500\nSlaveReloadRate=25\n\
         [GAPOWR]\nCost=800\nStrength=500\nFoundation=1x1\nTechLevel=1\nOwner=Americans\n\
         [TECH]\nStrength=500\n\
         [GAAIRC]\nFactory=AircraftType\nHelipad=yes\n",
    )).expect("factory manager fixture")
}

fn world(seed: u64) -> (Simulation, RuleSet, InternedId) {
    let rules = manager_rules();
    let mut sim = Simulation::with_seed(seed);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, true, 50_000, 10),
    );
    spawn_structure(&mut sim, 1, "Americans", "GAWEAP", 10, 10);
    spawn_structure(&mut sim, 2, "Americans", "GAPILE", 14, 10);
    spawn_structure(&mut sim, 3, "Americans", "GACNST", 18, 10);
    spawn_structure(&mut sim, 4, "Americans", "TECH", 22, 10);
    // Raw structure fixture admission bypasses lifecycle accounting.
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .tracking
        .set_buildings_for_test(4);
    (sim, rules, owner)
}

pub(super) fn held_id(sim: &Simulation, owner: InternedId, category: ProductionCategory) -> u64 {
    sim.production
        .factory_shadow
        .view(owner, category)
        .unwrap()
        .object
        .unwrap()
        .entity_id
        .unwrap()
}

pub(super) fn children(sim: &Simulation, parent: u64) -> Vec<u64> {
    let ids: Vec<_> = if let Some(manager) = sim
        .substrate
        .entities
        .get(parent)
        .unwrap()
        .spawn_manager
        .as_ref()
    {
        manager
            .slots
            .iter()
            .map(|slot| slot.spawn.unwrap())
            .collect()
    } else {
        sim.production
            .slave_bindings
            .get(&parent)
            .cloned()
            .unwrap_or_default()
    };
    for &id in &ids {
        let child = sim
            .substrate
            .entities
            .get(id)
            .expect("held manager child remains represented");
        assert!(child.lifecycle.in_limbo && !child.lifecycle.cell_marked);
        assert!(
            child.spawn_owner_id == Some(parent)
                || child
                    .slave_harvester
                    .as_ref()
                    .is_some_and(|slave| slave.master_id == parent)
        );
    }
    ids
}

fn counts(sim: &Simulation, owner: InternedId) -> (u32, u32) {
    sim.owned_object_counts(owner)
}

fn assert_gone(sim: &Simulation, parent: u64, child_ids: &[u64]) {
    assert!(!sim.substrate.entities.contains(parent));
    assert!(!sim.production.slave_bindings.contains_key(&parent));
    for &child in child_ids {
        assert!(!sim.substrate.entities.contains(child));
    }
}

fn assert_constructor_words(sim: &Simulation, parent: u64, expected: &mut SimRng) {
    for id in std::iter::once(parent).chain(children(sim, parent)) {
        let word = (expected.next_u32() & 0xffff) as u16;
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .techno_ctor_random_word,
            word
        );
    }
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
}

#[test]
fn manager_factory_cancellation_finishes_graph_accounting_and_promotion() {
    for parent_type in ["MTNK", "SMIN"] {
        let (mut sim, rules, owner) = world(0xfac7_0010);
        let before = counts(&sim, owner);
        let mut expected = sim.scenario_rng.clone();
        assert!(enqueue_by_type(&mut sim, &rules, "Americans", parent_type));
        let parent = held_id(&sim, owner, ProductionCategory::Vehicle);
        let child_ids = children(&sim, parent);
        assert_eq!(child_ids.len(), if parent_type == "MTNK" { 3 } else { 2 });
        assert_constructor_words(&sim, parent, &mut expected);
        assert_eq!(
            counts(&sim, owner),
            (before.0, before.1 + 1 + child_ids.len() as u32)
        );
        let allocated = sim.substrate.next_stable_object_id;
        assert!(enqueue_by_type(&mut sim, &rules, "Americans", parent_type));
        assert_eq!(sim.substrate.next_stable_object_id, allocated);
        assert!(cancel_last_for_owner(&mut sim, &rules, "Americans"));
        assert_eq!(held_id(&sim, owner, ProductionCategory::Vehicle), parent);
        assert_eq!(children(&sim, parent), child_ids);
        assert_eq!(sim.houses[&owner].economy.credits, 50_000);
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());

        assert!(cancel_last_for_owner(&mut sim, &rules, "Americans"));
        assert_gone(&sim, parent, &child_ids);
        assert_eq!(counts(&sim, owner), before);
        assert_eq!(sim.substrate.next_stable_object_id, allocated);
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
        assert!(
            sim.production
                .factory_shadow
                .view(owner, ProductionCategory::Vehicle)
                .is_none()
        );

        assert!(enqueue_by_type(&mut sim, &rules, "Americans", parent_type));
        let parent = held_id(&sim, owner, ProductionCategory::Vehicle);
        let child_ids = children(&sim, parent);
        assert_constructor_words(&sim, parent, &mut expected);
        let successor_type = if parent_type == "MTNK" {
            "SMIN"
        } else {
            "MTNK"
        };
        assert!(enqueue_by_type(
            &mut sim,
            &rules,
            "Americans",
            successor_type
        ));
        assert!(cancel_by_type_for_owner(
            &mut sim,
            &rules,
            "Americans",
            parent_type
        ));
        assert_gone(&sim, parent, &child_ids);
        let successor = held_id(&sim, owner, ProductionCategory::Vehicle);
        assert!(successor > parent);
        assert_constructor_words(&sim, successor, &mut expected);
        assert_eq!(
            counts(&sim, owner),
            (
                before.0,
                before.1 + 1 + children(&sim, successor).len() as u32
            )
        );
        assert_eq!(
            sim.production
                .factory_shadow
                .view(owner, ProductionCategory::Vehicle)
                .unwrap()
                .progress,
            0
        );
    }
}

#[test]
fn ready_manager_cancel_refunds_disposes_and_constructs_one_successor() {
    let (mut sim, rules, owner) = world(0xfac7_0011);
    let before = counts(&sim, owner);
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "YAREFN"));
    let parent = held_id(&sim, owner, ProductionCategory::Building);
    let child_ids = children(&sim, parent);
    assert_eq!(child_ids.len(), 2);
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(owner, ProductionCategory::Building)
    );
    assert!(!tick_production(&mut sim, &rules, &BTreeMap::new(), None));
    assert_eq!(sim.production.ready_by_owner[&owner].len(), 1);
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "YAREFN"));
    let mut expected = sim.scenario_rng.clone();
    let credits = sim.houses[&owner].economy.credits;
    assert!(cancel_by_type_for_owner(
        &mut sim,
        &rules,
        "Americans",
        "YAREFN"
    ));
    // Queue-first cancellation consumes the uncharged tail before ready fallback.
    assert_eq!(held_id(&sim, owner, ProductionCategory::Building), parent);
    assert_eq!(sim.houses[&owner].economy.credits, credits);
    // A different queued type does not intercept cancellation of the ready head.
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "GAPOWR"));
    assert!(cancel_by_type_for_owner(
        &mut sim,
        &rules,
        "Americans",
        "YAREFN"
    ));
    assert_gone(&sim, parent, &child_ids);
    assert_eq!(sim.houses[&owner].economy.credits, credits + 1000);
    assert_eq!(counts(&sim, owner), (before.0 + 1, before.1));
    assert!(
        sim.production
            .ready_by_owner
            .get(&owner)
            .is_none_or(|ready| ready.is_empty())
    );
    let successor = held_id(&sim, owner, ProductionCategory::Building);
    assert!(successor > parent);
    assert_eq!(
        sim.interner
            .resolve(sim.substrate.entities.get(successor).unwrap().type_ref),
        "GAPOWR"
    );
    assert_constructor_words(&sim, successor, &mut expected);
    assert_eq!(
        sim.production
            .factory_shadow
            .view(owner, ProductionCategory::Building)
            .unwrap()
            .progress,
        0
    );
}

#[test]
fn prerequisite_revalidation_disposes_manager_and_delays_promoted_first_charge() {
    let (mut sim, rules, owner) = world(0xfac7_0012);
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "SMIN"));
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "MTNK"));
    let parent = held_id(&sim, owner, ProductionCategory::Vehicle);
    let child_ids = children(&sim, parent);
    for _ in 0..40 {
        sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 67);
    }
    let spent = {
        let factory = sim
            .production
            .factory_shadow
            .test_factory_mut(owner, ProductionCategory::Vehicle)
            .unwrap();
        assert!(factory.progress > 0 && factory.progress < 54);
        factory.original_balance - factory.balance
    };
    let before = sim.houses[&owner].economy.credits;
    let mut expected = sim.scenario_rng.clone();
    // The producing factory remains; only the active type loses its prerequisite.
    sim.substrate.entities.remove(4);
    sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 67);
    assert_gone(&sim, parent, &child_ids);
    assert_eq!(sim.houses[&owner].economy.credits, before + spent);
    let successor = held_id(&sim, owner, ProductionCategory::Vehicle);
    assert_eq!(
        sim.interner
            .resolve(sim.substrate.entities.get(successor).unwrap().type_ref),
        "MTNK"
    );
    assert_constructor_words(&sim, successor, &mut expected);
    assert_eq!(
        sim.production
            .factory_shadow
            .view(owner, ProductionCategory::Vehicle)
            .unwrap()
            .progress,
        0
    );
    sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 67);
    assert_eq!(
        sim.production
            .factory_shadow
            .view(owner, ProductionCategory::Vehicle)
            .unwrap()
            .progress,
        1
    );
    assert!(sim.houses[&owner].economy.credits < before + spent);
}

#[test]
fn terminal_infantry_delivery_failure_refunds_and_promotes() {
    let (mut sim, rules, owner) = world(0xfac7_0013);
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "E1"));
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "E1"));
    let held = held_id(&sim, owner, ProductionCategory::Infantry);
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(owner, ProductionCategory::Infantry)
    );
    // Infantry can fall back to another owned structure when the barracks is
    // absent. Remove every producer candidate to reach terminal delivery failure.
    for structure in 1..=4 {
        sim.substrate.entities.remove(structure);
    }
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .tracking
        .set_buildings_for_test(0);
    let before = sim.houses[&owner].economy.credits;
    let mut expected = sim.scenario_rng.clone();
    assert!(!tick_production(&mut sim, &rules, &BTreeMap::new(), None));
    assert!(!sim.substrate.entities.contains(held));
    assert_eq!(sim.houses[&owner].economy.credits, before + 200);
    let successor = held_id(&sim, owner, ProductionCategory::Infantry);
    assert!(successor > held);
    assert_constructor_words(&sim, successor, &mut expected);
    assert_eq!(
        sim.production
            .factory_shadow
            .view(owner, ProductionCategory::Infantry)
            .unwrap()
            .progress,
        0
    );
}

#[test]
fn active_cancel_without_house_does_not_create_refund_account() {
    for cancel_last in [true, false] {
        let (mut sim, rules, owner) = world(0xfac7_0014);
        assert!(enqueue_by_type(&mut sim, &rules, "Americans", "SMIN"));
        let parent = held_id(&sim, owner, ProductionCategory::Vehicle);
        let child_ids = children(&sim, parent);
        sim.houses.remove(&owner);
        let rng = sim.scenario_rng.logical_state();
        let cancelled = if cancel_last {
            cancel_last_for_owner(&mut sim, &rules, "Americans")
        } else {
            cancel_by_type_for_owner(&mut sim, &rules, "Americans", "SMIN")
        };
        assert!(cancelled);
        assert_gone(&sim, parent, &child_ids);
        assert!(!sim.houses.contains_key(&owner));
        assert_eq!(sim.scenario_rng.logical_state(), rng);
    }
}

#[test]
fn missing_type_ready_without_held_object_is_removed_without_refund() {
    let (mut sim, rules, owner) = world(0xfac7_0015);
    let missing = sim.interner.intern("REMOVED_TYPE");
    sim.production
        .ready_by_owner
        .entry(owner)
        .or_default()
        .push_back(missing);
    let before = sim.houses[&owner].economy.credits;
    let rng = sim.scenario_rng.logical_state();
    assert!(cancel_by_type_for_owner(
        &mut sim,
        &rules,
        "Americans",
        "REMOVED_TYPE"
    ));
    assert_eq!(sim.houses[&owner].economy.credits, before);
    assert_eq!(sim.scenario_rng.logical_state(), rng);
    assert!(
        sim.production
            .ready_by_owner
            .get(&owner)
            .is_none_or(|ready| ready.is_empty())
    );
    assert!(
        sim.production
            .factory_shadow
            .view(owner, ProductionCategory::Building)
            .is_none()
    );
}

#[test]
fn factory_loss_revalidation_disposes_parent_and_children_before_returning() {
    for parent_type in ["MTNK", "SMIN"] {
        let (mut sim, rules, owner) = world(0xfac7_0016);
        assert!(enqueue_by_type(&mut sim, &rules, "Americans", parent_type));
        assert!(enqueue_by_type(&mut sim, &rules, "Americans", parent_type));
        let parent = held_id(&sim, owner, ProductionCategory::Vehicle);
        let child_ids = children(&sim, parent);
        for _ in 0..40 {
            sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 67);
        }
        let spent = {
            let factory = sim
                .production
                .factory_shadow
                .test_factory_mut(owner, ProductionCategory::Vehicle)
                .unwrap();
            assert!(factory.progress > 0 && factory.progress < 54);
            factory.original_balance - factory.balance
        };
        sim.substrate.entities.remove(1);
        let before = sim.houses[&owner].economy.credits;
        let allocated = sim.substrate.next_stable_object_id;
        let rng = sim.scenario_rng.logical_state();
        sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 67);
        assert_gone(&sim, parent, &child_ids);
        assert_eq!(sim.houses[&owner].economy.credits, before + spent);
        assert_eq!(sim.owned_object_counts(owner).1, 0);
        assert_eq!(sim.substrate.next_stable_object_id, allocated);
        assert_eq!(sim.scenario_rng.logical_state(), rng);
        assert!(
            sim.production
                .factory_shadow
                .view(owner, ProductionCategory::Vehicle)
                .is_none()
        );
    }
}

#[test]
fn revalidation_without_house_disposes_held_graph_without_creating_account() {
    let (mut sim, rules, owner) = world(0xfac7_0017);
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "SMIN"));
    let parent = held_id(&sim, owner, ProductionCategory::Vehicle);
    let child_ids = children(&sim, parent);
    sim.substrate.entities.remove(1);
    sim.houses.remove(&owner);
    let rng = sim.scenario_rng.logical_state();
    super::revalidate_and_step_factories(&mut sim, &rules);
    assert_gone(&sim, parent, &child_ids);
    assert!(!sim.houses.contains_key(&owner));
    assert_eq!(sim.scenario_rng.logical_state(), rng);
}

#[test]
fn missing_helipad_delivery_refunds_disposes_and_promotes_aircraft() {
    let (mut sim, rules, owner) = world(0xfac7_0018);
    spawn_structure(&mut sim, 5, "Americans", "GAAIRC", 26, 10);
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "ORCA"));
    assert!(enqueue_by_type(&mut sim, &rules, "Americans", "ORCA"));
    let parent = held_id(&sim, owner, ProductionCategory::Aircraft);
    assert!(
        sim.production
            .factory_shadow
            .test_arm_ready(owner, ProductionCategory::Aircraft)
    );
    sim.substrate.entities.remove(5);
    let before = sim.houses[&owner].economy.credits;
    let mut expected = sim.scenario_rng.clone();
    assert!(!tick_production(&mut sim, &rules, &BTreeMap::new(), None));
    assert!(!sim.substrate.entities.contains(parent));
    assert_eq!(sim.houses[&owner].economy.credits, before + 1000);
    let successor = held_id(&sim, owner, ProductionCategory::Aircraft);
    assert!(successor > parent);
    assert_constructor_words(&sim, successor, &mut expected);
    assert_eq!(
        sim.production
            .factory_shadow
            .view(owner, ProductionCategory::Aircraft)
            .unwrap()
            .progress,
        0
    );
}
