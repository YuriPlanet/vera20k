//! Replay of `tools/can_build_oracle.json`: the original CanBuild and
//! CheckBuildLimit over synthetic houses, answered by [`can_build`] and
//! [`check_build_limit`] on the same fixture written as rules and house
//! state.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::production::{ProductionCategory, construct_active_factory_fixture};
use serde_json::{Value, json};

#[cfg(feature = "ares")]
#[path = "can_build_ares_tests.rs"]
mod ares_tests;

fn rows(section: &str) -> Vec<Value> {
    let data: Value =
        serde_json::from_str(crate::test_fixture::text("tools/can_build_oracle.json")).unwrap();
    data[section].as_array().unwrap().clone()
}

#[cfg(not(feature = "ares"))]
fn comparable_can_build_rows() -> impl Iterator<Item = Value> {
    rows("can_build").into_iter().filter(|row| {
        // These gamemd controls intentionally differ from Ares100082B0's
        // positive specific count and CountOwnedNow100032F0's tracked
        // alternates. The Ares corpus covers those component boundaries.
        !crate::rules::ARES_ENABLED
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
        construct_active_factory_fixture(&mut sim, rules, holder, kind, object)
            .expect("the factory's object constructs");
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

#[cfg(not(feature = "ares"))]
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
#[cfg(not(feature = "ares"))]
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
#[cfg(not(feature = "ares"))]
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
#[cfg(not(feature = "ares"))]
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
    #[cfg(feature = "ares")]
    crate::sim::production::tests::spawn_structure(
        &mut sim, &rules, 10000, "Self", "FACTORY", 20, 20,
    );
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
#[cfg(not(feature = "ares"))]
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
    #[cfg(feature = "ares")]
    crate::sim::production::tests::spawn_structure(
        &mut sim,
        rules,
        10000,
        "YuriCountry",
        "YACNST",
        20,
        20,
    );
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
    assert!(!check_build_limit(&sim, rules, americans, orca));
    on_map(&mut sim, americans, EntityCategory::Aircraft, "BEAG", 1);
    assert!(check_build_limit(&sim, rules, americans, orca));
}

#[test]
#[cfg(not(feature = "ares"))]
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
