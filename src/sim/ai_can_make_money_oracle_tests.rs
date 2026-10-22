//! Direct original CanMakeMoney controls, joined to the actual typed cost,
//! country/Owner selection and retained House counter owners.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::util::native_x87::NativeF32Bits;
use serde_json::Value;

fn bits(value: &Value) -> NativeF32Bits {
    NativeF32Bits::from_bits(u32::from_str_radix(value.as_str().unwrap(), 16).unwrap())
}

fn mask_names(value: &Value) -> String {
    let mask = bits(value).bits();
    assert!(mask == u32::MAX || mask & !3 == 0);
    let names = [(1, "PARENT"), (2, "CHILD")]
        .into_iter()
        .filter_map(|(bit, name)| (mask & bit != 0).then_some(name))
        .collect::<Vec<_>>()
        .join(",");
    if names.is_empty() {
        // A present unresolved token yields mask0, including the required
        // mask's reject-all arm. An absent RequiredHouses means no gate.
        "UNREGISTERED".into()
    } else {
        names
    }
}

fn list(input: &Value, key: &str) -> String {
    input["lists"][key]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect::<Vec<_>>()
        .join(",")
}

fn native_scene(row: &Value) -> (Simulation, RuleSet, InternedId) {
    let input = &row["input"];
    let mut text = format!(
        "[Countries]\n0=PARENT\n1=CHILD\n\
         [PARENT]\nName=ParentAlias\nParentCountry=PARENT\n\
         [CHILD]\nName=ChildAlias\nParentCountry={}\n\
         [General]\nBaseUnit={}\nHarvesterUnit={}\nSeparateAircraft=yes\n\
         [AI]\nBuildConst=SYNTHYARD\nBuildRefinery={}\nBuildWeapons={}\n",
        input["parent_country"].as_str().unwrap(),
        list(input, "base_units"),
        list(input, "harvesters"),
        list(input, "refineries"),
        list(input, "war_factories")
    );
    let country = input["country_id"].as_str().unwrap();
    // Merge the country's fields as another real INI layer below. The
    // lexical source has only one occurrence of each section.
    let country_factors = format!(
        "[{country}]\nParentCountry={}\n{}",
        input["parent_country"].as_str().unwrap(),
        [
            "CostInfantryMult",
            "CostUnitsMult",
            "CostAircraftMult",
            "CostBuildingsMult",
            "CostDefensesMult",
        ]
        .iter()
        .zip(input["country_factors"].as_array().unwrap())
        .map(|(key, value)| format!("{key}={}\n", f32::from_bits(bits(value).bits())))
        .collect::<String>()
    );
    let types = input["types"].as_array().unwrap();
    for (kind, section, extra) in [
        ("unit", "VehicleTypes", "SYNTHGATHER"),
        (
            "building",
            "BuildingTypes",
            "SYNTHYARD,SYNTHDEST,SYNTHWEAP,SYNTHPLANT",
        ),
    ] {
        text.push_str(&format!("[{section}]\n"));
        let mut entries: Vec<_> = types
            .iter()
            .filter(|object| object["category"] == kind)
            .collect();
        entries.sort_by_key(|object| object["array_index"].as_i64().unwrap());
        for (index, id) in entries
            .iter()
            .map(|object| object["id"].as_str().unwrap())
            .chain(extra.split(','))
            .enumerate()
        {
            text.push_str(&format!("{index}={id}\n"));
        }
    }
    for object in types {
        let id = object["id"].as_str().unwrap();
        text.push_str(&format!(
            "[{id}]\nStrength=1000\nCost={}\nOwner={}\nAIBasePlanningSide={}\n",
            object["cost"],
            mask_names(&object["owner_mask"]),
            object["planned_side"]
        ));
        for (field, key) in [
            ("required_mask", "RequiredHouses"),
            ("forbidden_mask", "ForbiddenHouses"),
        ] {
            if bits(&object[field]).bits() != u32::MAX {
                text.push_str(&format!("{key}={}\n", mask_names(&object[field])));
            }
        }
        if let Some(free_unit) = object["free_unit"].as_str() {
            text.push_str(&format!("FreeUnit={free_unit}\n"));
        }
        match object["build_cat"].as_i64().unwrap() {
            0 => {}
            5 => text.push_str("BuildCat=Combat\n"),
            other => panic!("unrepresented native BuildCat {other}"),
        }
        if object["category"] == "unit" {
            text.push_str("Speed=4\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n");
        }
    }
    text.push_str(
        "[SYNTHYARD]\nStrength=1000\nConstructionYard=yes\nFactory=BuildingType\n\
         [SYNTHDEST]\nStrength=1000\nResourceDestination=yes\n\
         [SYNTHWEAP]\nStrength=1000\nWeaponsFactory=yes\nFactory=UnitType\n\
         [SYNTHGATHER]\nStrength=1000\nSpeed=4\nResourceGatherer=yes\n\
         Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
         [SYNTHPLANT]\nStrength=1000\nFactoryPlant=yes\n",
    );
    for (key, value) in [
        "InfantryCostBonus",
        "UnitsCostBonus",
        "AircraftCostBonus",
        "BuildingsCostBonus",
        "DefensesCostBonus",
    ]
    .iter()
    .zip(input["plant_factors"].as_array().unwrap())
    {
        text.push_str(&format!("{key}={}\n", f32::from_bits(bits(value).bits())));
    }
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(&text);
    ini.merge(&IniFile::from_str(&country_factors));
    let rules = RuleSet::from_ini(&ini).unwrap();
    assert_eq!(
        rules.country_parent_name(country),
        input["parent_country"].as_str()
    );
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session.game_options_mut_for_test().super_weapons =
        input["super_weapons"].as_bool().unwrap();
    let owner = sim.interner.intern("AIHouse");
    let country = sim.interner.intern(country);
    let mut house = HouseState::new(
        owner,
        input["country_side"].as_i64().unwrap() as i32,
        Some(country),
        false,
        input["money"].as_i64().unwrap() as i32,
        10,
    );
    house.project_country_mults(&rules, &sim.interner);
    house.enable_scripted_production();
    sim.houses.register(house);
    let counts = &input["counts"];
    assert!((0..=1).contains(&counts["yards"].as_i64().unwrap()));
    let explicit_counters = ["gatherers", "destinations", "war_factories", "base_units"]
        .iter()
        .any(|key| !(0..=1).contains(&counts[*key].as_i64().unwrap()));
    for (key, object, cell) in [
        ("yards", "SYNTHYARD", (4, 4)),
        ("destinations", "SYNTHDEST", (8, 8)),
        ("war_factories", "SYNTHWEAP", (12, 4)),
        ("gatherers", "SYNTHGATHER", (22, 22)),
    ] {
        let count = if explicit_counters && key != "yards" {
            0
        } else {
            counts[key].as_i64().unwrap()
        };
        for offset in 0..count {
            sim.spawn_object(
                object,
                "AIHouse",
                cell.0 + offset as u16 * 3,
                cell.1,
                0,
                &rules,
            )
            .unwrap();
        }
    }
    let base = first_owner_compatible(
        &rules,
        &rules.general.base_unit_types,
        ObjectCategory::Vehicle,
        rules.country_owner_mask(sim.interner.resolve(country)),
    )
    .unwrap();
    assert_eq!(
        base.id,
        row["output"]["selected"]["base_unit"].as_str().unwrap()
    );
    let base_id = sim.interner.intern(&base.id);
    let base_count = if explicit_counters {
        0
    } else {
        counts["base_units"].as_i64().unwrap()
    };
    for offset in 0..base_count {
        sim.spawn_object(&base.id, "AIHouse", 20 + offset as u16 * 3, 20, 0, &rules)
            .unwrap();
    }
    if explicit_counters {
        assert_eq!(input["domain"], "custom_arithmetic");
        let tracking = sim.houses.get_mut(&owner).unwrap().tracking_mut();
        tracking.set_economy_for_test(
            counts["gatherers"].as_i64().unwrap() as i32,
            counts["destinations"].as_i64().unwrap() as i32,
            counts["war_factories"].as_i64().unwrap() as i32,
        );
        tracking.set_active_for_test(
            EntityCategory::Unit,
            base_id,
            counts["base_units"].as_i64().unwrap() as i32,
        );
    }
    if input["plant_factors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| bits(value) != NativeF32Bits::ONE)
    {
        sim.spawn_object("SYNTHPLANT", "AIHouse", 24, 4, 0, &rules)
            .unwrap();
    }
    let factors = sim.houses[&owner].cost_factors();
    for (actual, expected) in [
        (factors.country, &input["country_factors"]),
        (factors.factory_plant, &input["plant_factors"]),
    ] {
        assert_eq!(
            actual.to_vec(),
            expected
                .as_array()
                .unwrap()
                .iter()
                .map(bits)
                .collect::<Vec<_>>()
        );
    }
    let house = &sim.houses[&owner];
    let tracking = house.tracking();
    for (actual, key) in [
        (tracking.resource_destinations(), "destinations"),
        (tracking.resource_gatherers(), "gatherers"),
        (tracking.war_factories(), "war_factories"),
        (
            tracking.active_count(EntityCategory::Unit, base_id),
            "base_units",
        ),
    ] {
        assert_eq!(
            i64::from(actual),
            counts[key].as_i64().unwrap(),
            "{}: {key}",
            input["name"]
        );
    }
    assert_eq!(
        house.build_const_order().len() as i64,
        counts["yards"].as_i64().unwrap()
    );
    (sim, rules, owner)
}

#[test]
fn original_can_make_money_uses_actual_selected_types_costs_and_counters() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/house_can_make_money.json");
    let rows = corpus["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 82);
    for row in rows {
        let (sim, rules, owner) = native_scene(row);
        let output = &row["output"];
        let context = row["input"]["name"].as_str().unwrap();
        let types = EconomyTypes::of(&sim, &rules, &sim.houses[&owner]);
        if output["available_money"].is_number() {
            assert_eq!(
                i64::from(available_money(&sim.houses, owner)),
                output["available_money"].as_i64().unwrap(),
                "{context}: money"
            );
            assert_eq!(
                types.harvester.map(|object| object.id.as_str()),
                output["selected"]["harvester"].as_str(),
                "{context}: selected harvester"
            );
        }
        for (key, object) in [
            ("refineries", types.refinery),
            ("war_factories", types.war_factory),
        ] {
            for selected in output["selected"][key].as_array().unwrap() {
                assert_eq!(
                    object.map(|object| object.id.as_str()),
                    selected.as_str(),
                    "{context}: selected {key}"
                );
            }
        }
        for (key, object) in [
            ("harvester", types.harvester),
            ("refinery", types.refinery),
            ("war_factory", types.war_factory),
        ] {
            if let Some(price) = output["prices"][key].as_i64() {
                assert_eq!(
                    i64::from(sim.cost_of(owner, object.unwrap(), &rules)),
                    price,
                    "{context}: {key} price"
                );
            }
        }
        let before = sim.state_hash();
        assert_eq!(
            can_make_money(&sim, &rules, owner),
            output["can_make_money"].as_bool().unwrap(),
            "{context}"
        );
        assert_eq!(sim.state_hash(), before, "{context}: query changes state");
    }
}

#[test]
fn native_refinery_threshold_reaches_whole_check_build_need() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/house_can_make_money.json");
    let rows = corpus["check_build_need_rows"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    for row in rows {
        let (sim, rules, owner) = native_scene(row);
        let context = row["input"]["name"].as_str().unwrap();
        let before = sim.state_hash();
        assert_eq!(
            can_make_money(&sim, &rules, owner),
            row["output"]["can_make_money"].as_bool().unwrap(),
            "{context}: income admission"
        );
        assert_eq!(
            check_build_need(&sim, &rules, owner),
            row["output"]["check_build_need"].as_bool().unwrap(),
            "{context}: recovery admission"
        );
        assert_eq!(sim.state_hash(), before, "{context}: queries change state");
    }
}
