//! Retained state/lifecycle regression coverage. Synthetic rules are read through
//!the production processor. Native qualification controls are bounded separately;
//!these checks do not claim joined DLL construction/capture/save parity.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::intern::InternedId;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::FrameEffects;

fn fixture(extra: &str) -> (Simulation, RuleSet, InternedId) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[Countries]\n0=SOURCE\n1=CHILD\n2=BUYER\n\
         [SOURCE]\nParentCountry=SOURCE\n[CHILD]\nParentCountry=SOURCE\n\
         [BUYER]\nParentCountry=BUYER\n[BuildingTypes]\n0=YARD\n1=PASS\n2=UP\n3=TARGET\n\
         [VehicleTypes]\n0=MCV\n[MCV]\nStrength=100\n\
         [YARD]\nStrength=100\nFactory=BuildingType\n\
         [PASS]\nStrength=100\nFactoryOwners.HasAllPlans=yes\n\
         [UP]\nStrength=100\nFactoryOwners.Permanent=yes\n\
         [TARGET]\nStrength=100\nFactoryOwners=SOURCE\n{extra}"
    )))
    .unwrap();
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("HUMAN");
    let country = sim.interner.intern("CHILD");
    sim.houses
        .register(HouseState::new(owner, 0, Some(country), true, 0, 10));
    (sim, rules, owner)
}

fn construct(sim: &mut Simulation, rules: &RuleSet, ty: &str) -> u64 {
    sim.construct_object_limbo_at_height(ty, "HUMAN", 8, 8, 0, 0, rules, FrameEffects::default())
        .unwrap()
}

fn list(sim: &mut Simulation, owner: InternedId, ids: Vec<u64>) {
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .base_projection_mut()
        .replace_buildings_for_test(ids);
}

#[test]
fn factory_qualification_matches_executed_ares_suffix_controls() {
    let data = crate::test_fixture::json("tools/rules_oracle/ares_factory_owners.json");
    for row in data["rows"].as_array().unwrap() {
        let input = &row["input"];
        let tokens = |key: &str| {
            input[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|name| name.as_str().unwrap())
                .collect::<Vec<_>>()
                .join(",")
        };
        let factory = match input["factory_rtti"].as_u64().unwrap_or(0) {
            0 => "none",
            7 => "BuildingType",
            16 => "InfantryType",
            other => panic!("uncovered native Factory RTTI {other}"),
        };
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Countries]\n0=CountryA\n1=CountryB\n\
             [BuildingTypes]\n0=TARGET\n1=FACTORY\n\
             [TARGET]\nStrength=100\nTechLevel=1\n\
             FactoryOwners={}\nFactoryOwners.Forbidden={}\n\
             [FACTORY]\nStrength=100\nFactory={}\nFactoryOwners.HasAllPlans={}\n",
            tokens("allowed"),
            tokens("forbidden"),
            factory,
            input["has_all_plans"].as_bool().unwrap_or(false)
        )))
        .unwrap();
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("HUMAN");
        let country = sim.interner.intern("CountryA");
        sim.houses
            .register(HouseState::new(owner, 0, Some(country), true, 0, 10));
        // These are declared retained-list/current-membership priors in the
        //native slice; neither side claims to execute their producers here.
        for plan in input["plans"].as_array().unwrap() {
            let plan = rules.country_index(plan.as_str().unwrap());
            sim.houses
                .get_mut(&owner)
                .unwrap()
                .read_scenario_factory_plan(plan);
        }
        let mut ids = Vec::new();
        for country in input["factories"].as_array().unwrap() {
            let id = construct(&mut sim, &rules, "FACTORY");
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .initialize_factory_country(
                    country.as_str().and_then(|name| rules.country_index(name)),
                );
            ids.push(id);
        }
        list(&mut sim, owner, ids);
        let native_status = row["qualification_status"].as_u64().unwrap();
        assert!(matches!(native_status, 2 | 3));
        assert_eq!(
            qualifies(
                &sim,
                &rules,
                &sim.houses[&owner],
                rules.object("TARGET").unwrap()
            ),
            native_status == 3,
            "{} (qualification suffix only, not final CanBuild)",
            input["name"]
        );
    }
}

#[test]
fn factory_country_is_initialized_from_direct_parent_and_survives_owner_transfer_and_conversion() {
    let (mut sim, rules, owner) = fixture("");
    assert!(
        sim.houses[&owner].permanent_factory_plans().is_empty(),
        "ordinary House ctor does not read scenario INI"
    );
    let source = construct(&mut sim, &rules, "YARD");
    let expected = rules.country_index("SOURCE");
    assert_eq!(
        sim.entities()
            .get(source)
            .unwrap()
            .original_factory_country(),
        expected
    );
    let buyer = sim.interner.intern("BUYER-HOUSE");
    let country = sim.interner.intern("BUYER");
    sim.houses
        .register(HouseState::new(buyer, 0, Some(country), true, 0, 10));
    sim.substrate
        .entities
        .get_mut(source)
        .unwrap()
        .set_owner_for_test(buyer);
    assert_eq!(
        sim.entities()
            .get(source)
            .unwrap()
            .original_factory_country(),
        expected
    );
    let target = construct(&mut sim, &rules, "MCV");
    sim.substrate
        .entities
        .get_mut(target)
        .unwrap()
        .initialize_factory_country(rules.country_index("BUYER"));
    sim.transfer_factory_country(source, target);
    assert_eq!(
        sim.entities()
            .get(target)
            .unwrap()
            .original_factory_country(),
        expected
    );
    // A second conversion copies null as well; it must not silently reconstruct
    //the Country from the new object's current owner.
    sim.substrate
        .entities
        .get_mut(source)
        .unwrap()
        .initialize_factory_country(None);
    sim.transfer_factory_country(source, target);
    assert_eq!(
        sim.entities()
            .get(target)
            .unwrap()
            .original_factory_country(),
        None
    );
}

#[test]
fn factory_qualification_uses_one_candidate_for_both_lists_and_requires_a_matching_factory_kind() {
    let (mut sim, rules, owner) = fixture("");
    let target = rules.object("TARGET").unwrap();
    assert!(!qualifies(&sim, &rules, &sim.houses[&owner], target));
    let yard = construct(&mut sim, &rules, "YARD");
    list(&mut sim, owner, vec![yard]);
    assert!(
        qualifies(&sim, &rules, &sim.houses[&owner], target),
        "suffix does not impose limbo/power gates"
    );
    let pass = construct(&mut sim, &rules, "PASS");
    list(&mut sim, owner, vec![pass]);
    assert!(
        qualifies(&sim, &rules, &sim.houses[&owner], target),
        "HasAllPlans admits the base type without Factory"
    );
    list(&mut sim, owner, vec![]);
    assert!(!qualifies(&sim, &rules, &sim.houses[&owner], target));
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .acquire_factory_plan(rules.country_index("SOURCE").unwrap());
    assert!(qualifies(&sim, &rules, &sim.houses[&owner], target));
    let (_, forbidden, _) = fixture("FactoryOwners.Forbidden=SOURCE\n");
    assert!(!qualifies(
        &sim,
        &forbidden,
        &sim.houses[&owner],
        forbidden.object("TARGET").unwrap()
    ));
}

#[test]
fn permanent_grant_scans_all_upgrade_slots_and_persists_without_the_building() {
    let (mut sim, rules, owner) = fixture("");
    let yard = construct(&mut sim, &rules, "YARD");
    let upgrade = sim.interner.get("UP").unwrap();
    sim.substrate
        .entities
        .get_mut(yard)
        .unwrap()
        .set_building_upgrades_for_test(0, [None, None, Some(upgrade)]);
    // Exercise the real entry hook before the stock ActuallyPlaced return.
    sim.substrate
        .entities
        .get_mut(yard)
        .unwrap()
        .building_art
        .set_actually_placed_for_test(true);
    sim.grand_opening(yard, false, false, &rules, None, FrameEffects::default());
    sim.grand_opening(yard, false, false, &rules, None, FrameEffects::default());
    let source = rules.country_index("SOURCE").unwrap();
    assert_eq!(sim.houses[&owner].permanent_factory_plans(), &[source]);
    list(&mut sim, owner, vec![]);
    assert!(qualifies(
        &sim,
        &rules,
        &sim.houses[&owner],
        rules.object("TARGET").unwrap()
    ));
    // ReadINI's producer preserves duplicate ordering; grant's producer dedupes.
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .read_scenario_factory_plan(Some(source));
    assert_eq!(
        sim.houses[&owner].permanent_factory_plans(),
        &[source, source]
    );
    // Native restore deliberately resets Scenario RNG to seed zero. Isolate
    //the retained factory state from that independent load transition.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let before = sim.state_hash();
    let bytes = GameSnapshot::save_validated(&sim, 1, 2, "factory plans", 0);
    let mut restored = GameSnapshot::load_validated(&bytes, 1, 2, "").unwrap().sim;
    restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&sim));
    restored.restore_after_snapshot_load().unwrap();
    restored
        .validate_factory_country_references(&rules)
        .unwrap();
    assert_eq!(restored.state_hash(), before);
    assert_eq!(
        restored.houses[&owner].permanent_factory_plans(),
        &[source, source]
    );
    assert_eq!(
        restored
            .entities()
            .get(yard)
            .unwrap()
            .original_factory_country(),
        Some(source)
    );
    assert!(Simulation::new().houses.is_empty());
}
