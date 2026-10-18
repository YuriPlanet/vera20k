//! Original Record4FB6B0 -> Counter49FA00/49FAE0 -> CanBuild4F7870 and
//! CheckBuildLimit50B370 on retained, family-local produced items. The corpus
//! supplies valid actor/type, Factory and counter priors. This replay uses
//! production readers and owners; it does not certify native paid delivery,
//! constructor RNG, allocator failure or a complete save/load.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::production::{FactoryHolder, ProductionCategory, construct_active_factory_fixture};
use crate::sim::world::FrameEffects;
use serde_json::Value;

fn category(family: &str) -> ObjectCategory {
    match family {
        "building" => ObjectCategory::Building,
        "unit" => ObjectCategory::Vehicle,
        "infantry" => ObjectCategory::Infantry,
        "aircraft" => ObjectCategory::Aircraft,
        other => panic!("unknown produced family {other}"),
    }
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn type_name(family: &str, index: i32) -> String {
    format!("HISTORY_{family}_{index}")
}

fn rules_for(case: &Value, limit: i32) -> RuleSet {
    let max_index = if case["input"]["allow_growth"].as_bool() == Some(true) {
        65
    } else {
        11
    };
    let mut ini = String::from("[Countries]\n0=C0\n[C0]\nSide=GDI\n");
    for (family, registry) in [
        ("building", "BuildingTypes"),
        ("unit", "VehicleTypes"),
        ("infantry", "InfantryTypes"),
        ("aircraft", "AircraftTypes"),
    ] {
        ini += &format!("[{registry}]\n");
        for index in 0..=max_index {
            ini += &format!("{index}={}\n", type_name(family, index));
        }
        for index in 0..=max_index {
            ini += &format!(
                "[{}]\nStrength=100\nCost=100\nOwner=C0\nTechLevel=1\nBuildLimit={limit}\nDontScore={}\n",
                type_name(family, index),
                case["input"]["dont_score"].as_bool().unwrap()
            );
        }
    }
    RuleSet::from_ini(&IniFile::from_str(&ini)).expect("declared native history inputs")
}

fn assert_state(sim: &Simulation, owner: InternedId, expected: &Value, context: &str) {
    let house = &sim.houses[&owner];
    let mut total = 0_u32;
    for family in ["building", "unit", "infantry", "aircraft"] {
        let category = category(family);
        assert_eq!(
            house.last_built_type_index(category),
            int(&expected["last_built"][family]),
            "{context}: last {family}"
        );
        for (index, count) in expected["produced_items"][family].as_object().unwrap() {
            assert_eq!(
                house.produced_count(category, index.parse().unwrap()) as u32,
                u32::try_from(count.as_u64().unwrap()).unwrap(),
                "{context}: {family} item{index}"
            );
        }
        total = total.wrapping_add(
            u32::try_from(expected["produced_totals"][family].as_u64().unwrap()).unwrap(),
        );
    }
    assert_eq!(house.stats.built(), total, "{context}: score Built");
    house.validate_production_history().unwrap();
}

#[test]
fn record_and_both_lifetime_limit_queries_match_original_retained_histories() {
    let corpus = crate::test_fixture::json("tools/can_build_produced_history.json");
    assert_eq!(corpus["schema_version"], 1);
    assert_eq!(
        corpus["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 14);
    let mut compared_phases = 0;
    for case in cases {
        let spec = &case["input"];
        let family = spec["family"].as_str().unwrap();
        let initial_index = int(&spec["type_index"]);
        let mut rules = rules_for(case, int(&spec["limit"]));
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.session.game_mode_nonzero = true;
        let country = sim.interner.intern("C0");
        let owner = sim.interner.intern("H0");
        let other = sim.interner.intern("H1");
        for owner in [owner, other] {
            sim.houses
                .insert(owner, HouseState::new(owner, 0, Some(country), true, 0, 10));
        }
        if let Some(prior) = spec["produced_prior"].as_i64() {
            sim.houses
                .get_mut(&owner)
                .unwrap()
                .set_produced_count_for_test(
                    category(family),
                    initial_index,
                    i32::try_from(prior).unwrap(),
                );
        }
        for (ordinal, phase) in case["phases"].as_array().unwrap().iter().enumerate() {
            let step = &phase["input"];
            match step["kind"].as_str().unwrap() {
                "observe" => {}
                "set_limit" => rules = rules_for(case, int(&step["limit"])),
                "record" => {
                    let selected = step["family"].as_str().unwrap_or(family);
                    let index = step["type_index"]
                        .as_i64()
                        .map_or(initial_index, |index| i32::try_from(index).unwrap());
                    let name = type_name(selected, index);
                    assert_eq!(
                        rules.type_array_index(category(selected), &name),
                        Some(index)
                    );
                    let target = if step["owner"].as_str() == Some("other") {
                        other
                    } else {
                        owner
                    };
                    let owner_name = sim.interner.resolve(target).to_owned();
                    let product = sim
                        .construct_object_limbo_at_height(
                            &name,
                            &owner_name,
                            0,
                            0,
                            0,
                            0,
                            &rules,
                            FrameEffects::default(),
                        )
                        .expect("valid typed actor for original Record prior");
                    let product_view =
                        crate::sim::production::factory_lifecycle::BuiltObject::capture(
                            &sim, product,
                        );
                    crate::sim::production::factory_lifecycle::record_last_built(
                        &mut sim,
                        &rules,
                        target,
                        product_view,
                    );
                    // Actor construction/cleanup are Rust fixture preparation,
                    // outside the original Record/limit comparison interval.
                    sim.discard_constructed_limbo(product, Some(&rules), FrameEffects::default());
                }
                "factory" => {
                    let strip = match category(family) {
                        ObjectCategory::Building => ProductionCategory::Building,
                        ObjectCategory::Vehicle => ProductionCategory::Vehicle,
                        ObjectCategory::Infantry => ProductionCategory::Infantry,
                        ObjectCategory::Aircraft => ProductionCategory::Aircraft,
                    };
                    let held = sim
                        .production
                        .factories
                        .view(owner, strip)
                        .and_then(|factory| factory.object)
                        .and_then(|object| object.entity_id);
                    if let Some(held) = held {
                        sim.discard_constructed_limbo(held, Some(&rules), FrameEffects::default());
                    }
                    sim.production
                        .factories
                        .remove_factory(FactoryHolder::House(owner, strip));
                    let queued = step["queued"].as_array().unwrap();
                    if let Some(index) = step["held_index"].as_i64() {
                        let name = type_name(family, i32::try_from(index).unwrap());
                        let type_id = sim.interner.get(&name).unwrap();
                        assert!(
                            sim.production
                                .factories
                                .test_enqueue_kernel(owner, strip, type_id, 0, 0,)
                        );
                        construct_active_factory_fixture(&mut sim, &rules, owner, strip, type_id)
                            .expect("declared held Factory prior");
                        for index in queued {
                            let name = type_name(family, int(index));
                            let type_id = sim.interner.get(&name).unwrap();
                            sim.production
                                .factories
                                .test_enqueue_kernel(owner, strip, type_id, 0, 0);
                        }
                    } else {
                        assert!(queued.is_empty(), "profile's NULL-held phase is empty");
                    }
                    let query = sim.interner.get(&type_name(family, initial_index)).unwrap();
                    sim.houses
                        .get_mut(&owner)
                        .unwrap()
                        .tracking
                        .set_owned_for_test(
                            EntityCategory::from(category(family)),
                            query,
                            int(&step["owned"]),
                        );
                }
                other => panic!("unknown history operation {other}"),
            }
            let context = format!("{} phase{ordinal}", case["name"].as_str().unwrap());
            let expected = &phase["expected"];
            assert_state(&sim, owner, expected, &context);
            if spec["second_house"].as_bool() == Some(true) {
                assert_state(&sim, other, &expected["other_house"], &context);
            }
            for query in expected["queries"].as_array().unwrap() {
                let family = query["family"].as_str().unwrap();
                let name = type_name(family, int(&query["type_index"]));
                let obj = rules.object_in_category(category(family), &name).unwrap();
                assert_eq!(obj.build_limit, int(&query["limit"]));
                let owner = if query["owner"].as_str() == Some("other") {
                    other
                } else {
                    owner
                };
                for control in query["can_build"].as_array().unwrap() {
                    let args = control["args"].as_array().unwrap();
                    let answer = can_build(
                        &sim,
                        &rules,
                        owner,
                        obj,
                        args[0].as_bool().unwrap(),
                        args[1].as_bool().unwrap(),
                    );
                    let answer = match answer {
                        CanBuild::No => 0,
                        CanBuild::Yes => 1,
                        CanBuild::AtLimit => -1,
                    };
                    assert_eq!(answer, int(&control["answer"]), "{context}: {name}");
                }
                assert_eq!(
                    check_build_limit(&sim, &rules, owner, obj),
                    query["check_build_limit"].as_bool().unwrap(),
                    "{context}: {name}"
                );
            }
            compared_phases += 1;
        }
    }
    assert_eq!(compared_phases, 45);
}
