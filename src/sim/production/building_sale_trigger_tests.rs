//! Sale callback accounting and the retained Building+53C producers.

use super::*;
use crate::map::resolved_terrain::test_flat_ground_grid;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;

fn rules() -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[Countries]\n0=C0\n1=C1\n[BuildingTypes]\n0=HOST\n\
         [VehicleTypes]\n0=TANK\n[HOST]\nStrength=1000\nCost=800\n\
         [TANK]\nStrength=300\nCost=700\n",
    ))
    .unwrap()
}

#[test]
fn building_init_managers_records_country_index_not_house_registration_order() {
    let rules = rules();
    let mut sim = Simulation::new();
    sim.install_resolved_terrain_for_new_map(test_flat_ground_grid(24));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    for (house, country) in [("H0", "C1"), ("H1", "C0")] {
        let owner = sim.intern(house);
        let country = sim.intern(country);
        sim.houses
            .register(HouseState::new(owner, 0, Some(country), true, 1000, 10));
    }
    for (house, x, expected) in [("H0", 10, 1), ("H1", 13, 0)] {
        let id = sim.spawn_object("HOST", house, x, 10, 0, &rules).unwrap();
        let entity = sim.entities().get(id).unwrap();
        assert_eq!(entity.building_loss_index(), expected);
        assert_ne!(
            sim.houses.index_of(entity.owner()).unwrap() as i32,
            expected
        );
    }
}

fn accounting_fixture(category: EntityCategory, loss_index: i32) -> (Simulation, RuleSet) {
    let rules = rules();
    let mut sim = Simulation::new();
    for name in ["VICTIM", "KILLER"] {
        let owner = sim.intern(name);
        sim.houses
            .register(HouseState::new(owner, 0, None, true, 1000, 10));
    }
    let kind = if category == EntityCategory::Structure {
        "HOST"
    } else {
        "TANK"
    };
    let mut actor = GameEntity::test_default(1, kind, "VICTIM", 10, 10);
    actor.set_owner_for_test(sim.intern("VICTIM"));
    actor.set_type_ref_for_test(sim.intern(kind));
    actor.set_category_for_test(category);
    actor.health.set_for_test(0);
    actor.initialize_building_loss_index(loss_index);
    sim.substrate.entities.insert(actor);
    (sim, rules)
}

#[test]
fn building_sale_sentinel_suppresses_only_the_victim_building_loss() {
    for (category, index, losses) in [
        (EntityCategory::Structure, -1, 0),
        (EntityCategory::Structure, 0, 1),
        (EntityCategory::Unit, -1, 1),
    ] {
        let (mut sim, rules) = accounting_fixture(category, index);
        let killer = sim.intern("KILLER");
        let victim = sim.intern("VICTIM");
        sim.record_the_kill(
            1,
            KillSource::House(Some(killer)),
            KillCallback::Terminal,
            UninitContext::with_rules(&rules),
        );
        assert_eq!(sim.houses[&victim].stats().losses(), losses);
        assert_eq!(sim.houses[&killer].stats().kills(), 1);
        assert_eq!(
            sim.houses[&killer].economy().score(),
            if category == EntityCategory::Structure {
                800
            } else {
                700
            }
        );
        assert!(sim.entities().get(1).unwrap().kill_record.is_recorded());
        Simulation::record_destruction_once(&mut sim.substrate, &mut sim.houses, 1);
        assert_eq!(
            sim.houses[&victim].stats().losses(),
            losses,
            "UnInit does not book the callback twice"
        );
    }
}

#[test]
fn nested_zero_health_uninit_honors_sale_sentinel_before_outer_kill_bookkeeping() {
    // This is the legacy fallback boundary a synchronous Tag removal can
    // reach before outer RecordKill returns, not a fabricated trigger action.
    for (index, expected) in [(-1, 0), (0, 1)] {
        let (mut sim, _rules) = accounting_fixture(EntityCategory::Structure, index);
        let owner = sim.intern("VICTIM");
        assert!(!sim.entities().get(1).unwrap().kill_record.is_recorded());
        Simulation::record_destruction_once(&mut sim.substrate, &mut sim.houses, 1);
        assert_eq!(sim.houses[&owner].stats().buildings_lost(), expected);
        assert!(sim.entities().get(1).unwrap().kill_record.is_recorded());
        Simulation::record_destruction_once(&mut sim.substrate, &mut sim.houses, 1);
        assert_eq!(sim.houses[&owner].stats().buildings_lost(), expected);
    }
}
