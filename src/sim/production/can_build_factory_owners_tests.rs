//! Shared production integration checks. Native suffix goldens are replayed in
//!sim::factory_owners; these tests exercise the necessary downstream distinction.

use super::*;
use crate::rules::ini_parser::IniFile;

fn fixture(
    factory_fields: &str,
    target_fields: &str,
    tech_level: i32,
) -> (Simulation, RuleSet, InternedId) {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[Countries]\n0=SOURCE\n1=BUYER\n[BuildingTypes]\n0=YARD\n1=OTHER\n2=TARGET\n\
         [YARD]\nStrength=100\nFactory=BuildingType\nOwner=SOURCE,BUYER\n{factory_fields}\n\
         [OTHER]\nStrength=100\nFactory=BuildingType\nOwner=SOURCE,BUYER\n\
         [TARGET]\nStrength=100\nTechLevel={tech_level}\nOwner=SOURCE,BUYER\n{target_fields}\n"
    )))
    .unwrap();
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("HUMAN");
    let country = sim.interner.intern("BUYER");
    sim.houses
        .register(HouseState::new(owner, 0, Some(country), true, 0, 10));
    (sim, rules, owner)
}

fn factory(sim: &mut Simulation, rules: &RuleSet, id: u64, ty: &str) {
    crate::sim::production::tests::spawn_structure(sim, rules, id, "HUMAN", ty, id as u16 * 4, 8);
    // Explicit retained-country input, as in the native qualification controls;
    //initialization and conversion are exercised through their common owner tests.
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .initialize_factory_country(rules.country_index("SOURCE"));
}

#[test]
fn campaign_does_not_apply_the_ares_skirmish_superweapon_shell_gate() {
    let (mut sim, rules, owner) = fixture(
        "",
        "SuperWeapon=SW0\n[SuperWeaponTypes]\n0=SW0\n[SW0]\nType=MultiMissile\nDisableableFromShell=yes",
        1,
    );
    let target = rules.object("TARGET").unwrap();
    assert!(rules.super_weapon("SW0").unwrap().disableable_from_shell);
    assert!(!rules.is_build_tech("TARGET"));
    factory(&mut sim, &rules, 1, "YARD");
    sim.session.game_options_mut_for_test().super_weapons = false;
    for (nonzero_mode, expected) in [(false, CanBuild::Yes), (true, CanBuild::No)] {
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(nonzero_mode),
        );
        // Native10021A00 returns false immediately for mode0. Otherwise it
        //may refuse this Type. All other production prerequisites are supplied.
        assert_eq!(
            can_build(&sim, &rules, owner, target, false, true),
            expected
        );
    }
}

#[test]
fn negative_one_tech_level_is_human_only_in_installed_ares_qualification() {
    let data = crate::test_fixture::json("tools/rules_oracle/ares_factory_owners.json");
    for row in data["human_tech_level_rows"].as_array().unwrap() {
        let input = &row["input"];
        let level = input["tech_level"].as_i64().unwrap();
        let (mut sim, rules, owner) = fixture("", "", i32::try_from(level).unwrap());
        let target = rules.object("TARGET").unwrap();
        assert_eq!(i64::from(sim.object_tech_level(target)), level);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
                input["session_mode"].as_u64().unwrap() != 0,
            ),
        );
        let house = sim.houses.get_mut(&owner).unwrap();
        house.set_is_human_for_test(input["human"].as_bool().unwrap());
        house.set_player_control_for_test(input["player_control"].as_bool().unwrap());
        factory(&mut sim, &rules, 1, "YARD");
        let expected = if row["admitted"].as_bool().unwrap() {
            CanBuild::Yes
        } else {
            CanBuild::No
        };
        assert_eq!(
            can_build(&sim, &rules, owner, target, false, true),
            expected,
            "{}",
            input["name"]
        );
        if expected == CanBuild::Yes {
            // Passing the native prefix still needs the shared online factory.
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .set_building_online_for_test(false);
            assert_eq!(
                can_build(&sim, &rules, owner, target, false, true),
                CanBuild::AtLimit
            );
        }
    }
}

#[test]
fn ares_build_limit_matches_executed_signed_capacity_and_held_object_controls() {
    let data = crate::test_fixture::json("tools/rules_oracle/ares_factory_owners.json");
    for row in data["build_limit_rows"].as_array().unwrap() {
        let input = &row["input"];
        assert_eq!(input["family"], "infantry");
        let limit = input["limit"].as_i64().unwrap();
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Countries]\n0=COUNTRY\n[InfantryTypes]\n0=TARGET\n1=OTHER\n\
             [TARGET]\nStrength=100\nBuildLimit={limit}\n[OTHER]\nStrength=100\n"
        )))
        .unwrap();
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let owner = sim.interner.intern("HUMAN");
        let country = sim.interner.intern("COUNTRY");
        sim.houses
            .register(HouseState::new(owner, 0, Some(country), true, 0, 10));
        let target = rules.object("TARGET").unwrap();
        let target_id = sim.interner.get("TARGET").unwrap();
        if input["held"].as_bool().unwrap() {
            let held = if input["wrong_held_type"].as_bool().unwrap_or(false) {
                "OTHER"
            } else {
                "TARGET"
            };
            let type_id = sim.interner.get(held).unwrap();
            let category = crate::sim::production::ProductionCategory::Infantry;
            assert!(
                sim.production
                    .factories_mut()
                    .test_enqueue_kernel(owner, category, type_id, 0, 0)
            );
            crate::sim::production::construct_active_factory_fixture(
                &mut sim, &rules, owner, category, type_id,
            )
            .unwrap();
        }
        // The original arithmetic/control slices receive these exact counter
        //priors. Factory/production history generation is not this corpus's claim.
        let mut tracking = HouseTracking::default();
        tracking.set_owned_for_test(
            EntityCategory::Infantry,
            target_id,
            i32::try_from(input["owned"].as_i64().unwrap()).unwrap(),
        );
        let house = sim.houses.get_mut(&owner).unwrap();
        house.set_tracking_for_test(tracking);
        house.set_produced_count_for_test(
            ObjectCategory::Infantry,
            0,
            i32::try_from(input["produced"].as_i64().unwrap()).unwrap(),
        );
        let expected = match row["can_build"].as_i64().unwrap() {
            -1 => CanBuild::AtLimit,
            0 => CanBuild::No,
            1 => CanBuild::Yes,
            other => panic!("unexpected native CanBuild {other}"),
        };
        assert_eq!(
            can_build(
                &sim,
                &rules,
                owner,
                target,
                true,
                input["include_queued"].as_bool().unwrap()
            ),
            expected,
            "{}",
            input["name"]
        );
    }
}

#[test]
fn ares_owned_count_matches_executed_conversion_and_installed_slot_controls() {
    let data = crate::test_fixture::json("tools/rules_oracle/ares_factory_owners.json");
    let rows = data["owned_count_rows"].as_array().unwrap();
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=COUNTRY\n[BuildingTypes]\n0=BASE\n1=HOST_A\n2=UPGRADE\n\
         [VehicleTypes]\n0=MCV\n[BASE]\nStrength=100\nUndeploysInto=MCV\n\
         [MCV]\nStrength=100\nDeploysInto=BASE\n\
         [HOST_A]\nStrength=100\nUpgrades=3\n[UPGRADE]\nStrength=100\nPowersUpBuilding=HOST_A\n",
    ))
    .unwrap();
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("HUMAN");
    let country = sim.interner.intern("COUNTRY");
    sim.houses
        .register(HouseState::new(owner, 0, Some(country), true, 0, 10));
    for row in rows {
        let input = &row["input"];
        let mut tracking = HouseTracking::default();
        let target = match row["name"].as_str().unwrap() {
            "building_undeploy_counter_wrap" => {
                tracking.set_owned_for_test(
                    EntityCategory::Structure,
                    sim.interner.get("BASE").unwrap(),
                    i32::try_from(input["building_tracked"].as_i64().unwrap()).unwrap(),
                );
                tracking.set_owned_for_test(
                    EntityCategory::Unit,
                    sim.interner.get("MCV").unwrap(),
                    i32::try_from(input["unit_tracked"].as_i64().unwrap()).unwrap(),
                );
                "BASE"
            }
            "upgrade_counts_all_matching_host_slots" => {
                for (index, slots) in input["slots"].as_array().unwrap().iter().enumerate() {
                    let id = index as u64 + 1;
                    factory(&mut sim, &rules, id, "HOST_A");
                    let slots = std::array::from_fn(|slot| {
                        slots[slot].as_str().and_then(|name| sim.interner.get(name))
                    });
                    sim.substrate
                        .entities
                        .get_mut(id)
                        .unwrap()
                        .set_building_upgrades_for_test(0, slots);
                }
                tracking.set_owned_for_test(
                    EntityCategory::Structure,
                    sim.interner.get("UPGRADE").unwrap(),
                    i32::try_from(input["upgrade_tracked_same_index"].as_i64().unwrap()).unwrap(),
                );
                "UPGRADE"
            }
            name => panic!("uncovered native CountOwned control {name}"),
        };
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_tracking_for_test(tracking);
        assert_eq!(
            i64::from(ares_owned_build_limit_count(
                &sim,
                &rules,
                &sim.houses[&owner],
                rules.object(target).unwrap()
            )),
            row["count_owned"].as_i64().unwrap(),
            "{}",
            row["name"]
        );
    }
}

#[test]
fn passing_factory_country_is_not_final_permission_without_an_available_factory() {
    let (mut sim, rules, owner) = fixture("", "FactoryOwners=SOURCE", 1);
    let target = rules.object("TARGET").unwrap();
    let source = rules.country_index("SOURCE").unwrap();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .acquire_factory_plan(source);
    assert_eq!(
        can_build(&sim, &rules, owner, target, false, true),
        CanBuild::No
    );
    factory(&mut sim, &rules, 1, "YARD");
    assert_eq!(
        can_build(&sim, &rules, owner, target, false, true),
        CanBuild::Yes
    );
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_building_online_for_test(false);
    assert_eq!(
        can_build(&sim, &rules, owner, target, false, true),
        CanBuild::AtLimit
    );
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_building_online_for_test(true);
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .deactivated
        .set_for_test(true);
    assert_eq!(
        can_build(&sim, &rules, owner, target, false, true),
        CanBuild::AtLimit
    );
    // FindFactory's include-CanBuild flag asks only the build limit, so it
    //cannot recursively consume either source qualification or the factory scan.
    assert_eq!(
        can_build(&sim, &rules, owner, target, true, true),
        CanBuild::Yes
    );
}

#[test]
fn built_at_overrides_explicit_only_and_an_offline_tail_does_not_hide_an_online_factory() {
    let (mut sim, rules, owner) = fixture("Factory.ExplicitOnly=yes", "BuiltAt=YARD", 1);
    let target = rules.object("TARGET").unwrap();
    factory(&mut sim, &rules, 1, "YARD");
    assert_eq!(
        find_factory(&sim, &rules, owner, target, true, true, false),
        Some(1)
    );
    let (_, implicit, _) = fixture("Factory.ExplicitOnly=yes", "", 1);
    assert_eq!(
        find_factory(
            &sim,
            &implicit,
            owner,
            implicit.object("TARGET").unwrap(),
            true,
            true,
            false
        ),
        None
    );
    let (_, other_only, _) = fixture("Factory.ExplicitOnly=yes", "BuiltAt=OTHER", 1);
    assert_eq!(
        find_factory(
            &sim,
            &other_only,
            owner,
            other_only.object("TARGET").unwrap(),
            true,
            true,
            false
        ),
        None
    );
    factory(&mut sim, &rules, 2, "YARD");
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .set_building_online_for_test(false);
    assert_eq!(
        query_factory(&sim, &rules, owner, target, true, true, false, false),
        FactorySearch::Online(1)
    );
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .set_building_online_for_test(true);
    assert_eq!(
        find_factory(&sim, &rules, owner, target, true, true, false),
        Some(2)
    );
    assert_eq!(
        query_factory(&sim, &rules, owner, target, true, true, false, true),
        FactorySearch::Online(1)
    );
}
