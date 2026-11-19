//! Replay of `tools/can_build_oracle.json`: the original CanBuild and
//! CheckBuildLimit over synthetic houses, answered by [`can_build`] and
//! [`check_build_limit`] on the same fixture written as rules and house
//! state.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::production::ProductionCategory;
use serde_json::{Value, json};

#[path = "can_build_ares_tests.rs"]
mod ares_tests;

fn rows(section: &str) -> Vec<Value> {
    let data: Value =
        serde_json::from_str(crate::test_fixture::text("tools/can_build_oracle.json")).unwrap();
    data[section].as_array().unwrap().clone()
}

fn comparable_can_build_rows() -> impl Iterator<Item = Value> {
    rows("can_build").into_iter().filter(|row| {
        // These gamemd controls intentionally differ from Ares100082B0's
        // positive specific count and CountOwnedNow100032F0's tracked
        // alternates. The Ares corpus covers those component boundaries.
        !crate::rules::ares_switch::ares_way()
            || (row["proc_alternate"].is_null()
                && row["name"].as_str() != Some("BuildingType count negative"))
    })
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn names(value: &Value) -> Vec<&str> {
    value.as_array().map_or_else(Vec::new, |items| {
        items.iter().map(|v| v.as_str().unwrap()).collect()
    })
}

fn category(class: &str) -> EntityCategory {
    match class {
        "unit" => EntityCategory::Unit,
        "infantry" => EntityCategory::Infantry,
        "aircraft" => EntityCategory::Aircraft,
        "building" => EntityCategory::Structure,
        other => panic!("class {other}"),
    }
}

/// The row's rules: the oracle's types, the asked type `Q` of its class and
/// the `[General]` lists.
fn rules_for(row: &Value) -> RuleSet {
    let class = row["class"].as_str().unwrap();
    let spec = &row["type"];
    let list = |section: &str, names: &[String], with_query: bool| {
        let mut text = format!("[{section}]\n");
        for (index, name) in names.iter().enumerate() {
            text += &format!("{index}={name}\n");
        }
        if with_query {
            text += &format!("{}=Q\n", names.len());
        }
        text
    };
    let numbered = |prefix: &str, count: usize| -> Vec<String> {
        (0..count).map(|index| format!("{prefix}{index}")).collect()
    };
    let mut ini = String::from("[Countries]\n0=C0\n1=C1\n2=C2\n3=C3\n[General]\n");
    for (list_name, key) in [
        ("POWER", "PrerequisitePower"),
        ("FACTORY", "PrerequisiteFactory"),
        ("BARRACKS", "PrerequisiteBarracks"),
        ("RADAR", "PrerequisiteRadar"),
        ("TECH", "PrerequisiteTech"),
        ("PROC", "PrerequisiteProc"),
    ] {
        let members = names(&row["groups"][list_name]);
        if !members.is_empty() {
            ini += &format!("{key}={}\n", members.join(","));
        }
    }
    if let Some(alternate) = row["proc_alternate"].as_str() {
        ini += &format!("PrerequisiteProcAlternate={alternate}\n");
    }
    let pads = names(&row["pads"]);
    if !pads.is_empty() {
        ini += &format!("PadAircraft={}\n", pads.join(","));
    }
    let super_weapon = &spec["super_weapon"];
    if super_weapon["build_tech"].as_bool() == Some(true) {
        ini += "[AI]\nBuildTech=Q\n";
    }
    ini += "[SuperWeaponTypes]\n0=SW0\n[SW0]\nType=MultiMissile\n";
    if super_weapon["disableable"].as_bool() == Some(true) {
        ini += "DisableableFromShell=yes\n";
    }
    ini += &list("InfantryTypes", &numbered("I", 2), class == "infantry");
    ini += &list("VehicleTypes", &numbered("U", 3), class == "unit");
    ini += &list("AircraftTypes", &numbered("A", 3), class == "aircraft");
    let mut buildings = numbered("B", 8);
    for (index, lab) in row["secret_labs"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let name = format!("LAB{index}");
        buildings.push(name.clone());
        // These corpus rows supply already-owned lab pointers. Construct the
        // same prerequisites through production type readers and Unlimbo.
        ini += &format!("[{name}]\nStrength=100\nSecretLab=yes\n");
        for (key, field) in [
            ("SecretInfantry", "infantry"),
            ("SecretUnit", "unit"),
            ("SecretBuilding", "building"),
        ] {
            if let Some(value) = lab[field].as_str() {
                ini += &format!("{key}={value}\n");
            }
        }
    }
    ini += &list("BuildingTypes", &buildings, class == "building");
    ini += &format!("[Q]\nTechLevel={}\n", spec["tech"].as_i64().unwrap_or(1));
    if let Some(limit) = spec["limit"].as_i64() {
        ini += &format!("BuildLimit={limit}\n");
    }
    for (key, field) in [
        ("Prerequisite", "prerequisite"),
        ("PrerequisiteOverride", "override"),
    ] {
        let entries = names(&spec[field]);
        if !entries.is_empty() {
            ini += &format!("{key}={}\n", entries.join(","));
        }
    }
    for (key, field) in [
        ("RequiredHouses", "required"),
        ("ForbiddenHouses", "forbidden"),
    ] {
        if let Some(countries) = spec[field].as_array() {
            let countries: Vec<String> = countries
                .iter()
                .map(|country| format!("C{}", country.as_u64().unwrap()))
                .collect();
            ini += &format!("{key}={}\n", countries.join(","));
        }
    }
    let stolen = spec["stolen"].as_array();
    for (slot, key) in [
        "RequiresStolenThirdTech",
        "RequiresStolenSovietTech",
        "RequiresStolenAlliedTech",
    ]
    .into_iter()
    .enumerate()
    {
        if stolen.is_some_and(|bytes| bytes[slot].as_bool().unwrap()) {
            ini += &format!("{key}=yes\n");
        }
    }
    if !super_weapon.is_null() {
        ini += "SuperWeapon=SW0\n";
    }
    if spec["naval"].as_bool() == Some(true) {
        ini += "Naval=yes\n";
    }
    if spec["airport_bound"].as_bool() == Some(true) {
        ini += "AirportBound=yes\n";
    }
    RuleSet::from_ini(&IniFile::from_str(&ini)).expect("oracle row rules")
}

/// The row's house `H0` (and `H1` owning the "other" factories), with its
/// counters, docks and factories.
fn sim_for(row: &Value, rules: &RuleSet) -> (Simulation, InternedId) {
    let house = &row["house"];
    let flag = |key: &str, default: bool| house[key].as_bool().unwrap_or(default);
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session.set_game_mode(
        crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
            house["game_mode"].as_i64().unwrap_or(1) != 0,
        ),
    );
    sim.session.game_options_mut_for_test().super_weapons = flag("super_weapons", true);
    let owner = sim.interner.intern("H0");
    let other = sim.interner.intern("H1");
    let country = sim
        .interner
        .intern(&format!("C{}", house["country"].as_i64().unwrap_or(1)));
    let human = flag("human", true);
    let tech = house["tech"].as_i64().map_or(10, |tech| tech as i32);
    let mut state = HouseState::new(owner, 0, Some(country), human, 0, tech);
    state.set_player_control_for_test(flag("control", human));
    // The stolen-tech bytes +0x2BC, +0x2BD and +0x2BE are what a spy's
    // BuildTech infiltration of an AIBasePlanningSide 2, 1 or 0 type sets.
    for (slot, side) in [2, 1, 0].into_iter().enumerate() {
        if house["stolen"]
            .as_array()
            .is_some_and(|bytes| bytes[slot].as_bool().unwrap())
        {
            state.steal_tech(side);
        }
    }
    sim.houses.register(state);
    sim.houses
        .register(HouseState::new(other, 0, Some(country), true, 0, 10));
    for factory in row["factories"].as_array().into_iter().flatten() {
        let holder = if factory["house"].as_str() == Some("other") {
            other
        } else {
            owner
        };
        let kind = match factory["kind"].as_str().unwrap() {
            "vehicle" => ProductionCategory::Vehicle,
            "ship" => ProductionCategory::Ship,
            "infantry" => ProductionCategory::Infantry,
            "aircraft" => ProductionCategory::Aircraft,
            "building" => ProductionCategory::Building,
            other => panic!("factory kind {other}"),
        };
        let object = sim
            .interner
            .get(factory["object"].as_str().unwrap())
            .unwrap();
        assert!(
            sim.production
                .factories_mut()
                .test_enqueue_kernel(holder, kind, object, 0, 0)
        );
        // The native oracle supplies +58 independently of the House slot,
        // including Infantry/Aircraft/Building held in the Ship factory.
        // Construct that declared object without claiming StartProduction
        // admits these synthetic cross-family holders.
        let object_type = rules
            .object(sim.interner.resolve(object))
            .expect("the row's unique declared object type");
        let owner_name = sim.interner.resolve(holder).to_owned();
        let entity = sim
            .construct_resolved_object_limbo_at_height(
                object_type,
                &owner_name,
                0,
                0,
                0,
                0,
                rules,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("the supplied factory object constructs");
        assert_eq!(
            sim.production.edit_factories().link_active_entity(
                crate::sim::production::factory::FactoryHolder::House(holder, kind),
                entity,
                0, // This supplied factory prior starts with zero debt.
            ),
            Some(entity)
        );
        for queued in names(&factory["queued"]) {
            let queued = sim.interner.get(queued).unwrap();
            sim.production
                .factories_mut()
                .test_enqueue_kernel(holder, kind, queued, 0, 0);
        }
    }
    for (index, lab) in row["secret_labs"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        let id = sim
            .spawn_object(
                &format!("LAB{index}"),
                "H0",
                10 + index as u16,
                10,
                0,
                rules,
            )
            .unwrap();
        if let Some(name) = lab["fallback"].as_str() {
            let type_id = sim.interner.intern(name);
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .assign_secret_lab_fallback((query(row, rules).category, type_id));
        }
    }
    // The row's counters are the house's whole count, its factories'
    // objects included.
    let mut tracking = HouseTracking::default();
    tracking.add_airport_docks(house["docks"].as_i64().map_or(0, |docks| docks as i32));
    for (key, active) in [("owned", false), ("active", true)] {
        for counter in row[key].as_array().into_iter().flatten() {
            let type_id = sim.interner.get(counter[1].as_str().unwrap()).unwrap();
            let category = category(counter[0].as_str().unwrap());
            let count = int(&counter[2]);
            if active {
                tracking.set_active_for_test(category, type_id, count);
            } else {
                tracking.set_owned_for_test(category, type_id, count);
            }
        }
    }
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_tracking_for_test(tracking);
    (sim, owner)
}

fn query(row: &Value, rules: &RuleSet) -> ObjectType {
    let class = match row["class"].as_str().unwrap() {
        "unit" => ObjectCategory::Vehicle,
        "infantry" => ObjectCategory::Infantry,
        "aircraft" => ObjectCategory::Aircraft,
        _ => ObjectCategory::Building,
    };
    rules.object_in_category(class, "Q").unwrap().clone()
}

fn upgrade_prerequisite_world() -> (Simulation, RuleSet, InternedId) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=C1\n[VehicleTypes]\n0=Q\n\
         [BuildingTypes]\n0=HOST\n1=UPGRADE\n2=OTHER\n\
         [Q]\nTechLevel=1\nPrerequisite=UPGRADE\n\
         [HOST]\nStrength=100\nUpgrades=3\n\
         [UPGRADE]\nStrength=100\nPowersUpBuilding=HOST\n\
         [OTHER]\nStrength=100\n",
    ))
    .unwrap();
    let (mut sim, owner) = sim_for(&json!({}), &rules);
    for (id, name, x) in [(1, "HOST", 10), (2, "OTHER", 14)] {
        crate::sim::production::tests::spawn_structure(&mut sim, &rules, id, "H0", name, x, 10);
    }
    let upgrade = sim.interner.get("UPGRADE").unwrap();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .set_building_upgrades_for_test(0, [None, None, Some(upgrade)]);
    (sim, rules, owner)
}

/// Steam CanBuild4F7DF8..4F7E4E stops at the last eligible House building,
/// even when an earlier host carries the prerequisite. Its fixed three-type
/// scan does not read the upgrade count or compare PowersUpBuilding's name.
#[test]
fn upgrade_prerequisite_uses_house_order_and_all_three_type_slots() {
    let (mut sim, rules, owner) = upgrade_prerequisite_world();
    let product = rules.object("Q").unwrap();
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, true),
        CanBuild::No,
        "the newer unrelated building hides the earlier installed upgrade"
    );
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .base_projection_mut()
        .replace_buildings_for_test(vec![2, 1]);
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, true),
        CanBuild::Yes,
        "House order decides; the third type slot counts even with count zero"
    );
    let upgrade = sim.interner.get("UPGRADE").unwrap();
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .set_building_upgrades_for_test(0, [None, None, Some(upgrade)]);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .base_projection_mut()
        .replace_buildings_for_test(vec![1, 2]);
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, true),
        CanBuild::Yes,
        "the selected building need not match the declared host type"
    );
}

#[test]
fn upgrade_prerequisite_skips_limbo_offline_and_both_selling_missions() {
    use crate::sim::mission::MissionDispatchTimer;
    use crate::sim::mission::state::MissionTestFixture;

    let (mut sim, rules, owner) = upgrade_prerequisite_world();
    let product = rules.object("Q").unwrap();
    let guard = MissionId::from_known(MissionType::Guard);
    let selling = MissionId::from_known(MissionType::Selling);
    for (limbo, online, current, queued) in [
        (true, true, guard, MissionId::NONE),
        (false, false, guard, MissionId::NONE),
        (false, true, selling, MissionId::NONE),
        (false, true, guard, selling),
    ] {
        let latest = sim.substrate.entities.get_mut(2).unwrap();
        latest.lifecycle.set_in_limbo_for_test(limbo);
        latest.set_building_online_for_test(online);
        latest.mission.apply_test_fixture(MissionTestFixture {
            current,
            suspended: MissionId::NONE,
            queued,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        assert_eq!(
            can_build(&sim, &rules, owner, product, false, true),
            CanBuild::Yes,
            "skip latest building: limbo={limbo}, online={online}, current={current:?}, queued={queued:?}"
        );
    }
}

#[test]
fn can_build_matches_the_original() {
    for row in comparable_can_build_rows() {
        let rules = rules_for(&row);
        let (sim, owner) = sim_for(&row, &rules);
        let args = row["args"].as_array().unwrap();
        let answer = can_build(
            &sim,
            &rules,
            owner,
            &query(&row, &rules),
            args[0].as_bool().unwrap(),
            args[1].as_bool().unwrap(),
        );
        let answer = match answer {
            CanBuild::No => 0,
            CanBuild::Yes => 1,
            CanBuild::AtLimit => -1,
        };
        assert_eq!(answer, int(&row["answer"]), "{}", row["name"]);
    }
}

/// ReadOwners4750D0 clears DA0 before comma tokenization; CanBuild4F79E4
/// rejects that zero mask. The limit-only and prerequisite-override arms
/// precede this country gate in the original function.
#[test]
fn comma_only_required_houses_refuses_ordinary_can_build_but_keeps_shortcuts() {
    let make = |required: &str| {
        RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Countries]\n0=C0\n1=C1\n[BuildingTypes]\n0=Q\n1=BYPASS\n\
             [Q]\nStrength=100\nTechLevel=1\nOwner=C1\nPrerequisiteOverride=BYPASS\n\
             RequiredHouses={required}\n[BYPASS]\nStrength=100\n"
        )))
        .unwrap()
    };
    let rules = make(",,,");
    let product = rules.object("Q").unwrap();
    let (mut sim, owner) = sim_for(&json!({}), &rules);
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, true),
        CanBuild::No
    );
    let (ai, ai_owner) = sim_for(&json!({"house":{"human":false}}), &rules);
    assert_eq!(
        can_build(&ai, &rules, ai_owner, product, false, true),
        CanBuild::No
    );
    assert_eq!(
        can_build(&sim, &rules, owner, product, true, true),
        CanBuild::Yes
    );

    crate::sim::production::tests::spawn_structure(&mut sim, &rules, 1, "H0", "BYPASS", 10, 10);
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, true),
        CanBuild::Yes
    );
    let blank = make("");
    let (sim, owner) = sim_for(&json!({}), &blank);
    assert_eq!(
        can_build(&sim, &blank, owner, blank.object("Q").unwrap(), false, true),
        CanBuild::Yes,
        "an empty ReadString keeps the unrestricted constructor default"
    );
}

// Ares hook 0x50B370 (10024F80) replaces CheckBuildLimit; its own tests are
// in can_build_ares_tests.rs.
#[test]
fn check_build_limit_matches_the_original() {
    for row in rows("check_build_limit") {
        let rules = rules_for(&row);
        let (sim, owner) = sim_for(&row, &rules);
        let answer = check_build_limit(&sim, &rules, owner, &query(&row, &rules));
        assert_eq!(i32::from(answer), int(&row["answer"]), "{}", row["name"]);
    }
}

/// House4F78E4..4F7925 consults its captured labs before TechLevel,
/// country and ordinary prerequisites, then still asks the build limit.
/// The declared SecretInfantry override isolates this gate from the separate
/// PostMap68C050 RNG assignment. Original whole CanBuild/getter controls:
/// tools/can_build_secret_lab.json (admission).
#[test]
fn captured_secret_lab_grants_before_normal_tech_admission_and_retracts_on_loss() {
    let rules = RuleSet::from_ini(
        &IniFile::from_str(
            "[Countries]\n0=C0\n[C0]\nSide=GDI\n\
             [InfantryTypes]\n0=Q\n\
             [BuildingTypes]\n0=LAB\n1=MISSING\n2=FACTORY\n\
             [Q]\nTechLevel=-1\nStrength=100\nCost=100\nOwner=C0\nPrerequisite=MISSING\n\
             [LAB]\nStrength=100\nCapturable=yes\nNeedsEngineer=yes\nSecretLab=yes\nSecretInfantry=Q\n\
             [MISSING]\nStrength=100\n[FACTORY]\nStrength=100\nFactory=InfantryType\nOwner=C0\n",
        ),
    )
    .unwrap();
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let country = sim.interner.intern("C0");
    let owner = sim.interner.intern("Self");
    let other = sim.interner.intern("Other");
    for (id, human) in [(owner, true), (other, false)] {
        sim.houses
            .register(HouseState::new(id, 0, Some(country), human, 1000, 10));
    }
    if rules.ares_form() {
        crate::sim::production::tests::spawn_structure(
            &mut sim, &rules, 10000, "Self", "FACTORY", 20, 20,
        );
    }
    let lab = sim.spawn_object("LAB", "Other", 12, 5, 0, &rules).unwrap();
    let product = rules.object("Q").unwrap();
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, true),
        CanBuild::No
    );
    sim.change_owner_with_rules(lab, owner, &rules, None, Default::default());
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, true),
        CanBuild::Yes,
        "captured lab grant bypasses TechLevel=-1 and the unmet ordinary prerequisite"
    );
    sim.change_owner_with_rules(lab, other, &rules, None, Default::default());
    assert_eq!(
        can_build(&sim, &rules, owner, product, false, true),
        CanBuild::No
    );
}

#[test]
fn can_build_after_runtime_tech_level_change_matches_original_current_value_controls() {
    for row in comparable_can_build_rows() {
        let native_tech_level = row["type"]["tech"].as_i64().unwrap_or(1) as i32;
        let mut initial = row.clone();
        // The original corpus's current value comes from the trigger owner,
        // deliberately differing from this fixture's frozen reader output.
        initial["type"]["tech"] = json!(if native_tech_level == -1 { 255 } else { -1 });
        let rules = rules_for(&initial);
        let (mut sim, owner) = sim_for(&row, &rules);
        sim.set_object_type_tech_level("Q", native_tech_level, &rules);
        let args = row["args"].as_array().unwrap();
        let answer = can_build(
            &sim,
            &rules,
            owner,
            &query(&row, &rules),
            args[0].as_bool().unwrap(),
            args[1].as_bool().unwrap(),
        );
        let answer = match answer {
            CanBuild::No => 0,
            CanBuild::Yes => 1,
            CanBuild::AtLimit => -1,
        };
        assert_eq!(answer, int(&row["answer"]), "{}", row["name"]);
    }
}

/// Retail `[General] PrerequisiteProcAlternate=SMIN` through the production
/// rules layers: a Yuri player's Slave Miner meets YAWEAP's `PROC` without a
/// refinery. And `PadAircraft=ORCA,BEAG`: the AirportBound Harrier is
/// limited by its house's docks.
#[test]
fn retail_slave_miner_meets_proc_and_docks_limit_harriers() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    assert_eq!(
        rules.general.prerequisite_proc_alternate.as_deref(),
        Some("SMIN")
    );
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let on_map = |sim: &mut Simulation, house, category, name: &str, count| {
        let type_id = sim.interner.get(name).unwrap();
        let tracking = sim.houses.get_mut(&house).unwrap().tracking_mut();
        tracking.set_active_for_test(category, type_id, count);
        tracking.set_owned_for_test(category, type_id, count);
    };

    let yuri = sim.interner.intern("YuriCountry");
    sim.houses
        .register(HouseState::new(yuri, 2, None, true, 0, 10));
    on_map(&mut sim, yuri, EntityCategory::Structure, "YABRCK", 1);
    on_map(&mut sim, yuri, EntityCategory::Structure, "YACNST", 1);
    if rules.ares_form() {
        crate::sim::production::tests::spawn_structure(
            &mut sim,
            rules,
            10000,
            "YuriCountry",
            "YACNST",
            20,
            20,
        );
    }
    let yaweap = rules.object("YAWEAP").unwrap();
    assert_eq!(
        can_build(&sim, rules, yuri, yaweap, false, true),
        CanBuild::No
    );
    on_map(&mut sim, yuri, EntityCategory::Unit, "SMIN", 1);
    assert_eq!(
        can_build(&sim, rules, yuri, yaweap, false, true),
        CanBuild::Yes
    );

    let americans = sim.interner.intern("Americans");
    let mut house = HouseState::new(americans, 0, None, true, 0, 10);
    house.tracking_mut().add_airport_docks(4);
    sim.houses.register(house);
    let orca = rules.object("ORCA").unwrap();
    assert!(orca.airport_bound);
    on_map(&mut sim, americans, EntityCategory::Aircraft, "ORCA", 3);
    // Below the docks Ares 10024F80 (hook 0x50B370) goes on to the limit and
    // a usable factory, and this house owns no airfield.
    assert_eq!(
        check_build_limit(&sim, rules, americans, orca),
        rules.ares_form()
    );
    on_map(&mut sim, americans, EntityCategory::Aircraft, "BEAG", 1);
    assert!(check_build_limit(&sim, rules, americans, orca));
}

#[test]
fn secret_lab_can_build_matches_original_human_ai_limit_and_priority_controls() {
    let native: Value =
        serde_json::from_str(crate::test_fixture::text("tools/can_build_secret_lab.json")).unwrap();
    for row in native["admission"].as_array().unwrap() {
        let rules = rules_for(row);
        let (sim, owner) = sim_for(row, &rules);
        let args = row["args"].as_array().unwrap();
        let answer = can_build(
            &sim,
            &rules,
            owner,
            &query(row, &rules),
            args[0].as_bool().unwrap(),
            args[1].as_bool().unwrap(),
        );
        let answer = match answer {
            CanBuild::No => 0,
            CanBuild::Yes => 1,
            CanBuild::AtLimit => -1,
        };
        assert_eq!(answer, int(&row["answer"]), "{}", row["name"]);
    }
}

/// VERA's room bans refuse the retail Spy and the stolen-tech units to every
/// house, even on the computer's limit-only FindFactory route and after a
/// spy stole the tech.
#[test]
fn retail_room_bans_refuse_the_spy_and_stolen_tech_units() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let americans = sim.interner.intern("Americans");
    let mut house = HouseState::new(americans, 0, None, false, 0, 10);
    house.steal_tech(2);
    sim.houses.register(house);
    let spy = rules.object("SPY").unwrap();
    let psi_commando = rules.object("PTROOP").unwrap();
    for product in [spy, psi_commando] {
        assert_eq!(
            can_build(&sim, rules, americans, product, false, true),
            CanBuild::Yes
        );
    }

    sim.session.game_options_mut_for_test().bans.spy = true;
    assert_eq!(
        can_build(&sim, rules, americans, spy, false, true),
        CanBuild::No
    );
    assert_eq!(
        can_build(&sim, rules, americans, spy, true, true),
        CanBuild::No
    );
    assert_eq!(
        can_build(&sim, rules, americans, psi_commando, false, true),
        CanBuild::Yes
    );
    sim.session.game_options_mut_for_test().bans.stolen_tech = true;
    assert_eq!(
        can_build(&sim, rules, americans, psi_commando, false, true),
        CanBuild::No
    );
}

/// Rules for the Phobos `BuildLimitGroup.*` gate. `UPG` is the asked
/// BuildingType (an upgrade when `powers_up` is set); `Q` a plain
/// VehicleType; `GT` a group member and `EX` an ExtraLimit type. `q_keys`
/// (newline-terminated) is appended to `[UPG]`.
#[cfg(feature = "phobos")]
fn build_limit_group_rules(q_keys: &str, powers_up: bool) -> RuleSet {
    let mut ini = String::from(
        "[Countries]\n0=C0\n[VehicleTypes]\n0=Q\n1=GT\n2=EX\n\
         [BuildingTypes]\n0=UPG\n1=HOST\n\
         [UPG]\nStrength=100\nTechLevel=1\n",
    );
    if powers_up {
        ini.push_str("PowersUp.Buildings=HOST\n");
    }
    ini.push_str(q_keys);
    ini.push_str(
        "[HOST]\nStrength=100\nTechLevel=1\n\
         [GT]\nStrength=100\nTechLevel=1\n\
         [EX]\nStrength=100\nTechLevel=1\n",
    );
    RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap()
}

/// Boundary 1: reaching the group cap refuses one more through both native
/// owners, and only a `PowersUp.Buildings` item takes the CanBuild gate
/// (`Hooks.Upgrade.cpp:104-108`).
#[test]
#[cfg(feature = "phobos")]
fn build_limit_group_reached_refuses_one_more() {
    let rules = build_limit_group_rules("BuildLimitGroup.Types=GT\nBuildLimitGroup.Nums=2\n", true);
    let upgrade = rules
        .object_in_category(ObjectCategory::Building, "UPG")
        .unwrap();
    // owned(GT)=1 -> sum 1 < 2 -> still buildable.
    let (sim, owner) = sim_for(&json!({"owned": [["unit", "GT", 1]]}), &rules);
    assert_eq!(
        can_build(&sim, &rules, owner, upgrade, true, true),
        CanBuild::Yes
    );
    assert!(!check_build_limit(&sim, &rules, owner, upgrade));
    // owned(GT)=2 -> sum 2 >= 2 + 1 - 1 -> TemporarilyUnbuildable.
    let (sim, owner) = sim_for(&json!({"owned": [["unit", "GT", 2]]}), &rules);
    assert_eq!(
        can_build(&sim, &rules, owner, upgrade, true, true),
        CanBuild::AtLimit
    );
    assert!(check_build_limit(&sim, &rules, owner, upgrade));

    // The same keys without `PowersUp.Buildings`: the grey-out still caps the
    // type, `CanBuild` does not (the native hook's own guard).
    let plain =
        build_limit_group_rules("BuildLimitGroup.Types=GT\nBuildLimitGroup.Nums=2\n", false);
    let plain_upgrade = plain
        .object_in_category(ObjectCategory::Building, "UPG")
        .unwrap();
    let (sim, owner) = sim_for(&json!({"owned": [["unit", "GT", 2]]}), &plain);
    assert_eq!(
        can_build(&sim, &plain, owner, plain_upgrade, true, true),
        CanBuild::Yes
    );
    assert!(check_build_limit(&sim, &plain, owner, plain_upgrade));
}

/// Boundary 2: `ExtraLimit` appends a quota to every limit (MaxNum caps it).
#[test]
#[cfg(feature = "phobos")]
fn build_limit_group_extra_limit_adds_quota_and_maxnum_caps_it() {
    let base = "BuildLimitGroup.Types=GT\nBuildLimitGroup.Nums=2\n\
                BuildLimitGroup.ExtraLimit.Types=EX\nBuildLimitGroup.ExtraLimit.Nums=1\n";
    let row = json!({"owned": [["unit", "GT", 3], ["unit", "EX", 2]]});
    // Owning 2 EX adds 2*1 -> limit 4, so owned(GT)=3 stays under the cap.
    let rules = build_limit_group_rules(base, true);
    let upgrade = rules
        .object_in_category(ObjectCategory::Building, "UPG")
        .unwrap();
    let (sim, owner) = sim_for(&row, &rules);
    assert_eq!(
        can_build(&sim, &rules, owner, upgrade, true, true),
        CanBuild::Yes,
        "ExtraLimit appends owned-EX quota so 3 group units are allowed"
    );
    assert!(!check_build_limit(&sim, &rules, owner, upgrade));

    // MaxNum=3 caps the appended limit to 3, so the same 3 reach it.
    let capped = build_limit_group_rules(
        &format!("{base}BuildLimitGroup.ExtraLimit.MaxNum=3\n"),
        true,
    );
    let upgrade = capped
        .object_in_category(ObjectCategory::Building, "UPG")
        .unwrap();
    let (sim, owner) = sim_for(&row, &capped);
    assert_eq!(
        can_build(&sim, &capped, owner, upgrade, true, true),
        CanBuild::AtLimit,
        "MaxNum=3 caps the appended quota so 3 group units reach the limit"
    );
    assert!(check_build_limit(&sim, &capped, owner, upgrade));
}

/// Boundary 3: a queue-only reach greys the cameo regardless of
/// `NotBuildableIfQueueMatch` (`ReachedBuildLimit` returns true on the
/// queue-inclusive reach; the flag only picks `RemoveProduction`'s count),
/// while `CanBuild` releases it in both settings (`UpgradeCheck`'s own queue
/// pass never returns unbuildable for a queued reach).
#[test]
#[cfg(feature = "phobos")]
fn build_limit_group_queue_only_reach_greys_but_stays_buildable() {
    let base = "BuildLimitGroup.Types=GT\nBuildLimitGroup.Nums=2\n";
    // owned(GT)=1 (<2) plus one queued GT -> reached only by the queue.
    let row = json!({
        "owned": [["unit", "GT", 1]],
        "factories": [{"kind": "vehicle", "object": "Q", "queued": ["GT"]}]
    });
    for extra in ["", "BuildLimitGroup.NotBuildableIfQueueMatch=yes\n"] {
        let rules = build_limit_group_rules(&format!("{base}{extra}"), true);
        let upgrade = rules
            .object_in_category(ObjectCategory::Building, "UPG")
            .unwrap();
        let (sim, owner) = sim_for(&row, &rules);
        assert_eq!(
            can_build(&sim, &rules, owner, upgrade, true, true),
            CanBuild::Yes,
            "a queue-only reach stays buildable"
        );
        assert!(
            check_build_limit(&sim, &rules, owner, upgrade),
            "the grey-out counts the queued copy"
        );
    }
}

/// Boundary 4: the paired `Nums` branch (`UpgradeCheck:892-947`) ANDs the
/// non-item slots on the first pass, then the second pass releases when any
/// slot is below its threshold.
#[test]
#[cfg(feature = "phobos")]
fn build_limit_group_paired_nums_ands_and_releases() {
    let rules = build_limit_group_rules(
        "BuildLimitGroup.Types=GT,EX\nBuildLimitGroup.Nums=1,1\n",
        true,
    );
    let upgrade = rules
        .object_in_category(ObjectCategory::Building, "UPG")
        .unwrap();
    // Both slots at 1: the first pass's non-item slots reach, so unbuildable.
    let (sim, owner) = sim_for(
        &json!({"owned": [["unit", "GT", 1], ["unit", "EX", 1]]}),
        &rules,
    );
    assert_eq!(
        can_build(&sim, &rules, owner, upgrade, true, true),
        CanBuild::AtLimit
    );
    // EX below its slot: the first pass fails; the second pass finds the
    // order's second threshold unmet and releases.
    let (sim, owner) = sim_for(&json!({"owned": [["unit", "GT", 1]]}), &rules);
    assert_eq!(
        can_build(&sim, &rules, owner, upgrade, true, true),
        CanBuild::Yes
    );
    // The grey-out's own multi-limit formula is `owned * (Factor - limit)`
    // (`ReachedBuildLimit:1142`); with Factor == limit every slot contributes
    // 0 and the group reads reached even at zero owned. Kept as the native
    // defect rather than the CanBuild answer.
    assert!(check_build_limit(&sim, &rules, owner, upgrade));
}

/// Phobos `BuildingTypeExt::CanUpgrade`
/// (`Ext/BuildingType/Hooks.Upgrade.cpp:21-25`) walks every
/// `PowersUp.Buildings` entry; Ares `HouseOwnsSpecific100082B0` asks it per
/// building prerequisite. The legacy `PowersUpBuilding` string is not the
/// whole set: here it names `HOST_A`, which the list puts second.
#[test]
#[cfg(feature = "phobos")]
fn powers_up_buildings_prerequisite_accepts_any_declared_host() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=HOST_A\n1=HOST_B\n2=UPGRADE\n\
         [HOST_A]\nStrength=100\nUpgrades=3\n\
         [HOST_B]\nStrength=100\nUpgrades=3\n\
         [UPGRADE]\nStrength=100\n\
         PowersUpBuilding=HOST_A\nPowersUp.Buildings=HOST_B,HOST_A\n",
    ))
    .unwrap();
    let index = rules.ares_find_building("UPGRADE").unwrap();
    for (host, expected) in [
        (None, false),
        (Some("HOST_A"), true),
        (Some("HOST_B"), true),
    ] {
        let (mut sim, owner) = sim_for(&json!({}), &rules);
        if let Some(host) = host {
            crate::sim::production::tests::spawn_structure(&mut sim, &rules, 1, "H0", host, 10, 10);
            let upgrade = sim.interner.get("UPGRADE").unwrap();
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .set_building_upgrades_for_test(1, [None, None, Some(upgrade)]);
        }
        assert_eq!(
            specific_prerequisite_on_map(&sim, &rules, sim.houses.get(&owner).unwrap(), index),
            expected,
            "host {host:?}"
        );
    }
}

/// Phobos `BuildingTypeExt::GetUpgradesAmount`
/// (`Ext/BuildingType/Body.cpp:160-161`) sums the installed slots over every
/// host, so Ares' `CountOwned10020AF0` build-limit census sees an upgrade
/// installed on the second host too.
#[test]
#[cfg(feature = "phobos")]
fn powers_up_buildings_build_limit_counts_every_host() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=HOST_A\n1=HOST_B\n2=UPGRADE\n\
         [HOST_A]\nStrength=100\nUpgrades=3\n\
         [HOST_B]\nStrength=100\nUpgrades=3\n\
         [UPGRADE]\nStrength=100\nTechLevel=1\nBuildLimit=2\n\
         PowersUpBuilding=HOST_A\nPowersUp.Buildings=HOST_B,HOST_A\n",
    ))
    .unwrap();
    let (mut sim, owner) = sim_for(&json!({}), &rules);
    let upgrade = sim.interner.get("UPGRADE").unwrap();
    for (id, host, x) in [(1, "HOST_A", 10), (2, "HOST_B", 14)] {
        crate::sim::production::tests::spawn_structure(&mut sim, &rules, id, "H0", host, x, 10);
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .set_building_upgrades_for_test(1, [None, None, Some(upgrade)]);
    }
    let upgrade_type = rules.object("UPGRADE").unwrap();
    assert_eq!(
        ares_owned_build_limit_count(&sim, &rules, sim.houses.get(&owner).unwrap(), upgrade_type),
        2,
        "one installed slot on each declared host"
    );
    assert_eq!(
        can_build(&sim, &rules, owner, upgrade_type, true, true),
        CanBuild::AtLimit,
        "BuildLimit=2 is reached only when both hosts count"
    );
}

#[cfg(feature = "phobos")]
fn queued_of(sim: &Simulation, owner: InternedId, category: ProductionCategory, name: &str) -> i32 {
    let type_id = sim.interner.get(name).unwrap();
    sim.production
        .factories()
        .count_total(owner, category, type_id)
}

/// `RemoveProduction @ Ext/House/Body.cpp:768-784`: a reached group drops the
/// excess copies of the **queried** type from its factory and leaves the held
/// product. Outside arm (the queried type is not a member) removes every copy.
#[test]
#[cfg(feature = "phobos")]
fn build_limit_group_reach_prunes_the_excess_queue() {
    let rules = build_limit_group_rules("BuildLimitGroup.Types=GT\nBuildLimitGroup.Nums=2\n", true);
    let row = json!({
        "owned": [["unit", "GT", 2]],
        "factories": [{"kind": "building", "object": "HOST", "queued": ["UPG", "UPG"]}]
    });
    let (mut sim, owner) = sim_for(&row, &rules);
    let upgrade = rules.object("UPG").unwrap().clone();
    assert!(
        check_build_limit(&sim, &rules, owner, &upgrade),
        "owned GT=2 reaches Nums=2"
    );
    assert_eq!(
        queued_of(&sim, owner, ProductionCategory::Building, "UPG"),
        2
    );
    phobos_trim_build_limit_group_queue(&mut sim, &rules, owner, &upgrade);
    assert_eq!(
        queued_of(&sim, owner, ProductionCategory::Building, "UPG"),
        0,
        "the outside arm passes -1, so every queued copy leaves"
    );
    assert_eq!(
        queued_of(&sim, owner, ProductionCategory::Building, "HOST"),
        1,
        "the held product stays"
    );
}

/// The tail prune of the multi-limit arm (`Body.cpp:1179`) only fires when
/// `realReached || NotBuildableIfQueueMatch`; the flag is the difference.
#[test]
#[cfg(feature = "phobos")]
fn build_limit_group_tail_prune_follows_not_buildable_if_queue_match() {
    let base = "BuildLimitGroup.Types=GT,EX\nBuildLimitGroup.Nums=2,1\n";
    let row = json!({
        "owned": [["unit", "GT", 1]],
        "factories": [
            {"kind": "vehicle", "object": "Q", "queued": ["GT"]},
            {"kind": "building", "object": "HOST", "queued": ["UPG"]}
        ]
    });
    for (extra, expected) in [
        ("", 1),                                               // realReached=false: keep it
        ("BuildLimitGroup.NotBuildableIfQueueMatch=yes\n", 0), // prune all
    ] {
        let rules = build_limit_group_rules(&format!("{base}{extra}"), true);
        let upgrade = rules.object("UPG").unwrap().clone();
        let (mut sim, owner) = sim_for(&row, &rules);
        assert!(
            check_build_limit(&sim, &rules, owner, &upgrade),
            "the grey-out still reaches without the flag"
        );
        phobos_trim_build_limit_group_queue(&mut sim, &rules, owner, &upgrade);
        assert_eq!(
            queued_of(&sim, owner, ProductionCategory::Building, "UPG"),
            expected,
            "{extra:?}"
        );
    }
}

/// `BuildLimitGroupUpgradeCheck` trims too (`Body.cpp:896,943,962,995,1008`):
/// a `PowersUp.Buildings` item outside the reached group drops every queued
/// copy when `num >= 1 - factor`, and a group that never reaches leaves the
/// queue alone.
#[test]
#[cfg(feature = "phobos")]
fn build_limit_group_upgrade_check_prunes_the_queue() {
    let row = json!({
        "owned": [["unit", "GT", 3]],
        "factories": [{"kind": "building", "object": "HOST", "queued": ["UPG", "UPG"]}]
    });
    let reached =
        build_limit_group_rules("BuildLimitGroup.Types=GT\nBuildLimitGroup.Nums=2\n", true);
    let upgrade = reached.object("UPG").unwrap().clone();
    let (sim, owner) = sim_for(&row, &reached);
    let gate = build_limit_group_upgrade_gate(&sim, &reached, owner, &upgrade, true);
    assert!(gate.reached, "owned GT=3 reaches Nums=2");
    assert_eq!(gate.removals, vec![-1], "the outside arm passes -1");

    let (mut sim, owner) = sim_for(&row, &reached);
    phobos_trim_build_limit_group_queue(&mut sim, &reached, owner, &upgrade);
    assert_eq!(
        queued_of(&sim, owner, ProductionCategory::Building, "UPG"),
        0,
        "every queued copy leaves"
    );

    // Owned GT=1 is below Nums=2: neither gate reaches, so nothing is pruned.
    let below = build_limit_group_rules("BuildLimitGroup.Types=GT\nBuildLimitGroup.Nums=2\n", true);
    let upgrade = below.object("UPG").unwrap().clone();
    let (mut sim, owner) = sim_for(
        &json!({
            "owned": [["unit", "GT", 1]],
            "factories": [{"kind": "building", "object": "HOST", "queued": ["UPG"]}]
        }),
        &below,
    );
    phobos_trim_build_limit_group_queue(&mut sim, &below, owner, &upgrade);
    assert_eq!(
        queued_of(&sim, owner, ProductionCategory::Building, "UPG"),
        1,
        "an unreached group keeps the queue"
    );
}
