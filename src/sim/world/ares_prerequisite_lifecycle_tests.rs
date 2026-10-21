//! Ares3.0 alternate refresh hooks, joined to the existing lifecycle owners.
//! Saved hook controls pin only the Ares contribution. Ordinary Building
//! ChangeOwner retains its independent unconditional native recheck tail.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;
#[cfg(feature = "ares")]
use crate::sim::production::ProductionCategory;

fn fixture(category: &str, declared: bool) -> (Simulation, RuleSet, InternedId, InternedId, u64) {
    let registry = match category {
        "building" => "[VehicleTypes]\n0=OTHER\n[BuildingTypes]\n0=ALT\n",
        "unit" => "[VehicleTypes]\n0=OTHER\n1=ALT\n",
        "infantry" => "[VehicleTypes]\n0=OTHER\n[InfantryTypes]\n0=ALT\n",
        "aircraft" => "[VehicleTypes]\n0=OTHER\n[AircraftTypes]\n0=ALT\n",
        other => panic!("unsupported fixture category {other}"),
    };
    let alternate = if declared { "ALT" } else { "OTHER" };
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(&format!(
            "[Countries]\n0=C0\n[C0]\nSide=GDI\n\
             {registry}\
             [GenericPrerequisites]\ncustom=,,,\n\
             [General]\nPrerequisiteCustomAlternate={alternate}\n\
             [ALT]\nStrength=100\nTechLevel=1\nBuildLimit=100000\nOwner=C0\n",
        )),
        &IniFile::empty(),
    )
    .unwrap();
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let country = sim.interner.intern("C0");
    let owner = sim.interner.intern("H0");
    let other = sim.interner.intern("H1");
    for id in [owner, other] {
        let mut house = HouseState::new(id, 0, Some(country), true, 1000, 10);
        house.take_tech_recheck();
        sim.houses.register(house);
    }
    sim.session.set_current_house_for_test(Some(other));
    let id = sim
        .construct_object_limbo_at_height(
            "ALT",
            "H0",
            10,
            10,
            0,
            0,
            &rules,
            FrameEffects::default(),
        )
        .unwrap_or_else(|| panic!("ALT constructor for category={category}, declared={declared}"));
    assert!(
        !sim.houses[&owner].tech_recheck_pending(),
        "construction has no Ares refresh hook"
    );
    (sim, rules, owner, other, id)
}

#[test]
fn reached_alternate_events_match_executed_hook_controls() {
    let corpus = crate::test_fixture::json("tools/rules_oracle/ares_prerequisites.json");
    for row in corpus["controls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row.get("refresh_hook").is_some())
    {
        let class = row["object_category"].as_str().unwrap();
        let (mut sim, rules, owner, other, id) =
            fixture(class, row["registered_alternate"].as_bool().unwrap());
        let hook = row["refresh_hook"].as_str().unwrap();
        match hook {
            "put" => {
                // Suppress the independent discovery hook so this compares
                // the reached FootPut contribution by itself.
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .discovery
                    .set_discovered_by_other_house_for_test(true);
                assert!(matches!(
                    sim.reveal_entity_with_rules(id, &rules),
                    RevealOutcome::Revealed { .. }
                ));
            }
            "unit_dtor" | "infantry_dtor" | "aircraft_dtor" => {
                assert!(sim.discard_constructed_limbo(id, &rules, FrameEffects::default()));
            }
            "change_owner" => {
                sim.change_owner_with_rules(id, other, &rules, None, FrameEffects::default());
            }
            "discovered" => {
                // Use the common Techno body directly: Building's separate
                //44D61A tail requests its own refresh after this body returns.
                assert!(sim.record_techno_discovery(id, other, UninitContext::with_rules(&rules)));
            }
            other => panic!("unhandled native refresh hook {other}"),
        }
        let vanilla_building_capture = hook == "change_owner" && class == "building";
        for (house, field) in [(owner, "old_house_recheck"), (other, "new_house_recheck")] {
            assert_eq!(
                sim.houses[&house].tech_recheck_pending(),
                vanilla_building_capture
                    || (crate::rules::ARES_ENABLED && row[field].as_bool().unwrap()),
                "{} {field}",
                row["name"],
            );
        }
    }
}

#[test]
fn alternate_dtor_rechecks_with_another_copy_and_obeys_game_state_gate() {
    for class in ["unit", "infantry", "aircraft"] {
        let (mut sim, rules, owner, _, first) = fixture(class, true);
        let second = sim
            .construct_object_limbo_at_height(
                "ALT",
                "H0",
                12,
                10,
                0,
                0,
                &rules,
                FrameEffects::default(),
            )
            .unwrap();
        assert!(sim.discard_constructed_limbo(first, &rules, FrameEffects::default()));
        let type_id = sim.interner.get("ALT").unwrap();
        let category = sim.entities().get(second).unwrap().category();
        assert_eq!(
            sim.houses[&owner].tracking().owned_count(category, type_id),
            1
        );
        assert_eq!(
            sim.houses[&owner].tech_recheck_pending(),
            crate::rules::ARES_ENABLED
        );
        sim.houses.get_mut(&owner).unwrap().take_tech_recheck();
        sim.finalize_and_remove_common(second, UninitContext::with_rules(&rules), 0x100);
        assert!(
            !sim.houses[&owner].tech_recheck_pending(),
            "low GameState byte is zero"
        );
    }
}

#[test]
fn repeated_successful_discovery_does_not_repeat_the_hook() {
    for (current_is_owner, viewer_is_owner) in [(false, false), (true, true), (false, true)] {
        let (mut sim, rules, owner, other, id) = fixture("unit", true);
        let current = if current_is_owner { owner } else { other };
        let viewer = if viewer_is_owner { owner } else { other };
        sim.session.set_current_house_for_test(Some(current));
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .discovery
            .classify_owner(current == owner);
        assert!(sim.record_techno_discovery(id, viewer, UninitContext::with_rules(&rules)));
        assert_eq!(
            sim.houses[&owner].tech_recheck_pending(),
            crate::rules::ARES_ENABLED
        );
        assert!(
            !sim.houses[&other].tech_recheck_pending(),
            "the live owner is the receiver"
        );
        sim.houses.get_mut(&owner).unwrap().take_tech_recheck();
        assert!(!sim.record_techno_discovery(id, viewer, UninitContext::with_rules(&rules)));
        assert!(!sim.houses[&owner].tech_recheck_pending());
    }
}

#[test]
#[cfg(feature = "ares")]
fn alternate_membership_uses_actual_class_when_type_names_collide() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};

    // The first pass resolves SHARED to its only reached class, Vehicle.
    // Adding the Infantry later cannot change the retained alternate pointer
    // when that layer omits PrerequisiteCustomAlternate.
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=SHARED\n\
         [GenericPrerequisites]\ncustom=,,,\n\
         [General]\nPrerequisiteCustomAlternate=SHARED\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[InfantryTypes]\n0=SHARED\n"),
    );
    let rules = RuleSet::from_rules_layers(&layers).unwrap();
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("H0");
    let viewer = sim.interner.intern("H1");
    for id in [owner, viewer] {
        sim.houses
            .register(HouseState::new(id, 0, None, true, 0, 10));
        sim.houses.get_mut(&id).unwrap().take_tech_recheck();
    }
    sim.session.set_current_house_for_test(Some(viewer));
    let type_id = sim.interner.get("SHARED").unwrap();
    for (id, category, expected) in [
        (1, EntityCategory::Unit, true),
        (2, EntityCategory::Infantry, false),
    ] {
        let mut entity = GameEntity::test_default_of_category(id, "SHARED", "H0", 0, 0, category);
        entity.set_type_ref_for_test(type_id);
        entity.set_owner_for_test(owner);
        entity.discovery.classify_owner(false);
        sim.substrate.entities.insert(entity);
        sim.houses.get_mut(&owner).unwrap().take_tech_recheck();
        assert!(sim.record_techno_discovery(id, viewer, UninitContext::with_rules(&rules)));
        assert_eq!(
            sim.houses[&owner].tech_recheck_pending(),
            expected,
            "{category:?}"
        );
    }
}

#[test]
#[cfg(feature = "ares")]
fn captured_alternate_refreshes_cameos_then_retracts_queued_product_on_loss() {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(
            "[Countries]\n0=C0\n[C0]\nSide=GDI\n\
             [BuildingTypes]\n0=FACTORY\n\
             [VehicleTypes]\n0=ALT\n1=Q\n\
             [GenericPrerequisites]\ncustom=,,,\n\
             [General]\nPrerequisiteCustomAlternate=ALT\n\
             [FACTORY]\nStrength=1000\nFactory=UnitType\nWeaponsFactory=yes\nOwner=C0\n\
             [ALT]\nStrength=100\nTechLevel=1\nOwner=C0\n\
             [Q]\nStrength=100\nTechLevel=1\nOwner=C0\nCost=100\nPrerequisite=custom\n",
        ),
        &IniFile::empty(),
    )
    .unwrap();
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let owner = sim.interner.intern("H0");
    let other = sim.interner.intern("H1");
    let country = sim.interner.intern("C0");
    for id in [owner, other] {
        sim.houses
            .register(HouseState::new(id, 0, Some(country), true, 1000, 10));
    }
    sim.session.set_current_house_for_test(Some(owner));
    let factory = sim.spawn_object("FACTORY", "H0", 8, 8, 0, &rules).unwrap();
    sim.substrate
        .entities
        .get_mut(factory)
        .unwrap()
        .discovery
        .set_discovered_by_current_house_for_test(true);
    let alternate = sim
        .construct_object_limbo_at_height(
            "ALT",
            "H1",
            12,
            10,
            0,
            0,
            &rules,
            FrameEffects::default(),
        )
        .unwrap();
    for id in [owner, other] {
        sim.houses.get_mut(&id).unwrap().take_tech_recheck();
    }
    let query = sim.interner.get("Q").unwrap();
    assert!(
        !crate::sim::production::construction_options_for_owner(&sim, &rules, owner)
            .contains(&query)
    );
    sim.change_owner_with_rules(alternate, owner, &rules, None, FrameEffects::default());
    assert!(sim.houses[&owner].tech_recheck_pending());
    assert!(sim.recheck_house_tech_tree(owner, &rules).is_empty());
    let outputs = sim.lifecycle_outputs.take();
    assert!(outputs.iter().any(|output| matches!(
        output, LifecycleOutput::SidebarConstructionOption { owner: reached, type_id }
            if *reached == owner && *type_id == query
    )));
    assert!(!sim.houses[&owner].tech_recheck_pending());
    assert!(crate::sim::production::enqueue_by_type(
        &mut sim,
        &rules,
        "H0",
        "Q",
        FrameEffects::default(),
        false,
    ));
    assert!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Vehicle)
            .is_some()
    );
    sim.change_owner_with_rules(alternate, other, &rules, None, FrameEffects::default());
    assert!(sim.houses[&owner].tech_recheck_pending());
    let events = sim.recheck_house_tech_tree(owner, &rules);
    let cancellation_order = events
        .iter()
        .map(|event| {
            assert_eq!(event.owner, owner);
            assert_eq!(event.execute_tick, sim.session.tick() + 1);
            match event.payload {
                crate::sim::command::Command::CancelProductionByType { type_id, all } => {
                    assert_eq!(type_id, query);
                    all
                }
                ref other => panic!("unexpected prerequisite-loss output {other:?}"),
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        cancellation_order,
        [false, true],
        "active-head ABANDON precedes queued-copy cleanup"
    );
    let outputs = sim.lifecycle_outputs.take();
    assert!(!outputs.iter().any(|output| matches!(
        output, LifecycleOutput::SidebarConstructionOption { owner: reached, type_id }
            if *reached == owner && *type_id == query
    )));
    assert!(outputs.iter().any(|output| matches!(
        output, LifecycleOutput::SidebarRecalculated { owner: reached } if *reached == owner
    )));
    sim.advance_tick(&events, Some(&rules), None, None, 67);
    assert!(
        sim.production
            .factories()
            .view(owner, ProductionCategory::Vehicle)
            .is_none()
    );
}
