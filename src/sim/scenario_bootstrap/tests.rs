//! Unit tests for fresh Scenario bootstrap: start gathering and assignment,
//! the stock-offline prefix plan, launch House population and alliances,
//! starting objects, and the bootstrap RNG handoff.

use super::launch_houses::project_pre_fill_start_assignment;
use super::start_positions::choose_quick_coop_start;
use super::starting_objects::place_starting_object_near_base;
use super::*;

// Each stock Country reaches its section, so its ParentCountry defaults to
// its own ID as in retail; a sectionless Country keeps ctor-empty
// ParentCountry and owns nothing.
const CONTROLLED_STOCK_COUNTRIES: &str = "[Countries]\n0=Americans\n1=Alliance\n2=French\n3=Germans\n4=British\n5=Africans\n6=Arabs\n7=Confederation\n8=Russians\n9=YuriCountry\n[Sides]\nAllied=Americans,Alliance,French,Germans,British\nSoviet=Africans,Arabs,Confederation,Russians\nThirdSide=YuriCountry\n[Americans]\nParentCountry=Americans\n[Alliance]\nParentCountry=Alliance\n[French]\nParentCountry=French\n[Germans]\nParentCountry=Germans\n[British]\nParentCountry=British\n[Africans]\nParentCountry=Africans\n[Arabs]\nParentCountry=Arabs\n[Confederation]\nParentCountry=Confederation\n[Russians]\nParentCountry=Russians\n[YuriCountry]\nParentCountry=YuriCountry\n";

/// Layer a test's source over the Countries fixture, so its own Country
/// keys join the fixture's sections; a fresh load would keep only the
/// first of two same-named sections.
fn launch_fixture_ini(source: &str) -> IniFile {
    let mut ini = IniFile::from_str(CONTROLLED_STOCK_COUNTRIES);
    ini.merge(&IniFile::from_str(source));
    ini
}

#[test]
fn starting_role_alliances_follow_admitted_rules_without_replacing_teams() {
    use crate::map::houses::are_houses_mutually_allied;
    use crate::sim::scenario_session::ScenarioGameMode;
    use crate::skirmish_launch::AiDifficulty;

    let mut launch = crate::headless_scenario::battle_session(
        "XMP03T4.MAP",
        &[
            (LaunchCountry::Russia, AiDifficulty::Normal),
            (LaunchCountry::Cuba, AiDifficulty::Normal),
        ],
    );
    launch.local_slot_mut().team = LaunchTeam::Team(0);
    launch.opponents[0].team = LaunchTeam::Team(0);
    let rules = RuleSet::from_ini(&launch_fixture_ini("")).unwrap();
    let mut sim = Simulation::new();
    populate_launch_houses(&mut sim, &normalized_launch_slots(&launch), &rules);
    let mode = ScenarioGameMode::from_game_mode(5);
    let class = Some(launch.mode.class);
    let stock = mode.stock_rule_policy(class);

    sim.install_session_admission(mode, class, stock);
    apply_skirmish_launch_alliances(&mut sim, &launch, &rules);
    let alliances = sim.house_alliances.as_map();
    assert!(are_houses_mutually_allied(alliances, "Player", "Computer1"));
    assert!(!are_houses_mutually_allied(
        alliances,
        "Computer1",
        "Computer2"
    ));

    sim.install_session_admission(mode, class, stock.with_starting_house_role_alliances(true));
    apply_skirmish_launch_alliances(&mut sim, &launch, &rules);
    let alliances = sim.house_alliances.as_map();
    assert!(are_houses_mutually_allied(alliances, "Player", "Computer1"));
    assert!(are_houses_mutually_allied(
        alliances,
        "Computer1",
        "Computer2"
    ));
}

fn starting_country_native_fixture(passive: bool) -> (serde_json::Value, RuleSet) {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/session_country_starting_ownership.json",
    ))
    .unwrap();
    let mut source = String::from("[General]\nBaseUnit=V_BASE\n");
    for (section, prefix) in [("VehicleTypes", "V_"), ("InfantryTypes", "I_")] {
        source.push_str(&format!("[{section}]\n"));
        for (index, row) in corpus["type_inputs"].as_array().unwrap().iter().enumerate() {
            let id = row["id"].as_str().unwrap();
            if id.starts_with(prefix) {
                source.push_str(&format!("{index}={id}\n"));
            }
        }
    }
    source.push_str(corpus["root"].as_str().unwrap());
    let mut ini = IniFile::from_str(&source);
    if passive {
        ini.merge(&IniFile::from_str(
            corpus["passive_layer"].as_str().unwrap(),
        ));
    }
    let rules = RuleSet::from_ini(&ini).unwrap();
    (corpus, rules)
}

#[test]
fn starting_country_candidate_lists_match_original_owner_only_admission() {
    let (corpus, rules) = starting_country_native_fixture(false);
    for row in corpus["type_inputs"].as_array().unwrap() {
        let object = rules.object(row["id"].as_str().unwrap()).unwrap();
        assert_eq!(
            object.owner_mask(&rules),
            row["owner_mask"].as_u64().unwrap() as u32,
            "{} Owner mask",
            row["id"],
        );
    }
    for country in corpus["country_inputs"].as_array().unwrap() {
        let id = country["id"].as_str().unwrap();
        let candidates = starting_unit_candidates_for_country(
            &rules,
            LaunchCountry::from_country_index(country["actual_index"].as_u64().unwrap() as u32),
            corpus["tech_level"].as_i64().unwrap() as i32,
        );
        for (category, actual) in [
            ("vehicles", candidates.vehicles),
            ("infantry", candidates.infantry),
        ] {
            let ids: Vec<_> = actual
                .iter()
                .map(|candidate| candidate.type_id.as_str())
                .collect();
            let expected: Vec<_> = corpus["candidate_lists"][id][category]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect();
            assert_eq!(ids, expected, "{id}/{category}");
        }
    }
    for row in corpus["candidate_house_tech_controls"].as_array().unwrap() {
        let index = corpus["country_inputs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|country| country["id"] == row["country"])
            .unwrap()["actual_index"]
            .as_u64()
            .unwrap();
        let candidates = starting_unit_candidates_for_country(
            &rules,
            LaunchCountry::from_country_index(index as u32),
            row["house_tech_level"].as_i64().unwrap() as i32,
        );
        for (category, actual) in [
            ("vehicles", candidates.vehicles),
            ("infantry", candidates.infantry),
        ] {
            let expected: Vec<_> = row[category]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect();
            assert_eq!(
                actual
                    .iter()
                    .map(|candidate| candidate.type_id.as_str())
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
}

#[test]
fn starting_country_budgets_match_original_actual_nonpassive_mask() {
    let (corpus, active_rules) = starting_country_native_fixture(false);
    let (_, passive_rules) = starting_country_native_fixture(true);
    let template =
        normalized_launch_slots(one_player_battle_launch("budget.map").session()).remove(0);
    for row in corpus["budget_rows"].as_array().unwrap() {
        let rules = if row["alliance_passive"].as_bool().unwrap() {
            &passive_rules
        } else {
            &active_rules
        };
        let slots: Vec<_> = row["participants"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| {
                let index = corpus["country_inputs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|country| country["id"] == *name)
                    .unwrap()["actual_index"]
                    .as_u64()
                    .unwrap();
                let mut slot = template.clone();
                slot.country = LaunchCountry::from_country_index(index as u32);
                slot
            })
            .collect();
        assert_eq!(
            starting_unit_budget(
                rules,
                slots.iter().map(|slot| slot.country.index()),
                corpus["tech_level"].as_i64().unwrap() as i32,
                corpus["unit_count"].as_i64().unwrap() as i32
            ),
            row["budget"].as_i64().unwrap() as i32,
            "{}",
            row["name"],
        );
    }
}

/// Ares MPGameModeClass_CreateStartingUnits_UnitCost (1001EE10, hook
/// 0x5D6D9A): a set StartInMultiplayerUnitCost prices each unit, so
/// the budget is UnitCount times it, whatever the eligible types cost.
#[test]
fn ares_start_in_multiplayer_unit_cost_prices_each_starting_unit() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let ini = |general: &str| {
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            &format!(
                "[General]\n{general}[Countries]\n0=Americans\n[Americans]\nSide=GDI\n\
                 [VehicleTypes]\n0=MTNK\n[MTNK]\nOwner=Americans\nCost=700\nTechLevel=1\n"
            ),
        ))
        .unwrap()
    };
    let americans = [crate::rules::ruleset::CountryIdx(0)];
    assert_eq!(starting_unit_budget(&ini(""), americans, 10, 3), 2100);
    let priced = ini("StartInMultiplayerUnitCost=250\n");
    assert_eq!(starting_unit_budget(&priced, americans, 10, 3), 750);
    assert_eq!(starting_unit_budget(&priced, americans, 10, 0), 0);
}

const ARES_START_TYPES_RULES: &str = "[General]\nBaseUnit=AMCV\n\
     [Countries]\n0=Americans\n1=Russians\n2=Iraq\n3=Cuba\n\
     [Americans]\nSide=GDI\nStartInMultiplayer.Types=HTNK,E1,GAPOWR,AMCV\n\
     [Russians]\nSide=Nod\n[Iraq]\nSide=Nod\nStartInMultiplayer.Types=GAPOWR\n\
     [Cuba]\nSide=Nod\nStartInMultiplayer.Types=NOSUCH\n\
     [VehicleTypes]\n0=AMCV\n1=MTNK\n2=HTNK\n[InfantryTypes]\n0=E1\n\
     [BuildingTypes]\n0=GAPOWR\n\
     [AMCV]\nOwner=Americans\nCost=1000\nTechLevel=1\n\
     [MTNK]\nOwner=Russians\nCost=700\nTechLevel=1\n\
     [HTNK]\nOwner=Russians\nCost=300\nTechLevel=99\nAllowedToStartInMultiplayer=no\n\
     [E1]\nOwner=Russians\nCost=100\nTechLevel=99\n[GAPOWR]\nOwner=Americans\n";

/// Ares MPGameMode_SpawnStartingUnits_Types (1001F220, hook 0x5D7163):
/// the country's own list is the candidates, unfiltered (even its
/// BaseUnit); a list of only other types takes the refusal at 0x5D72AB,
/// one that resolved to nothing the success edge 0x5D743E.
#[test]
fn ares_start_in_multiplayer_types_replace_the_starting_candidates() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::rules::ini_parser::IniFile::from_str(ARES_START_TYPES_RULES),
    )
    .unwrap();
    let candidates = |index| {
        starting_unit_candidates_for_country(&rules, LaunchCountry::from_country_index(index), 1)
    };
    let ids = |pool: &[StartingUnitCandidate]| {
        pool.iter()
            .map(|candidate| candidate.type_id.clone())
            .collect::<Vec<_>>()
    };
    let listed = candidates(0);
    assert_eq!(ids(&listed.vehicles), ["HTNK", "AMCV"]);
    assert_eq!(ids(&listed.infantry), ["E1"]);
    assert!(!listed.listed_empty);
    let empty = candidates(3);
    assert!(empty.vehicles.is_empty() && empty.infantry.is_empty() && empty.listed_empty);
    let refused = candidates(2);
    assert!(refused.vehicles.is_empty() && refused.infantry.is_empty());
    assert!(!refused.listed_empty);
}

/// Ares MPGameModeClass_CreateStartingUnits_UnitCost (1001EE10, hook
/// 0x5D6D9A): the listed Unit/Infantry types join the types the unlisted
/// countries own, then BaseUnit leaves: {HTNK, E1, MTNK} averages
/// (1 + 1100) / 3 = 367, where the native average over both countries'
/// owned types is MTNK's 700.
#[test]
fn ares_start_in_multiplayer_types_enter_the_unit_cost_average() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = crate::rules::ruleset::RuleSet::from_ini(
        &crate::rules::ini_parser::IniFile::from_str(ARES_START_TYPES_RULES),
    )
    .unwrap();
    let countries = [0, 1].map(crate::rules::ruleset::CountryIdx);
    assert_eq!(starting_unit_budget(&rules, countries, 10, 3), 367 * 3);
}

#[test]
fn starting_country_native_qualifier_sentinels_reach_shared_consumers() {
    let corpus =
        crate::test_fixture::json("tools/rules_oracle/session_country_starting_ownership.json");
    let gate = &corpus["qualifier_gates"];
    let mut source = String::from("[Countries]\n");
    for (index, id) in gate["sentinel_registry_ids"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        source.push_str(&format!("{index}={}\n", id.as_str().unwrap()));
    }
    // The native House's Country reads its retained ParentCountry, which
    // supplies the Owner-side bit; its actual index supplies the rest.
    let actual_country = gate["actual_country"].as_str().unwrap();
    source.push_str(&format!(
        "[{actual_country}]\nParentCountry={}\n",
        gate["owner_parent"].as_str().unwrap()
    ));
    let type_id = gate["type_id"].as_str().unwrap();
    source.push_str(&format!(
        "[VehicleTypes]\n0={type_id}\n[{type_id}]\nTechLevel=1\n"
    ));
    // The fixture is an Ares game exactly when its thread reads Rules
    // the Ares way.
    if crate::rules::ares_switch::ares_way() {
        source.push_str(
            "[BuildingTypes]\n0=QUALIFIER_FACTORY\n\
             [QUALIFIER_FACTORY]\nStrength=100\nFactory=UnitType\nOwner=Americans,Alliance\n",
        );
    }
    for row in gate["rows"].as_array().unwrap() {
        let mut ini = IniFile::from_str(&source);
        ini.merge(&IniFile::from_str(gate["owner_layer"].as_str().unwrap()));
        ini.merge(&IniFile::from_str(row["physical_layer"].as_str().unwrap()));
        let rules = RuleSet::from_ini(&ini).unwrap();
        let object = rules.object(type_id).unwrap();
        assert_eq!(
            object.owner_mask(&rules),
            gate["owner_mask"].as_u64().unwrap() as u32
        );
        assert_eq!(
            crate::sim::ai_buildable::house_country_bit(&rules, actual_country),
            1 << gate["actual_index"].as_u64().unwrap()
        );
        assert_eq!(
            crate::sim::ai_buildable::candidate_allowed(
                object,
                crate::sim::ai_buildable::CountryBuildMasks::for_country(&rules, actual_country),
                gate["planned_side"].as_i64().unwrap() as i32,
                true,
                &rules,
            ),
            row["first_buildable_result"].as_str() == Some(type_id),
            "{} FirstBuildable",
            row["name"],
        );
        // Only the original qualifier region is a native CanBuild
        // golden. Other production gates are open in this Rust scene.
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("QualifierHouse");
        let country = sim.interner.intern(actual_country);
        sim.houses
            .register(HouseState::new(owner, -1, Some(country), false, 0, 10));
        if rules.ares_form() {
            // The Ares CanBuild suffix also requires an online factory;
            // leave that gate open while comparing the Country qualifiers.
            let factory = sim
                .spawn_object("QUALIFIER_FACTORY", "QualifierHouse", 10, 10, 0, &rules)
                .unwrap();
            sim.substrate
                .entities
                .get_mut(factory)
                .unwrap()
                .finish_building_construction_for_test();
        }
        assert_eq!(
            crate::sim::production::can_build(&sim, &rules, owner, object, false, false)
                == crate::sim::production::CanBuild::Yes,
            row["can_build_qualifier_gate"].as_bool().unwrap(),
            "{} CanBuild qualifier integration",
            row["name"],
        );
    }
}

#[test]
fn starting_country_passive_blocks_both_mcv_and_extra_unit_callbacks() {
    // Original5D6EF8 skips both callbacks. Exercise the shared launch
    // consumer; this is a Rust integration check, not placement parity.
    for (passive, house_tech_level, changed_house_country) in [
        (false, 10, false),
        (false, 0, false),
        (true, 10, false),
        (false, 10, true),
    ] {
        let rules = RuleSet::from_ini(&launch_fixture_ini(&format!(
            "[General]\nBaseUnit=MTNK\n[Americans]\nMultiplayPassive={}\n[Alliance]\nMultiplayPassive=yes\n\
             [VehicleTypes]\n0=MTNK\n1=HTNK\n\
             [MTNK]\nStrength=300\nSpeed=6\nCost=100\nTechLevel=1\nOwner=Americans\nAllowedToStartInMultiplayer=yes\n\
             [HTNK]\nStrength=300\nSpeed=6\nCost=100\nTechLevel=1\nOwner=Americans\nAllowedToStartInMultiplayer=yes\n",
            if passive { "yes" } else { "no" },
        ))).unwrap();
        let mut session = one_player_battle_launch("passive.map").session().clone();
        session.options.bases = true;
        session.options.unit_count = 1;
        session.options.tech_level = 10;
        let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
        let map = one_start_prefix_map(Waypoint {
            index: 0,
            rx: 44,
            ry: 48,
        });
        let terrain = techno_constructor_flat_start_terrain(96);
        let mut sim = techno_constructor_start_sim(0xC701_1005, 96);
        sim.install_resolved_terrain_for_new_map(terrain.clone());
        initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &launch);
        let owner = sim.session.current_house().unwrap();
        if changed_house_country {
            let country = sim.interner.intern("Alliance");
            sim.houses
                .get_mut(&owner)
                .unwrap()
                .set_country_for_test(Some(country));
        }
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_tech_level_for_test(house_tech_level);
        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &HouseRoster::default(),
            &rules,
            &terrain,
            &launch,
        );
        let skips_forces = passive || changed_house_country;
        assert_eq!(result.spawned_mcvs, u32::from(!skips_forces));
        assert_eq!(
            sim.entities().len(),
            if skips_forces {
                0
            } else {
                1 + usize::from(house_tech_level > 0)
            },
        );
    }
}

const ARES_WITH_CONST_RULES: &str = "[General]\nBaseUnit=AMCV\n[AI]\nBuildConst=GACNST\n\
     [Americans]\nStartInMultiplayer.WithConst=yes\n[VehicleTypes]\n0=AMCV\n\
     [AMCV]\nStrength=1000\nSpeed=4\nOwner=Americans\nDeploysInto=GACNST\n\
     [BuildingTypes]\n0=GACNST\n\
     [GACNST]\nStrength=1000\nConstructionYard=yes\nOwner=Americans\n\
     [Clear]\nBuildable=yes\n";

/// Ares MPGameMode_SpawnBaseUnit_BuildConst (1001F0B0, hook 0x5D7048)
/// and CreateStartingUnits_BaseCenter (1001ED60, hook 0x5D6F61): a
/// WithConst house's base unit is its first BuildConst yard, one cell
/// up-left of its start for a 4x3 foundation, with Guard queued. Either
/// owner's yard opens during its Unlimbo (a human's at its cell put, a
/// computer's at its owner discovery) and commences Construction; a
/// computer's house also gets the deploy's AI half at that cell.
#[test]
fn ares_start_with_const_places_a_construction_yard_for_the_base_unit() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    use crate::sim::mission::{MissionId, MissionType};
    // `Foundation=` is an ART key of the type's Image.
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &launch_fixture_ini(ARES_WITH_CONST_RULES),
        &IniFile::from_str("[GACNST]\nFoundation=4x3\n"),
    )
    .unwrap();
    for human in [true, false] {
        let mut session = one_player_battle_launch("withconst.map").session().clone();
        session.options.bases = true;
        session.options.unit_count = 0;
        let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
        let map = one_start_prefix_map(Waypoint {
            index: 0,
            rx: 44,
            ry: 48,
        });
        let terrain = techno_constructor_flat_start_terrain(96);
        let mut sim = techno_constructor_start_sim(0xC701_1006, 96);
        // A skirmish's discovery rules: the cell put's DiscoveredBy(PlayerPtr)
        // runs outside a campaign (`0x0047E9C8..0x0047E9DC`).
        sim.session
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        sim.install_resolved_terrain_for_new_map(terrain.clone());
        initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &launch);
        let owner = sim.session.current_house().unwrap();
        if !human {
            let house = sim.houses.get_mut(&owner).unwrap();
            house.set_is_human_for_test(false);
            house.set_player_control_for_test(false);
        }
        let result = apply_explicit_skirmish_launch_session(
            &mut sim,
            &map,
            &HouseRoster::default(),
            &rules,
            &terrain,
            &launch,
        );
        assert_eq!(result.spawned_mcvs, 1);
        assert_eq!(sim.entities().len(), 1, "the yard replaces the MCV");
        let yard = sim
            .substrate
            .entities
            .values()
            .find(|entity| entity.category() == EntityCategory::Structure)
            .unwrap();
        assert_eq!(sim.interner.resolve(yard.type_ref()), "GACNST");
        let yard_id = yard.stable_id();
        assert!(yard.building_art.actually_placed(), "the yard opened");
        assert_eq!(
            yard.mission.current(),
            MissionId::from_known(MissionType::Construction)
        );
        assert_eq!(
            yard.mission.queued(),
            MissionId::from_known(MissionType::Guard)
        );
        let house = &sim.houses[&owner];
        // The decrement moves only the yard: its Unlimbo republishes the
        // primary centre (House 4FD150), which leaves none here, and
        // 1001ED60 restores the start cell it saved before the callback.
        assert_eq!(house.base_center(), Some((44, 48)));
        assert!(sim.substrate.occupancy.contains_entity(43, 47, yard_id));
        assert!(!sim.substrate.occupancy.contains_entity(47, 47, yard_id));
        if !human {
            let latches = house.ai_activation();
            assert!(latches.production && latches.ai_triggers_active);
            assert!(latches.auto_base_building);
            assert_eq!(house.base_plan_center(), (43, 47));
        }
    }
}

#[test]
fn registered_nonstock_country_reaches_house_without_stock_remapping() {
    let mut ini = String::from(CONTROLLED_STOCK_COUNTRIES);
    // A second physical Countries section is not a registry extension;
    // append the new ID inside the original master list instead.
    ini = ini.replace("[Sides]", "10=Allocated\n[Sides]");
    let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
    let mut session = one_player_battle_launch("allocated.map").session().clone();
    session.local_slot_mut().country = LaunchCountry::from_country_index(10);
    let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
    let mut sim = Simulation::new();
    initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &launch);
    let local = sim.session.current_house().unwrap();
    let house = &sim.houses[&local];
    assert_eq!(sim.interner.resolve(house.country().unwrap()), "Allocated");
    assert_eq!(
        house.side_index(),
        -1,
        "Country ctor's absent Side remains native-1"
    );
}

#[test]
fn multiple_humans_including_observer_apply_layered_comp_easy_bonus_before_house_biases() {
    let mut session = one_player_battle_launch("bonus.map").session().clone();
    let mut observer = session.humans[0].clone();
    observer.name = "Observer".into();
    observer.raw_slot_index = Some(1);
    observer.set_observer(true);
    observer.slot.color_index = 8;
    session.humans.push(observer);
    session
        .opponents
        .push(crate::skirmish_launch::SkirmishAiSlot {
            raw_slot_index: 2,
            country: LaunchCountry::Russia,
            country_random: false,
            color_index: 1,
            color_random: false,
            start_position: LaunchStartPosition::Auto,
            team: LaunchTeam::None,
            difficulty: crate::skirmish_launch::AiDifficulty::Normal,
        });

    let descriptor = MatchLaunchDescriptor::from_resolved(session).unwrap();
    for (setting, expected) in [
        ("", HouseDifficulty::Hard),
        ("[AI]\nCompEasyBonus=no\n", HouseDifficulty::Normal),
    ] {
        let rules = RuleSet::from_ini(&launch_fixture_ini(setting)).unwrap();
        let mut sim = Simulation::new();
        initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &descriptor);
        let ai = sim.interner.get("Computer1").unwrap();
        assert_eq!(sim.houses[&ai].difficulty(), expected);
    }
}

#[test]
fn observer_start_views_match_executed_native_chooser_and_rng() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/storage_oracle/lan_lobby.json",
    ))
    .unwrap();
    for case in corpus["observer_starts"].as_array().unwrap() {
        let mut session = one_player_battle_launch("observer.map").session().clone();
        let template = session.humans[0].clone();
        session.humans = case["observers"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, observer)| {
                let mut human = template.clone();
                human.name = format!("Human{index}");
                human.raw_slot_index = Some(index as u8);
                human.set_observer(observer.as_bool().unwrap());
                human.slot.color_index = if human.is_observer() { 8 } else { index as u8 };
                human.slot.start_position = case["initial_table"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .position(|owner| owner.as_i64() == Some(index as i64))
                    .map_or(LaunchStartPosition::Auto, |start| {
                        LaunchStartPosition::Position(start as u8)
                    });
                human
            })
            .collect();

        let starts: Vec<_> = case["starts"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, cell)| Waypoint {
                index: index as u32,
                rx: cell[0].as_u64().unwrap() as u16,
                ry: cell[1].as_u64().unwrap() as u16,
            })
            .collect();
        let mut rng =
            SimRng::from_native_state_hex_for_test(case["initial_rng_hex"].as_str().unwrap());
        let result = native_assign_launch_starts(&session, &starts, &mut rng);
        assert_eq!(
            result
                .start_table
                .iter()
                .map(|owner| owner.map_or(-1, |owner| owner as i64))
                .collect::<Vec<_>>(),
            case["table"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_i64().unwrap())
                .collect::<Vec<_>>()
        );
        for (index, position) in &result.placements {
            assert_eq!(
                [u64::from(position.rx), u64::from(position.ry)],
                [
                    case["placements"][*index][0].as_u64().unwrap(),
                    case["placements"][*index][1].as_u64().unwrap()
                ]
            );
        }
        assert_eq!(
            rng.native_state_hex(),
            case["rng_hex"].as_str().unwrap(),
            "{}",
            case["name"]
        );
    }
}

/// VERA's room where only AI play: every observer comes before them, and
/// a second one leaves the first one's reserved start to the first AI,
/// so on a map with a start for each AI every AI gets its own.
#[test]
fn observers_ahead_of_only_ai_leave_each_ai_a_start() {
    let mut session = one_player_battle_launch("watch.map").session().clone();
    let template = session.humans[0].clone();
    session.humans = (0..2u8)
        .map(|row| {
            let mut human = template.clone();
            human.name = format!("Watcher{row}");
            human.raw_slot_index = Some(row);
            human.set_observer(true);
            human.slot.color_index = 8;
            human
        })
        .collect();
    session.opponents = (2..4u8)
        .map(|row| crate::skirmish_launch::SkirmishAiSlot {
            raw_slot_index: row,
            country: LaunchCountry::Russia,
            country_random: false,
            color_index: row,
            color_random: false,
            start_position: LaunchStartPosition::Auto,
            team: LaunchTeam::None,
            difficulty: crate::skirmish_launch::AiDifficulty::Normal,
        })
        .collect();
    session.ai_player_count = 2;

    let starts = [(20, 20), (60, 60)].map(|(rx, ry)| Waypoint { index: 0, rx, ry });
    let result = native_assign_launch_starts(&session, &starts, &mut SimRng::new(7));
    let slots = normalized_launch_slots(&session);
    let ai_starts: Vec<_> = result
        .placements
        .iter()
        .filter(|(slot, _)| !slots[*slot].observer)
        .map(|(_, start)| (start.rx, start.ry))
        .collect();
    assert_eq!(ai_starts.len(), 2);
    assert_ne!(ai_starts[0], ai_starts[1]);
}

fn network_hash_perspective(local_player: usize) -> (crate::sim::runtime::SimRuntime, [u64; 2]) {
    network_hash_perspective_with_survivor(local_player, false)
}

fn network_hash_perspective_with_survivor(
    local_player: usize,
    third_human: bool,
) -> (crate::sim::runtime::SimRuntime, [u64; 2]) {
    network_vision_perspective(local_player, 2 + usize::from(third_human), None, "")
}

fn network_vision_perspective(
    local_player: usize,
    human_count: usize,
    observer: Option<usize>,
    extra_rules: &str,
) -> (crate::sim::runtime::SimRuntime, [u64; 2]) {
    use crate::sim::runtime::{SimResources, SimRuntime};
    use crate::sim::scenario_session::ScenarioGameMode;
    let mut ini = launch_fixture_ini(
        "[Clear]\nBuildable=yes\n[CombatDamage]\nC4Warhead=Super\n[Warheads]\n0=Super\n[Super]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\nSpeed=6\nSight=4\nOwner=Americans\nMovementZone=Normal\nSpeedType=Track\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    );
    ini.merge(&IniFile::from_str(extra_rules));
    let rules = RuleSet::from_ini(&ini).unwrap();
    let mut session = one_player_battle_launch("shared-world.map")
        .session()
        .clone();
    let template = session.humans[0].clone();
    session.humans = (0..human_count)
        .map(|index| {
            let mut human = template.clone();
            human.name = match index {
                0 => "Host".into(),
                1 => "Guest".into(),
                2 => "Third".into(),
                _ => format!("Player{index:02}"),
            };
            human.raw_slot_index = Some(index as u8);
            human.set_observer(observer == Some(index));
            human.slot.color_index = if human.is_observer() {
                8
            } else {
                (index * 8 / human_count.max(8)) as u8
            };
            human
        })
        .collect();
    session.local_player_index = local_player;
    session.options.short_game = false;
    session.options.bases = false;
    session.options.unit_count = 0;

    let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
    let mut sim = Simulation::with_seed(73);
    sim.session.set_game_mode(ScenarioGameMode::Wol);
    sim.session.install_network_timing(10);
    initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &launch);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.fog = crate::sim::vision::FogState::new(32, 32);
    let actors = [
        sim.spawn_object("MTNK", "Host", 8, 8, 0, &rules).unwrap(),
        sim.spawn_object("MTNK", "Guest", 24, 24, 0, &rules)
            .unwrap(),
    ];
    for (index, human) in launch.session().humans.iter().enumerate().skip(2) {
        if !human.is_observer() {
            sim.spawn_object(
                "MTNK",
                &human.name,
                8 + ((index - 2) % 8) as u16 * 2,
                24 - ((index - 2) / 8) as u16 * 2,
                0,
                &rules,
            )
            .unwrap();
        }
    }
    sim.finalize_scenario_post_map(crate::sim::scenario_post_map::ScenarioPostMapInput {
        map_width: 32,
        map_height: 32,
        fill_silos: false,
        rules: &rules,
        overlay_registry: rules.overlay_registry(),
        house_roster: &HouseRoster::default(),
        construction_rules: sim.session.rule_policy(),
        skirmish_session: Some(&launch),
        campaign_map: None,
        stage_home_global: None,
    })
    .unwrap();
    sim.hash_state_every(std::num::NonZeroU64::MIN);
    (
        SimRuntime {
            simulation: sim,
            resources: std::sync::Arc::new(SimResources::new(rules)),
        },
        actors,
    )
}

#[test]
fn scripted_result_network_views_keep_both_humans_and_local_exit_consistent() {
    use crate::rules::rule_policy::RulePolicy;
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    use crate::sim::house_state::HouseOutcomeKind;
    let mut finals = Vec::new();
    for local in [0, 1] {
        let (mut runtime, actors) = network_hash_perspective(local);
        let sim = &mut runtime.simulation;
        let rules = &runtime.resources.rules;
        let owner = sim.entities().get(actors[0]).unwrap().owner();
        let ini = IniFile::from_str(
            "[TeamTypes]\n0=RESULT-TEAM\n[RESULT-TEAM]\n\
            House=Americans\nScript=RESULT-SCRIPT\nTaskForce=RESULT-FORCE\nGroup=-1\nRecruiter=yes\nMax=1\n\
            [ScriptTypes]\n0=RESULT-SCRIPT\n[RESULT-SCRIPT]\n0=23,77\n\
            [TaskForces]\n0=RESULT-FORCE\n[RESULT-FORCE]\nGroup=-1\n0=1,MTNK\n",
        );
        let registry = TeamAiIniRegistry::from_sources(
            &IniFile::empty(),
            &ini,
            RulePolicy::stock_multiplayer(),
        );
        sim.install_team_ai_registry(&registry, rules);
        let ty = sim.interner.get("RESULT-TEAM").unwrap();
        let team = sim.create_team(ty, Some(owner), rules).unwrap();
        let player = sim.session.current_house().unwrap();
        let mut executed = false;
        for _ in 0..120 {
            runtime.advance_frame_for_tooling(&[], 22).unwrap();
            let sim = &runtime.simulation;
            if let Some(team) = sim.team_script_vm.team(team)
                && team.cursor() == 0
            {
                assert!(team.refusal().is_none(), "{:?}", team.refusal());
                assert!(team.advance_pending());
                assert_eq!(team.members().collect::<Vec<_>>(), [actors[0]]);
                executed = true;
            }
            if sim.ready_outcome_for_owner(player).is_some() {
                break;
            }
        }
        let sim = &runtime.simulation;
        assert!(executed, "ordinary Team AI reaches the loss action");
        assert!(sim.houses[&owner].has_lost());
        let guest = sim.entities().get(actors[1]).unwrap().owner();
        assert!(sim.houses[&guest].has_lost());
        assert_eq!(
            sim.ready_outcome_for_owner(player).unwrap().kind,
            HouseOutcomeKind::Defeat
        );
        assert!(
            sim.ready_outcome_for_owner(if player == owner { guest } else { owner })
                .is_none()
        );
        assert!(sim.team_script_vm.team(team).is_none());
        finals.push(sim.state_hash());
    }
    assert_eq!(
        finals[0], finals[1],
        "local identity does not select shared House flags"
    );
}

#[test]
fn network_local_perspectives_share_startup_fog_and_ordinary_frame_hashes() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::world::{FrameEffects, TickLane};
    let (mut left, actors) = network_hash_perspective(0);
    let (mut right, other_actors) = network_hash_perspective(1);
    assert_eq!(actors, other_actors);
    assert_ne!(
        left.simulation.session.current_house(),
        right.simulation.session.current_house()
    );
    assert_eq!(
        left.simulation.fog.plane_digests(),
        right.simulation.fog.plane_digests()
    );
    assert_eq!(
        left.simulation.state_hash(),
        right.simulation.state_hash(),
        "fresh accepted world"
    );
    let start = left.simulation.entities().get(actors[0]).unwrap().position;
    let start = (start.rx(), start.ry());
    for frame in 0..48 {
        // These are ordinary local frame-ingress inputs, intentionally
        // different on the two clients, not synchronized player orders.
        if frame == 2 {
            for (runtime, actor) in [(&mut left, actors[0]), (&mut right, actors[1])] {
                let owner = runtime.simulation.session.current_house().unwrap();
                assert!(runtime.simulation.issue_local_command(CommandEnvelope::new(
                    owner,
                    runtime.simulation.session.tick(),
                    Command::Select {
                        entity_ids: vec![actor],
                        additive: false
                    },
                )));
            }
        }
        if frame == 3 {
            let owner = left.simulation.interner.get("Host").unwrap();
            let record = left
                .simulation
                .encode_megamission_record(
                    owner,
                    &Command::Move {
                        entity_id: actors[0],
                        target_rx: 12,
                        target_ry: 8,
                    },
                )
                .unwrap();
            assert!(left.simulation.admit_native_command_record(record.clone()));
            assert!(right.simulation.admit_native_command_record(record));
        }
        assert!(left.simulation.prepare_fog_view_for("Host"));
        assert!(right.simulation.prepare_fog_view_for("Guest"));
        assert!(right.simulation.prepare_fog_view_for("Guest"));
        let a = left
            .advance_frame(&[], 33, TickLane::Ordinary, &mut FrameEffects::empty())
            .unwrap();
        let b = right
            .advance_frame(&[], 33, TickLane::Ordinary, &mut FrameEffects::empty())
            .unwrap();
        assert!(a.tick.frame_committed && b.tick.frame_committed);
        assert_eq!(
            a.tick.state_hash, b.tick.state_hash,
            "ordinary frame {frame}"
        );
        assert_eq!(
            left.simulation.fog.plane_digests(),
            right.simulation.fog.plane_digests(),
            "all human views at frame {frame}"
        );
    }
    let final_position = left.simulation.entities().get(actors[0]).unwrap().position;
    assert_ne!(
        (final_position.rx(), final_position.ry()),
        start,
        "real admitted Move must execute"
    );
}

#[test]
fn network_zero_sight_discovery_uses_each_human_admission_and_retains_shared_hash() {
    use crate::sim::world::UninitContext;
    let extra = "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\nSight=0\nOwner=Americans\nSpeed=4\nMovementZone=Normal\nSpeedType=Foot\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n";
    let (mut left, _) = network_vision_perspective(0, 2, None, extra);
    let (mut right, _) = network_vision_perspective(1, 2, None, extra);
    let host = left.simulation.interner.get("Host").unwrap();
    let guest = left.simulation.interner.get("Guest").unwrap();
    let mut ids = Vec::new();
    for runtime in [&mut left, &mut right] {
        let id = runtime
            .simulation
            .spawn_object("E1", "Guest", 16, 16, 0, &runtime.resources.rules)
            .unwrap();
        assert!(!runtime.simulation.techno_discovered_by_viewer(id, host));
        assert!(!runtime.simulation.techno_discovered_by_viewer(id, guest));
        ids.push(id);
    }
    assert_eq!(ids[0], ids[1]);
    assert_eq!(left.simulation.state_hash(), right.simulation.state_hash());
    let unseen = left.simulation.state_hash();
    for viewer in [host, guest] {
        for runtime in [&mut left, &mut right] {
            let mut context = UninitContext::new(&runtime.resources.rules);
            assert!(
                runtime
                    .simulation
                    .discover_techno(ids[0], viewer, context.reborrow())
            );
            assert!(!runtime.simulation.discover_techno(ids[0], viewer, context));
            assert!(
                runtime
                    .simulation
                    .techno_discovered_by_viewer(ids[0], viewer)
            );
        }
        assert_eq!(left.simulation.state_hash(), right.simulation.state_hash());
    }
    assert_ne!(
        left.simulation.state_hash(),
        unseen,
        "retained shared discovery is hashed"
    );
    assert!(left.simulation.houses[&guest].discovered_by_current_house());
    assert!(right.simulation.houses[&guest].discovered_by_current_house());
}

#[test]
fn network_high_discovery_bits_survive_observer_and_early_player_removal() {
    use crate::sim::world::{FrameEffects, TickLane, UninitContext};
    let extra = "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\nSight=0\nOwner=Americans\nSpeed=4\nMovementZone=Normal\nSpeedType=Foot\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n";
    let (mut left, actors) = network_vision_perspective(0, 30, Some(28), extra);
    let (mut right, other_actors) = network_vision_perspective(29, 30, Some(28), extra);
    assert_eq!(actors, other_actors);
    let last = right.simulation.session.current_house().unwrap();
    let observer = left.simulation.interner.get("Player28").unwrap();
    let early = left.simulation.interner.get("Guest").unwrap();
    let host = left.simulation.session.current_house().unwrap();
    let viewers: Vec<_> = left
        .simulation
        .houses
        .in_order()
        .filter(|house| house.is_human())
        .map(|house| house.name())
        .collect();
    // Test precondition: native color sorting puts the observer last.
    // These distinct logical viewers exercise both compact high bits.
    assert_eq!(
        (viewers.len(), viewers[28], viewers[29]),
        (30, last, observer)
    );
    assert_eq!(left.simulation.session.observer_houses(), [observer]);
    let ids = [&mut left, &mut right].map(|runtime| {
        runtime
            .simulation
            .spawn_object("E1", "Host", 16, 8, 0, &runtime.resources.rules)
            .unwrap()
    });
    assert_eq!(ids[0], ids[1]);
    assert_eq!(left.simulation.state_hash(), right.simulation.state_hash());
    for viewer in [last, observer] {
        let before = left.simulation.state_hash();
        for runtime in [&mut left, &mut right] {
            let mut context = UninitContext::new(&runtime.resources.rules);
            let sim = &mut runtime.simulation;
            assert!(!sim.techno_discovered_by_viewer(ids[0], viewer));
            assert!(sim.discover_techno(ids[0], viewer, context.reborrow()));
            assert!(sim.techno_discovered_by_viewer(ids[0], viewer));
            if viewer == last {
                assert!(!sim.techno_discovered_by_viewer(ids[0], observer));
            }
            let retained = (sim.state_hash(), sim.rng_state());
            assert!(!sim.discover_techno(ids[0], viewer, context));
            assert_eq!((sim.state_hash(), sim.rng_state()), retained);
        }
        assert_ne!(left.simulation.state_hash(), before);
        assert_eq!(left.simulation.state_hash(), right.simulation.state_hash());
    }
    // Real EventClass REMOVEPLAYER4C7B73/Flag_To_Die4C7BE1, not a
    // direct defeated flag or mutation of the immutable House registry.
    let early_index = left
        .simulation
        .houses
        .order()
        .iter()
        .position(|&owner| owner == early)
        .unwrap() as i32;
    let record = left
        .simulation
        .encode_remove_player_record(host, early_index)
        .unwrap();
    assert!(left.simulation.admit_native_command_record(record.clone()));
    assert!(right.simulation.admit_native_command_record(record));
    for _ in 0..6 {
        let a = left
            .advance_frame(&[], 33, TickLane::Ordinary, &mut FrameEffects::empty())
            .unwrap();
        let b = right
            .advance_frame(&[], 33, TickLane::Ordinary, &mut FrameEffects::empty())
            .unwrap();
        assert!(a.tick.frame_committed && b.tick.frame_committed);
        assert_eq!(a.tick.state_hash, b.tick.state_hash);
    }
    for runtime in [&mut left, &mut right] {
        let sim = &mut runtime.simulation;
        assert!(sim.houses[&early].is_defeated());
        assert!(
            !sim.entities()
                .get(actors[1])
                .is_some_and(|entity| entity.lifecycle.object_alive())
        );
        for viewer in [last, observer] {
            assert!(sim.techno_discovered_by_viewer(ids[0], viewer));
            let retained = (sim.state_hash(), sim.rng_state());
            assert!(!sim.discover_techno(
                ids[0],
                viewer,
                UninitContext::new(&runtime.resources.rules)
            ));
            assert_eq!((sim.state_hash(), sim.rng_state()), retained);
        }
    }
    assert_eq!(left.simulation.state_hash(), right.simulation.state_hash());
    // Simulation deserialization intentionally restarts Scenario RNG.
    // Compare this retained owner separately from the live-world hashes
    // above, which include every shared state/RNG change during cleanup.
    let saved = crate::util::save_codec::serialize(&left.simulation).unwrap();
    let restored: Simulation = crate::util::save_codec::deserialize(&saved).unwrap();
    assert_eq!(
        restored.entities().get(ids[0]).unwrap().seldom.discovery,
        left.simulation
            .entities()
            .get(ids[0])
            .unwrap()
            .seldom
            .discovery,
    );
    for viewer in [last, observer] {
        assert!(restored.techno_discovered_by_viewer(ids[0], viewer));
    }
}

#[test]
fn network_trigger_and_reveal_to_all_lifecycle_update_both_logical_maps() {
    use crate::sim::world::{FrameEffects, TickLane, UninitContext};
    let extra = "[General]\nRevealTriggerRadius=3\n[BuildingTypes]\n0=CAMERA\n[CAMERA]\nStrength=100\nSight=3\nRevealToAll=yes\nOwner=Americans\nFoundation=1x1\n";
    let (mut left, _) = network_vision_perspective(0, 2, None, extra);
    let (mut right, _) = network_vision_perspective(1, 2, None, extra);
    let host = left.simulation.interner.get("Host").unwrap();
    let guest = left.simulation.interner.get("Guest").unwrap();
    let mut ids = Vec::new();
    for runtime in [&mut left, &mut right] {
        let sim = &mut runtime.simulation;
        sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
            theater: "TEMPERATE".into(),
            fill: "Clear".into(),
            level: 0,
            width: 16,
            height: 16,
            local_left: 0,
            local_top: 0,
            local_width: 16,
            local_height: 16,
        });
        sim.session.install_waypoints(
            &[(
                7,
                crate::map::waypoints::Waypoint {
                    index: 7,
                    rx: 16,
                    ry: 16,
                },
            )]
            .into(),
        );
        let rules = &runtime.resources.rules;
        assert!(!sim.fog.is_cell_revealed(host, 16, 16));
        assert!(!sim.fog.is_cell_revealed(guest, 16, 16));
        assert!(sim.trigger_reveal_at_waypoint(7, UninitContext::new(rules)));
        assert!(sim.fog.is_cell_revealed(host, 16, 16));
        assert!(sim.fog.is_cell_revealed(guest, 16, 16));
        assert!(sim.trigger_conceal_at_waypoint(7, UninitContext::new(rules)));
        let id = sim
            .spawn_object("CAMERA", "Host", 16, 16, 0, rules)
            .unwrap();
        let camera = sim.entities().get(id).unwrap();
        assert!(rules.object("CAMERA").unwrap().reveal_to_all);
        assert_eq!(camera.seldom.sight.range(), 3);
        assert!(camera.in_playfield());
        assert!(camera.building_art.actually_placed());
        assert!(sim.fog.sight_is_latched(id, host));
        assert!(sim.techno_discovered_by_viewer(id, guest));
        assert!(sim.houses[&guest].initial_sight_refresh_pending());
        // CellPUT discovery can open this Human building before Unlimbo
        // establishes3D5 (6F6CFE). Native UpdateReveal70AF58..60 rejects
        // that early alternate call. House4F9338..935A subsequently
        // consumes24B and visits Map4ADCD0's RTA arm in ordinary Logic.
        assert!(!sim.fog.holds_alternate_sight(id, guest));
        ids.push(id);
    }
    assert_eq!(ids[0], ids[1]);
    for runtime in [&mut left, &mut right] {
        let frame = runtime
            .advance_frame(&[], 33, TickLane::Ordinary, &mut FrameEffects::empty())
            .unwrap();
        assert!(frame.tick.frame_committed);
        let sim = &mut runtime.simulation;
        let rules = &runtime.resources.rules;
        let id = ids[0];
        assert!(sim.fog.holds_alternate_sight(id, guest));
        assert_eq!(
            sim.fog.by_owner()[&guest].shroud_knowledge_raw()[16 * 32 + 16].counter(),
            -1,
            "one represented RTA visitor must add one alternate sight contribution"
        );
        assert!(!sim.fog.holds_alternate_sight(id, host));
        assert!(sim.fog.is_cell_visible(host, 16, 16));
        assert!(sim.fog.is_cell_visible(guest, 16, 16));
        // Native Limbo6F6B06 releases every represented PlayerPtr,
        // including the owner (DestructionEffects44172F excludes it).
        assert_eq!(
            sim.techno_limbo_with_context(id, UninitContext::new(rules)),
            crate::sim::world::ConcealOutcome::Concealed
        );
        assert!(!sim.fog.holds_alternate_sight(id, guest));
    }

    assert_eq!(
        left.simulation.fog.plane_digests(),
        right.simulation.fog.plane_digests()
    );
    assert_eq!(left.simulation.state_hash(), right.simulation.state_hash());
}

#[test]
fn network_two_human_destruct_keeps_full_frame_and_terminal_score_hashes_equal() {
    use crate::sim::house_state::HouseOutcomeKind;
    use crate::sim::world::{FrameEffects, TickLane};
    let (mut left, _) = network_hash_perspective(0);
    let (mut right, _) = network_hash_perspective(1);
    let host = left.simulation.interner.get("Host").unwrap();
    let guest = left.simulation.interner.get("Guest").unwrap();
    let mut finished = false;
    for frame in 0..600 {
        if frame == 30 {
            let record = left.simulation.encode_destruct_record(guest).unwrap();
            assert!(left.simulation.admit_native_command_record(record.clone()));
            assert!(right.simulation.admit_native_command_record(record));
        }
        let a = left
            .advance_frame(&[], 33, TickLane::Ordinary, &mut FrameEffects::empty())
            .unwrap();
        let b = right
            .advance_frame(&[], 33, TickLane::Ordinary, &mut FrameEffects::empty())
            .unwrap();
        assert_eq!(a.tick.tick, b.tick.tick, "frame {frame}");
        assert_eq!(
            a.tick.frame_committed, b.tick.frame_committed,
            "frame {frame}"
        );
        assert_eq!(
            a.tick.state_hash,
            b.tick.state_hash,
            "frame {frame}: {:?} / {:?}",
            left.simulation.state_hash_owners(),
            right.simulation.state_hash_owners()
        );
        assert_eq!(
            left.simulation.fog.plane_digests(),
            right.simulation.fog.plane_digests(),
            "frame {frame}"
        );
        if left.simulation.terminal_score_snapshot().is_some()
            || right.simulation.terminal_score_snapshot().is_some()
        {
            assert_eq!(
                left.simulation.terminal_score_snapshot(),
                right.simulation.terminal_score_snapshot()
            );
            assert_eq!(left.simulation.state_hash(), right.simulation.state_hash());
            assert_eq!(
                left.simulation.ready_outcome_for_owner(host).unwrap().kind,
                HouseOutcomeKind::Victory
            );
            assert_eq!(
                right
                    .simulation
                    .ready_outcome_for_owner(guest)
                    .unwrap()
                    .kind,
                HouseOutcomeKind::Defeat
            );
            finished = true;
            break;
        }
    }
    assert!(
        finished,
        "real DESTRUCT must reach the committed terminal score frame"
    );
}

#[test]
fn network_defeated_viewer_and_storm_keep_shared_maps_and_radar() {
    use crate::sim::power_system::has_active_radar;
    use crate::sim::world::{FrameEffects, TickLane};
    let (mut left, _) = network_hash_perspective_with_survivor(0, true);
    let (mut right, _) = network_hash_perspective_with_survivor(1, true);
    let guest = left.simulation.interner.get("Guest").unwrap();
    let host = left.simulation.interner.get("Host").unwrap();
    let record = left.simulation.encode_destruct_record(guest).unwrap();
    assert!(left.simulation.admit_native_command_record(record.clone()));
    assert!(right.simulation.admit_native_command_record(record));
    for _ in 0..6 {
        for runtime in [&mut left, &mut right] {
            runtime
                .advance_frame(&[], 33, TickLane::Ordinary, &mut FrameEffects::empty())
                .unwrap();
        }
        assert_eq!(
            left.simulation.state_hash(),
            right.simulation.state_hash(),
            "defeat projection: {:?} versus {:?}",
            left.simulation.hash_owner_diagnostics_for_test(),
            right.simulation.hash_owner_diagnostics_for_test()
        );
    }
    for runtime in [&mut left, &mut right] {
        let sim = &runtime.simulation;
        assert!(sim.houses[&guest].is_defeated());
        assert!(!sim.houses[&host].is_defeated());
        assert!(sim.fog.whole_map_revealed_for_owner(guest));
        assert!(has_active_radar(sim.power_states.as_map(), guest));
        assert!(
            sim.session.outcome_request().is_none(),
            "defeated viewer watches hostile survivors"
        );
    }
    assert_eq!(
        left.simulation.fog.plane_digests(),
        right.simulation.fog.plane_digests()
    );
    for runtime in [&mut left, &mut right] {
        crate::sim::superweapon::lightning_storm::start(
            &mut runtime.simulation,
            &runtime.resources.rules,
            60,
            0,
            (16, 16),
            None,
        );
        let sim = &runtime.simulation;
        assert_eq!(
            sim.power_states[&host]
                .radar_outage_for_test()
                .remaining(sim.session.binary_frame() as i32),
            60
        );
        assert_eq!(
            sim.power_states[&guest]
                .radar_outage_for_test()
                .remaining(sim.session.binary_frame() as i32),
            0
        );
    }
    assert_eq!(left.simulation.state_hash(), right.simulation.state_hash());
    for _ in 0..2 {
        for runtime in [&mut left, &mut right] {
            runtime
                .advance_frame(&[], 33, TickLane::Ordinary, &mut FrameEffects::empty())
                .unwrap();
        }
        assert_eq!(left.simulation.state_hash(), right.simulation.state_hash());
        assert!(has_active_radar(
            left.simulation.power_states.as_map(),
            guest
        ));
        assert!(has_active_radar(
            right.simulation.power_states.as_map(),
            guest
        ));
    }
}

#[test]
fn network_hash_omits_local_control_but_preserves_sight_fog_credits_and_timer() {
    let (mut left, _) = network_hash_perspective(0);
    let (mut right, _) = network_hash_perspective(1);
    let host = left.simulation.interner.get("Host").unwrap();
    let guest = left.simulation.interner.get("Guest").unwrap();
    // PlayerControl is this client's input/EVA selector. Initial sight
    // invalidation drives shared fog work and must still be detectable.
    left.simulation
        .houses
        .get_mut(&guest)
        .unwrap()
        .assign_player_control(true);
    assert_eq!(left.simulation.state_hash(), right.simulation.state_hash());
    assert!(
        left.simulation
            .houses
            .get_mut(&host)
            .unwrap()
            .take_initial_sight_refresh()
    );
    assert_ne!(left.simulation.state_hash(), right.simulation.state_hash());
    assert!(
        right
            .simulation
            .houses
            .get_mut(&host)
            .unwrap()
            .take_initial_sight_refresh()
    );
    assert_eq!(left.simulation.state_hash(), right.simulation.state_hash());
    left.simulation
        .houses
        .get_mut(&host)
        .unwrap()
        .economy_mut()
        .add_credits(1);
    assert_ne!(left.simulation.state_hash(), right.simulation.state_hash());
    right
        .simulation
        .houses
        .get_mut(&host)
        .unwrap()
        .economy_mut()
        .add_credits(1);
    assert_eq!(left.simulation.state_hash(), right.simulation.state_hash());
    let fog_before = left.simulation.fog.plane_digests();
    left.simulation
        .reveal_map_for_owner(host, &left.resources.rules);
    assert_ne!(left.simulation.fog.plane_digests(), fog_before);
    assert_ne!(left.simulation.state_hash(), right.simulation.state_hash());
    right
        .simulation
        .reveal_map_for_owner(host, &right.resources.rules);
    assert_eq!(left.simulation.state_hash(), right.simulation.state_hash());
    // AllowWin Tag destruction owns the same +298 timer as ToDie. Its
    // shared anchor remains detectable even with identical result flags.
    left.simulation
        .houses
        .get_mut(&host)
        .unwrap()
        .removed_allow_win_tag(0);
    right
        .simulation
        .houses
        .get_mut(&host)
        .unwrap()
        .removed_allow_win_tag(1);
    assert_ne!(left.simulation.state_hash(), right.simulation.state_hash());
}

#[test]
fn thirty_human_local_first_lan_nodes_share_the_same_constructed_house_order() {
    let mut host = one_player_battle_launch("thirty-lan.map").session().clone();
    let template = host.humans[0].clone();
    host.humans = (0..30)
        .map(|row| {
            let mut human = template.clone();
            human.raw_slot_index = Some(row);
            human.name = format!("Player{row:02}");
            human.slot.color_index = row % 8;
            human
        })
        .collect();

    let mut guest = host.clone();
    guest.humans.rotate_left(8);

    assert_eq!(host.local_player().name, "Player00");
    assert_eq!(guest.local_player().name, "Player08");
    assert_eq!(host.pre_fill_house_roster(), guest.pre_fill_house_roster());
    let host_order: Vec<_> = normalized_launch_slots(&host)
        .into_iter()
        .map(|slot| slot.owner_name)
        .collect();
    let guest_order: Vec<_> = normalized_launch_slots(&guest)
        .into_iter()
        .map(|slot| slot.owner_name)
        .collect();
    assert_eq!(host_order, guest_order);
    let rules = techno_constructor_start_rules();
    let worlds: Vec<_> = [host, guest]
        .into_iter()
        .map(|session| {
            let descriptor = MatchLaunchDescriptor::from_resolved(session).unwrap();
            let mut sim = Simulation::with_seed(73);
            sim.session
                .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Lan);
            initialize_skirmish_launch_houses(
                &mut sim,
                &HouseRoster::default(),
                &rules,
                &descriptor,
            );
            sim
        })
        .collect();
    assert_eq!(worlds[0].houses.len(), 32);
    for name in host_order {
        assert_eq!(worlds[0].interner.get(&name), worlds[1].interner.get(&name));
    }
    assert_ne!(
        worlds[0].session.current_house(),
        worlds[1].session.current_house()
    );
    assert_eq!(worlds[0].state_hash(), worlds[1].state_hash());
}

#[test]
fn ordered_humans_keep_local_identity_after_color_sort_and_observer_viewpoint() {
    let mut session = one_player_battle_launch("multiplayer.map")
        .session()
        .clone();
    session.humans[0].name = "Host".into();
    session.humans[0].slot.color_index = 6;
    session.humans[0].slot.start_position = LaunchStartPosition::Position(0);
    let mut local = session.humans[0].clone();
    local.name = "Guest".into();
    local.raw_slot_index = Some(1);
    local.slot.color_index = 1;
    local.slot.start_position = LaunchStartPosition::Position(1);
    session.humans.push(local);
    let mut observer = session.humans[0].clone();
    observer.name = "Observer".into();
    observer.raw_slot_index = Some(2);
    observer.set_observer(true);
    observer.slot.color_index = 8;
    observer.slot.start_position = LaunchStartPosition::Auto;
    session.humans.push(observer);
    session.local_player_index = 1;

    assert!(session.house_roster_is_consistent());
    assert_eq!(session.required_start_count(), 2);
    assert_eq!(session.pre_fill_house_roster().created_house_count(), 5);
    let descriptor = MatchLaunchDescriptor::from_resolved(session.clone()).unwrap();
    let rules = techno_constructor_start_rules();
    let mut sim = Simulation::new();
    initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &descriptor);
    let guest = sim.interner.get("Guest").unwrap();
    let host = sim.interner.get("Host").unwrap();
    assert_eq!(sim.session.current_house(), Some(guest));
    assert_eq!(sim.houses.order()[0], guest);
    assert!(sim.houses[&guest].is_human() && sim.houses[&guest].player_control());
    assert!(sim.houses[&host].is_human() && !sim.houses[&host].player_control());
    let starts = [
        Waypoint {
            index: 0,
            rx: 20,
            ry: 20,
        },
        Waypoint {
            index: 1,
            rx: 40,
            ry: 40,
        },
    ];
    let map = prefix_map_with_starts(&starts);
    let full_prefix =
        prepare_stock_offline_scenario_prefix_plan(&descriptor, &map, &map.waypoints, 73)
            .expect("ordered multi-human launch enters the real pre-Fill prefix");
    assert_eq!(full_prefix.first_house_timers.len(), 5);
    assert_eq!(full_prefix.second_house_timers.len(), 5);
    assert_eq!(full_prefix.projection.assignment.placements.len(), 3);
    let mut rng = SimRng::new(73);
    let before = rng.logical_state();
    let assigned = native_assign_launch_starts(&session, &starts, &mut rng);
    assert_eq!(assigned.start_table, vec![Some(1), Some(0)]);
    assert_eq!(
        assigned.placements,
        vec![(0, starts[1]), (1, starts[0]), (2, starts[0])]
    );
    assert_eq!(
        rng.logical_state(),
        before,
        "observer shares an occupied view without a draw"
    );
    let mut unresolved_remote = session;
    unresolved_remote.humans[0].slot.country_random = true;
    assert!(MatchLaunchDescriptor::from_resolved(unresolved_remote).is_err());
}

/// VERA shared control: players who pick the same start build one House,
/// led by the lowest row, and each sends as its own source.
#[test]
fn humans_on_one_start_share_a_house_as_separate_senders() {
    use crate::sim::control_authority::{ControlGrant, ControlGrantLevel, ControlScope};

    let mut session = one_player_battle_launch("shared.map").session().clone();
    let template = session.humans[0].clone();
    // CreateHouses687F66 color order: Ally, Rival, Host.
    session.humans = [("Host", 3, 0), ("Ally", 1, 0), ("Rival", 2, 1)]
        .into_iter()
        .enumerate()
        .map(|(row, (name, color, start))| {
            let mut human = template.clone();
            human.name = name.into();
            human.raw_slot_index = Some(row as u8);
            human.slot.color_index = color;
            human.slot.start_position = LaunchStartPosition::Position(start);
            human
        })
        .collect();
    session.local_player_index = 1;

    assert_eq!(session.required_start_count(), 2);
    let mut cooperative = session.clone();
    cooperative.mode.class = crate::skirmish_modes::MpModeClass::Cooperative;
    assert_eq!(
        cooperative
            .humans_with_house_leaders()
            .map(|(_, _, leader)| leader)
            .collect::<Vec<_>>(),
        [0, 1, 2],
        "Cooperative shares nothing"
    );

    let grants = [
        ControlGrant {
            source: 0,
            house: 0,
            level: ControlGrantLevel::FullControl,
        },
        ControlGrant {
            source: 2,
            house: 0,
            level: ControlGrantLevel::UnitsOnly,
        },
    ];
    let descriptor = MatchLaunchDescriptor::from_resolved(session)
        .unwrap()
        .with_control_grants(grants.to_vec());
    let roster = HouseRoster::default();
    let rules = techno_constructor_start_rules();
    let mut sim = Simulation::new();
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Lan);
    initialize_skirmish_launch_houses(&mut sim, &roster, &rules, &descriptor);
    let host = sim.interner.get("Host").unwrap();
    let rival = sim.interner.get("Rival").unwrap();
    assert_eq!(sim.houses.len(), 4, "two player Houses, Neutral, Special");
    assert_eq!(sim.houses.order()[..2], [rival, host]);
    // The Ally plays the Host's House.
    assert_eq!(sim.session.current_house(), Some(host));
    assert!(sim.houses[&host].player_control());
    assert_eq!(
        sim.session.network_sources(4).collect::<Vec<_>>(),
        [(0, 1), (1, 0), (2, 1)]
    );
    assert_eq!(sim.session.local_shared_house_source(), Some(0));

    let construction_rules = sim.session.rule_policy();
    crate::sim::runtime::finalize_constructed_scenario(
        &mut sim,
        &prefix_map_with_starts(&[]),
        &rules,
        &roster,
        construction_rules,
        Some(&descriptor),
        None,
    )
    .unwrap();
    assert_eq!(
        sim.control_scope_for_source(0, rival),
        ControlScope::FullControl
    );
    assert_eq!(
        sim.control_scope_for_source(2, rival),
        ControlScope::UnitsOnly
    );
    assert_eq!(
        sim.control_scope_for_source(1, rival),
        ControlScope::OwnFull
    );
}

/// `[0xAC1198]` holds the last observer; a VERA room's every observer
/// House gets its treatment.
#[test]
fn every_observer_house_takes_the_observer_identity() {
    let mut session = one_player_battle_launch("multiplayer.map")
        .session()
        .clone();
    session.humans[0].name = "Host".into();
    let mut guest = session.humans[0].clone();
    guest.name = "Guest".into();
    guest.raw_slot_index = Some(1);
    guest.slot.color_index = 1;
    session.humans.push(guest);
    for (row, name) in [(2, "Observer"), (3, "Spectator")] {
        let mut observer = session.humans[0].clone();
        observer.name = name.into();
        observer.raw_slot_index = Some(row);
        observer.set_observer(true);
        observer.slot.color_index = 8;
        observer.slot.start_position = LaunchStartPosition::Auto;
        session.humans.push(observer);
    }

    let descriptor = MatchLaunchDescriptor::from_resolved(session).unwrap();
    let mut sim = Simulation::new();
    initialize_skirmish_launch_houses(
        &mut sim,
        &HouseRoster::default(),
        &techno_constructor_start_rules(),
        &descriptor,
    );
    let [host, guest, observer, spectator] =
        ["Host", "Guest", "Observer", "Spectator"].map(|name| sim.interner.get(name).unwrap());
    assert_eq!(sim.session.observer_houses(), [observer, spectator]);
    assert!(!sim.session.is_observer_house(host) && !sim.session.is_observer_house(guest));
}

#[test]
fn campaign_current_house_uses_exact_case_and_registry_fallback() {
    let mut sim = Simulation::new();
    // Earlier interning must not become the original House name spelling.
    sim.interner.intern("alpha");
    let map = IniFile::from_str(
        "[Houses]\n7=Zulu\n2=Alpha\n9=1234567890123456789\n[Zulu]\n[Alpha]\n[1234567890123456789]\n",
    );
    let roster = crate::map::houses::parse_house_roster(&map, &[], None);
    initialize_map_roster_houses(
        &mut sim,
        &roster,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        None,
    );
    let first = sim.interner.get("Zulu").unwrap();
    let second = sim.interner.get("Alpha").unwrap();
    let capped = sim.interner.get("1234567890123456789").unwrap();
    for (input, expected) in [
        ("", first),
        ("Player=missing", first),
        ("Player=alpha", first),
        ("Player=Alpha", second),
        ("Player=1234567890123456789suffix", capped),
        // The first 19 bytes end in whitespace that only the post-copy
        // trim removes; trimming the uncut source cannot remove it.
        ("Player=Alpha              suffix", second),
    ] {
        initialize_campaign_current_house(
            &mut sim.session,
            &mut sim.houses,
            &IniFile::from_str(&format!("[Basic]\n{input}\n")),
        );
        assert_eq!(sim.session.current_house(), Some(expected));
        assert!(sim.houses[&expected].is_human());
        assert!(sim.houses[&expected].player_control());
    }
    assert!(
        sim.houses[&first].is_human() && sim.houses[&first].player_control(),
        "selecting another House does not clear previously set flags"
    );
}

#[test]
fn scenario_side_campaign_uses_its_own_basic_player_reader() {
    let rules = techno_constructor_start_rules();
    // ReadHousesList first reads each roster name with capacity20, so
    // authored names retain19 bytes before House's own20-byte copy.
    // A suffix is cut by the current-House reader but retained by this
    // independent32-byte Player reader, whose exact lookup must miss.
    let long_name = "1234567890123456789";
    let map = IniFile::from_str(&format!(
        "[Houses]\n0=First\n1=Americans\n2={long_name}\n[First]\nCountry=YuriCountry\n[Americans]\nCountry=Russians\n[{long_name}]\nCountry=YuriCountry\n"
    ));
    let roster = crate::map::houses::parse_house_roster(&map, &rules.color_schemes, Some(&rules));
    let mut sim = Simulation::new();
    initialize_map_roster_houses(&mut sim, &roster, &rules, None);
    let rng = sim.rng_state();
    for (basic, expected) in [
        (String::new(), 1),
        ("[Basic]\n".to_owned(), 1),
        // Physical INI loading omits empty values, so this also uses
        // the literal Americans default rather than a stored-empty key.
        ("[Basic]\nPlayer=\n".to_owned(), 1),
        ("[Basic]\nPlayer=First\n".to_owned(), 2),
        ("[Basic]\nPlayer=americans\n".to_owned(), 0),
        (format!("[Basic]\nPlayer={long_name}\n"), 2),
        (format!("[Basic]\nPlayer={long_name}suffix\n"), 0),
    ] {
        let ini = IniFile::from_str(&basic);
        initialize_campaign_current_house(&mut sim.session, &mut sim.houses, &ini);
        let current = sim.session.current_house();
        if basic.ends_with("suffix\n") {
            assert_eq!(current, sim.interner.get(long_name));
        }
        initialize_campaign_scenario_side(&mut sim, &ini, &rules);
        assert_eq!(sim.session.scenario_side(), expected, "{basic}");
        assert_eq!(sim.session.current_house(), current);
        assert_eq!(sim.rng_state(), rng);
    }
}

#[test]
fn scenario_side_skirmish_uses_local_source_identity_before_house_color_order() {
    let mut session = one_player_battle_launch("side.map").session().clone();
    let mut second = session.humans[0].clone();
    second.name = "LocalSecond".into();
    second.raw_slot_index = Some(1);
    second.slot.country = LaunchCountry::Russia;
    second.slot.color_index = 1;
    session.humans[0].slot.color_index = 0;
    session.humans.push(second);
    session.local_player_index = 1;

    let descriptor = MatchLaunchDescriptor::from_resolved(session).unwrap();
    let rules = techno_constructor_start_rules();
    let mut sim = Simulation::new();
    initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &descriptor);
    assert_eq!(sim.houses[&sim.houses.order()[0]].side_index(), 0);
    assert_eq!(sim.session.scenario_side(), 1);
    let hash = sim.state_hash();
    let rng = sim.rng_state();
    sim.session
        .install_current_house(Some(sim.houses.order()[0]));
    assert_eq!(sim.session.scenario_side(), 1);
    assert_eq!(sim.state_hash(), hash);
    assert_eq!(sim.rng_state(), rng);
}

/// x87 chop (cw `0x0E7F`) versus f64 round-to-nearest for the AI opening
/// grant: `3 * 0.01` chops just below `0.03`, `* 100` chops just below `3.0`,
/// and `Math__ftol` truncates to 2 where `((3.0 * 0.01) * 100.0) as i32`
/// gives 3. The stock `400,0,0 x 10000` case agrees under both modes.
#[test]
fn native_ai_opening_grant_chops_where_round_to_nearest_differs() {
    assert_eq!(
        ((3.0_f64 * 0.01) * 100.0) as i32,
        3,
        "round-to-nearest baseline"
    );
    assert_eq!(
        native_ai_opening_grant(3, 100),
        2,
        "chop at 53 bits then ftol"
    );
    assert_eq!(native_ai_opening_grant(3, 200), 5);
    assert_eq!(
        native_ai_opening_grant(400, 10_000),
        40_000,
        "stock Hard AI"
    );
    assert_eq!(native_ai_opening_grant(0, 10_000), 0, "stock Normal/Easy");
    assert_eq!(native_ai_opening_grant(7, 12_345), 864);
    assert_eq!(native_ai_opening_grant(400, 0), 0);
    assert_eq!(
        native_ai_opening_grant(-100, 10_000),
        -10_000,
        "negative chops toward zero"
    );
}
use crate::map::resolved_terrain::ResolvedTerrainCell;
use crate::rules::ini_parser::IniFile;

fn descriptor(seed: u32) -> ScenarioDescriptor {
    ScenarioDescriptor {
        seed,
        ..ScenarioDescriptor::default()
    }
}

fn one_player_battle_launch(selected_map_file: &str) -> MatchLaunchDescriptor {
    MatchLaunchDescriptor::from_resolved(crate::headless_scenario::battle_session(
        selected_map_file,
        &[],
    ))
    .expect("one-player Battle fixture is fully resolved")
}

fn prefix_map_with_starts(starts: &[Waypoint]) -> MapFile {
    MapFile {
        header: crate::map::map_file::MapHeader {
            theater: "TEMPERATE".to_string(),
            fill: "Clear".to_string(),
            level: 0,
            width: 44,
            height: 52,
            local_left: 2,
            local_top: 5,
            local_width: 40,
            local_height: 40,
        },
        basic: Default::default(),
        ranking: Default::default(),
        preview: Default::default(),
        cells: Vec::new(),
        iso_map_pack_lookups: Vec::new(),
        entities: Vec::new(),
        overlays: Vec::new(),
        authored_overlay_packs: crate::map::map_file::AuthoredOverlayPackSlot::empty(),
        smudges: Vec::new(),
        terrain_objects: Vec::new(),
        waypoints: starts
            .iter()
            .copied()
            .map(|start| (start.index, start))
            .collect(),
        tags: Default::default(),
        triggers: Default::default(),
        events: Default::default(),
        actions: Default::default(),
        local_variables: Default::default(),
        special_flags: Default::default(),
        ini: IniFile::from_str(""),
    }
}

fn one_start_prefix_map(start: Waypoint) -> MapFile {
    prefix_map_with_starts(&[start])
}

#[test]
fn shell_allocated_country_is_readmitted_against_post_reset_process_rules() {
    use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
    let root = "[Countries]\n0=Alpha\n[Alpha]\nName=Alpha\n[Orphan]\nName=Orphan\n";
    for replacement in [None, Some("Replacement")] {
        let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
            IniFile::from_str(root),
            None,
            IniFile::empty(),
            std::sync::Arc::default(),
            Default::default(),
        )
        .unwrap();
        let index =
            owner.read_multiplayer_country(&IniFile::from_str("[MultiPlayer]\nSide=Orphan\n"), -1);
        assert_eq!(index, 1);
        owner.refresh_multiplayer_country_bodies();
        let mut shell_rules = RuleSet::from_ini(&IniFile::from_str(root)).unwrap();
        owner.project_multiplayer_countries(&mut shell_rules);
        let mut launch = one_player_battle_launch("country-reset.map");
        launch.session.local_slot_mut().country = LaunchCountry::from_country_index(index as u32);
        launch.validate_countries(&shell_rules).unwrap();
        let mut map = one_start_prefix_map(Waypoint {
            index: 0,
            rx: 40,
            ry: 40,
        });
        if let Some(name) = replacement {
            map.ini = IniFile::from_str(&format!("[Countries]\n1={name}\n[{name}]\nName={name}\n"));
        }
        let plan = prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, 1234)
            .unwrap();
        let (rules, _, _, receipt) = owner
            .load_scenario(NativeScenarioRulesPrefix::NonCampaign(None), &map.ini)
            .unwrap()
            .into_parts();
        assert!(rules.country_index("Orphan").is_none());
        let admitted = plan.bind_native_rules_receipt(receipt, &launch, &rules);
        if replacement.is_none() {
            assert!(matches!(
                admitted,
                Err(PreFillScenarioPrefixPlanError::MissingLaunchCountry { index: 1 })
            ));
            continue;
        }
        let (mut sim, _) = ScenarioBootstrapRng::new(1234)
            .into_stock_offline_staged_simulation(
                &crate::sim::scenario_session::ScenarioDescriptor {
                    seed: 1234,
                    ..Default::default()
                },
                admitted.unwrap(),
            )
            .unwrap();
        initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &launch);
        let house = sim
            .houses
            .get(&sim.session.current_house().unwrap())
            .unwrap();
        assert_eq!(
            house.country().map(|id| sim.interner.resolve(id)),
            Some("Replacement")
        );
    }
}

#[test]
fn current_house_precedes_map_objects_and_survives_failed_starting_mcv() {
    // Exercise the same staged population and pre-Fill launch projection
    // used by the offline app. Blocking every candidate after map-object
    // creation supplies a deterministic total placement failure; it does
    // not replace the production MCV constructor/search/result branch.
    let rules_ini = launch_fixture_ini(
        "[General]\nBaseUnit=MTNK\n[VehicleTypes]\n0=MTNK\n\
         [BuildingTypes]\n0=CABHUT\n\
         [MTNK]\nStrength=300\nSpeed=6\nCost=100\nOwner=Americans,Russians\n\
         [CABHUT]\nStrength=100\nFoundation=1x1\nOwner=Neutral\n",
    );
    let start = Waypoint {
        index: 0,
        rx: 40,
        ry: 40,
    };
    let mut map = one_start_prefix_map(start);
    map.entities = crate::map::entities::parse_map_entities(
        &IniFile::from_str("[Structures]\n0=Neutral,CABHUT,256,30,30,0,None\n"),
        4,
    );
    let roster = HouseRoster::default();
    let overlays = OverlayTypeRegistry::empty();
    for (handle, with_ai, owner_key) in [
        ("Player", false, "Player"),
        ("Commander", false, "Commander"),
        ("Neutral", false, "Player"),
        ("sPeCiAl", true, "Player"),
        ("COMPUTER1", true, "Player"),
        ("Computer2", true, "Computer2"),
        ("Computer01", true, "Computer01"),
    ] {
        let mut launch = one_player_battle_launch("current-house.map");
        launch.session.local_player_mut().name = handle.to_string();
        launch.session.options.bases = true;
        launch.session.options.unit_count = 0;
        if with_ai {
            launch
                .session
                .opponents
                .push(crate::skirmish_launch::SkirmishAiSlot {
                    raw_slot_index: 1,
                    country: LaunchCountry::Russia,
                    country_random: false,
                    color_index: 1,
                    color_random: false,
                    start_position: LaunchStartPosition::Auto,
                    team: LaunchTeam::None,
                    difficulty: crate::skirmish_launch::AiDifficulty::Easy,
                });
            launch.session.ai_player_count = 1;

            map.waypoints.insert(
                1,
                Waypoint {
                    index: 1,
                    rx: 50,
                    ry: 50,
                },
            );
        } else {
            map.waypoints.remove(&1);
        }
        for block_placement in [false, true] {
            let mut scenario = descriptor(0xA83D4C);
            scenario.rule_policy = crate::rules::rule_policy::RulePolicy::stock_multiplayer();
            scenario.map_width = 96;
            scenario.map_height = 96;
            scenario.mp_start_waypoints.insert(0, (40, 40));
            let plan = prepare_stock_offline_scenario_prefix_plan(
                &launch,
                &map,
                &map.waypoints,
                scenario.seed,
            )
            .expect("admitted offline launch prefix");
            let mut rules_owner =
                crate::rules::process_owner::NativeRulesProcessOwner::from_cold_start_sources(
                    rules_ini.clone(),
                    None,
                    IniFile::from_str(""),
                    std::sync::Arc::default(),
                    Default::default(),
                )
                .expect("cold process Rules authority");
            let (rules, _, _, rules_receipt) = rules_owner
                .load_scenario(
                    crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
                    &map.ini,
                )
                .expect("production noncampaign Rules reset/rebuild")
                .into_parts();
            let bound_prefix = plan
                .bind_native_rules_receipt(rules_receipt, &launch, &rules)
                .unwrap();
            let (mut sim, projection) = ScenarioBootstrapRng::new(scenario.seed)
                .into_stock_offline_staged_simulation(&scenario, bound_prefix)
                .expect("production admission installs the paired RNG/native-ID prefix");
            assert!(sim.native_unique_ids.is_some());
            let native_before_map = sim.native_unique_ids.as_ref().unwrap().current_raw();
            let mut terrain = techno_constructor_flat_start_terrain(96);
            let overlay_grid = crate::sim::overlay_grid::OverlayGrid::new(96, 96);
            crate::sim::runtime::populate_staged_fixture(
                &mut sim,
                &map,
                &terrain,
                "TEMPERATE",
                &rules,
                Some(&overlays),
                Some(overlay_grid),
                crate::map::basic::BridgeDestroyabilityMode::SkirmishOrMultiplayer {
                    bridge_destruction: true,
                },
                &scenario,
                None,
                |sim| {
                    initialize_skirmish_launch_houses(sim, &roster, &rules, &launch);
                    assert!(
                        sim.entities().is_empty(),
                        "binding precedes every map object"
                    );
                    let player = sim.interner.get(owner_key).unwrap();
                    assert_eq!(sim.session.current_house(), Some(player));
                    assert_eq!(sim.houses.order()[0], player);
                    assert!(sim.houses[&player].is_human() && sim.houses[&player].player_control());
                },
            )
            .expect("shared staged object construction");
            assert_eq!(sim.entities().len(), 1, "map object actually constructed");
            assert_eq!(
                sim.entities().values().next().unwrap().native_unique_id(),
                native_before_map.wrapping_add(1) as i32,
                "production admission retains its concrete native constructor ID",
            );
            let player = sim.interner.get(owner_key).unwrap();
            assert_eq!(sim.session.current_house(), Some(player));
            if block_placement {
                let mut cells = terrain.cells().to_vec();
                for cell in &mut cells {
                    cell.overlay_blocks = true;
                }
                terrain = ResolvedTerrainGrid::from_cells(96, 96, cells);
                sim.install_resolved_terrain_for_new_map(terrain.clone());
            }
            let result = apply_pre_fill_scenario_prefix_launch_session(
                &mut sim,
                &map,
                &roster,
                &rules,
                &terrain,
                &launch,
                &projection,
            );
            assert_eq!(
                result.spawned_mcvs,
                if block_placement {
                    0
                } else {
                    1 + u32::from(with_ai)
                }
            );
            assert_eq!(
                sim.entities().len(),
                if block_placement {
                    1
                } else {
                    2 + usize::from(with_ai)
                }
            );
            assert_eq!(sim.session.current_house(), Some(player));
            assert!(sim.houses[&player].is_human() && sim.houses[&player].player_control());
            assert_eq!(launch.session.local_player().name, handle);
            let mut distinct = sim.houses.order().to_vec();
            distinct.sort();
            distinct.dedup();
            assert_eq!(distinct.len(), 3 + usize::from(with_ai));
            assert_eq!(sim.houses.len(), distinct.len());
            for name in ["Neutral", "Special"]
                .into_iter()
                .chain(with_ai.then_some("Computer1"))
            {
                let other = sim.interner.get(name).unwrap();
                assert_ne!(player, other);
                assert!(!sim.houses[&other].is_human() && !sim.houses[&other].player_control());
            }
            let neutral = sim.interner.get("Neutral").unwrap();
            let mut expected_mcv_owners = Vec::new();
            if !block_placement {
                expected_mcv_owners.push(player);
                if with_ai {
                    expected_mcv_owners.push(sim.interner.get("Computer1").unwrap());
                }
            }
            expected_mcv_owners.sort();
            let assert_entity_owners = |world: &Simulation| {
                let hut_owners: Vec<_> = world
                    .entities()
                    .values()
                    .filter(|entity| world.interner.resolve(entity.type_ref()) == "CABHUT")
                    .map(|entity| entity.owner())
                    .collect();
                assert_eq!(
                    hut_owners,
                    [neutral],
                    "authored hut keeps its Neutral owner"
                );
                let mut mcv_owners: Vec<_> = world
                    .entities()
                    .values()
                    .filter(|entity| world.interner.resolve(entity.type_ref()) == "MTNK")
                    .map(|entity| entity.owner())
                    .collect();
                mcv_owners.sort();
                assert_eq!(
                    mcv_owners, expected_mcv_owners,
                    "each successful MCV belongs to its distinct participant; failures leave none"
                );
            };
            assert_entity_owners(&sim);
            let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "owner collision", 0);
            let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
                .unwrap()
                .sim;
            restored
                .restore_after_snapshot_load()
                .expect("valid distinct House registry");
            assert_eq!(restored.session.current_house(), Some(player));
            assert_eq!(restored.houses.order(), sim.houses.order());
            assert!(
                restored.houses[&player].is_human() && restored.houses[&player].player_control()
            );
            assert_entity_owners(&restored);
        }
    }
}

fn techno_constructor_start_rules() -> RuleSet {
    RuleSet::from_ini(&launch_fixture_ini(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n0=MTNK\n1=HTNK\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [MTNK]\nStrength=300\nSpeed=6\nCost=100\nTechLevel=1\nOwner=Americans\nAllowedToStartInMultiplayer=yes\n\n\
         [HTNK]\nStrength=400\nSpeed=5\nCost=100\nTechLevel=1\nOwner=Americans\nAllowedToStartInMultiplayer=yes\n",
    ))
    .expect("starting-object constructor rules")
}

fn techno_constructor_flat_start_terrain(size: u16) -> ResolvedTerrainGrid {
    crate::map::resolved_terrain::test_grid(size, size, |rx, ry| ResolvedTerrainCell {
        speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
        base_speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
        ..crate::map::resolved_terrain::test_tiberium_cell(rx, ry)
    })
}

fn techno_constructor_start_sim(seed: u64, size: u16) -> Simulation {
    let mut sim = Simulation::with_seed(seed);
    sim.session.set_map_width_for_test(size);
    sim.session.set_map_height_for_test(size);
    sim.set_playfield_bounds_for_test(Some(crate::map::playfield::PlayfieldBounds {
        base: i32::from(size),
        off_fc: 0,
        off_100: 0,
        off_104: i32::from(size),
        off_108: i32::from(size),
    }));
    sim
}

#[test]
fn techno_constructor_postmap_exact_and_fallback_place_one_constructed_identity() {
    let seed = 0xC701_1001;
    let rules = techno_constructor_start_rules();
    let terrain = techno_constructor_flat_start_terrain(10);
    let bounds = NativeStartBounds {
        min_rx: 1,
        min_ry: 1,
        width: 9,
        height: 9,
    };

    let mut exact = techno_constructor_start_sim(seed, 10);
    let mut exact_expected = SimRng::new(seed);
    let exact_word = (exact_expected.next_u32() & 0xFFFF) as u16;
    let exact_id = place_starting_object_near_base(
        &mut exact,
        "MTNK",
        "Americans",
        6,
        5,
        STARTING_MCV_FACING,
        1,
        bounds,
        &rules,
        &terrain,
        None,
    )
    .expect("exact PostMap placement");
    let exact_entity = exact.substrate.entities.get(exact_id).unwrap();
    assert_eq!(exact_entity.techno_ctor_random_word(), exact_word);
    assert_eq!(
        (exact_entity.position.rx(), exact_entity.position.ry()),
        (6, 5)
    );
    assert_eq!(
        exact.scenario_rng.logical_state(),
        exact_expected.logical_state()
    );

    let mut fallback = techno_constructor_start_sim(seed, 10);
    let mut fallback_expected = SimRng::new(seed);
    let blocker_word = (fallback_expected.next_u32() & 0xFFFF) as u16;
    let blocker = fallback
        .spawn_object("MTNK", "Americans", 6, 5, 0, &rules)
        .unwrap();
    assert_eq!(
        fallback
            .substrate
            .entities
            .get(blocker)
            .unwrap()
            .techno_ctor_random_word(),
        blocker_word
    );
    let fallback_word = (fallback_expected.next_u32() & 0xFFFF) as u16;
    let _start_direction = fallback_expected.next_range_u32_inclusive(0, 7);
    let fallback_id = place_starting_object_near_base(
        &mut fallback,
        "MTNK",
        "Americans",
        6,
        5,
        STARTING_MCV_FACING,
        1,
        bounds,
        &rules,
        &terrain,
        None,
    )
    .expect("fallback PostMap placement");
    let fallback_entity = fallback.substrate.entities.get(fallback_id).unwrap();
    assert_eq!(fallback_entity.techno_ctor_random_word(), fallback_word);
    assert_ne!(
        (fallback_entity.position.rx(), fallback_entity.position.ry()),
        (6, 5)
    );
    assert_eq!(
        fallback.scenario_rng.logical_state(),
        fallback_expected.logical_state()
    );
}

#[test]
fn techno_constructor_postmap_total_failure_deletes_object_but_keeps_draw_first() {
    let seed = 0xC701_1002;
    let rules = techno_constructor_start_rules();
    let terrain = ResolvedTerrainGrid::from_cells(10, 10, Vec::new());
    let bounds = NativeStartBounds {
        min_rx: 1,
        min_ry: 1,
        width: 9,
        height: 9,
    };
    let mut sim = techno_constructor_start_sim(seed, 10);
    let mut expected = SimRng::new(seed);
    let _constructor_word = expected.next_u32();
    for _radius in 1..=STARTING_MCV_FALLBACK_MAX_RADIUS {
        let _ = expected.next_range_u32_inclusive(0, 7);
        for jitter_pass in 0..2 {
            for _offset in 0..8 {
                if jitter_pass != 0 {
                    let _ = expected.next_range_u32_inclusive(0, 1);
                    let _ = expected.next_range_u32_inclusive(0, 99);
                    let _ = expected.next_range_u32_inclusive(0, 1);
                    let _ = expected.next_range_u32_inclusive(0, 99);
                }
            }
        }
    }

    assert!(
        place_starting_object_near_base(
            &mut sim,
            "MTNK",
            "Americans",
            5,
            5,
            STARTING_MCV_FACING,
            1,
            bounds,
            &rules,
            &terrain,
            None,
        )
        .is_none()
    );
    assert!(sim.substrate.entities.is_empty());
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
}

#[test]
fn techno_constructor_starting_unit_uses_raw_unit_admission_before_fallback() {
    let seed = 0xC701_1004;
    let rules = techno_constructor_start_rules();
    let terrain = techno_constructor_flat_start_terrain(10);
    let bounds = NativeStartBounds {
        min_rx: 1,
        min_ry: 1,
        width: 9,
        height: 9,
    };
    let base = (6, 5);
    let mut sim = techno_constructor_start_sim(seed, 10);
    sim.install_resolved_terrain_for_new_map(terrain.clone());
    sim.substrate.raw_cell_occupation.mark_ground(
        base.0,
        base.1,
        crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
    );

    let mut expected = SimRng::new(seed);
    let expected_word = (expected.next_u32() & 0xFFFF) as u16;
    let _fallback_start_direction = expected.next_range_u32_inclusive(0, 7);
    let stable_id = place_starting_object_near_base(
        &mut sim,
        "MTNK",
        "Americans",
        base.0,
        base.1,
        STARTING_MCV_FACING,
        1,
        bounds,
        &rules,
        &terrain,
        None,
    )
    .expect("raw-bit rejection must retry the same constructed Unit");

    let placed = sim.substrate.entities.get(stable_id).unwrap();
    assert_eq!(placed.techno_ctor_random_word(), expected_word);
    assert_ne!((placed.position.rx(), placed.position.ry()), base);
    assert_eq!(stable_id, 1, "fallback must retain the first identity");
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
}

#[test]
fn techno_constructor_extra_unit_selection_precedes_constructor_word() {
    let seed = 0xC701_1003;
    let rules = techno_constructor_start_rules();
    let terrain = techno_constructor_flat_start_terrain(10);
    let bounds = NativeStartBounds {
        min_rx: 1,
        min_ry: 1,
        width: 9,
        height: 9,
    };
    let mut sim = techno_constructor_start_sim(seed, 10);
    sim.session.game_options_mut_for_test().tech_level = 10;
    let owner = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner, 0, Some(owner), true, 0, 10);
    house.set_base_center_for_test(Some((6, 5)));
    sim.houses.register(house);
    let slots = [NormalizedSkirmishSlot {
        owner_name: "Americans".to_string(),
        country: LaunchCountry::America,
        color_index: 0,
        start_position: LaunchStartPosition::Auto,
        team: LaunchTeam::None,
        is_human: true,
        observer: false,
        controllers: vec![LaunchController {
            human_index: 0,
            source: 0,
        }],
        handle: None,
        difficulty: HouseDifficulty::Normal,
        commander: None,
    }];
    let mut expected = SimRng::new(seed);
    let blocker_word = (expected.next_u32() & 0xFFFF) as u16;
    let blocker = sim
        .spawn_object("MTNK", "Americans", 6, 5, 0, &rules)
        .expect("starting-cell blocker");
    assert_eq!(
        sim.substrate
            .entities
            .get(blocker)
            .unwrap()
            .techno_ctor_random_word(),
        blocker_word
    );
    let candidate_index = expected.next_range_u32_inclusive(0, 1) as usize;
    let expected_type = ["MTNK", "HTNK"][candidate_index];
    let expected_word = (expected.next_u32() & 0xFFFF) as u16;
    let _fallback_start_direction = expected.next_range_u32_inclusive(0, 7);

    assert_eq!(
        seed_starting_extra_units(&mut sim, &slots, &rules, &terrain, bounds, 1, false,),
        1
    );
    let entity = sim.substrate.entities.get(2).unwrap();
    assert_eq!(sim.interner.resolve(entity.type_ref()), expected_type);
    assert_eq!(entity.techno_ctor_random_word(), expected_word);
    assert_ne!((entity.position.rx(), entity.position.ry()), (6, 5));
    assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
}

#[test]
fn nearoref_native_start_bounds_use_inclusive_full_cell_array_endpoints() {
    let mut sim = Simulation::new();
    sim.session.set_map_width_for_test(138);
    sim.session.set_map_height_for_test(138);
    let terrain = ResolvedTerrainGrid::from_cells(138, 138, Vec::new());

    let bounds = NativeStartBounds::from_session(&sim, &terrain);

    assert_eq!(
        (
            bounds.min_rx,
            bounds.min_ry,
            bounds.width,
            bounds.height,
            bounds.max_rx(),
            bounds.max_ry(),
        ),
        (1, 1, 137, 137, 137, 137)
    );
    assert_eq!(bounds.clamp(0, 0), (1, 1));
    assert_eq!(bounds.clamp(1, 1), (1, 1));
    assert_eq!(bounds.clamp(100, 75), (100, 75));
    assert_eq!(bounds.clamp(137, 137), (137, 137));
    assert_eq!(bounds.clamp(138, 138), (137, 137));
    assert_eq!(bounds.clamp(i32::MIN, i32::MAX), (1, 137));
}

/// Retail starts sit outside `[Map] Size=`: Dustbowl's Size is 70x76 and
/// its starts are at rx 70 and 79. Start bounds and the session's cell
/// extent therefore come from the Resize cell array, which must hold every
/// authored start without the fallback clamp moving it.
#[test]
fn retail_dustbowl_starts_need_the_cell_array_not_the_size_rect() {
    let Some((root, _assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let map = crate::map::map_file::load_from_path(&root.join("Dustbowl.mmx"))
        .expect("retail Dustbowl.mmx parses");
    let bounds = NativeStartBounds::from_map_header(&map.header).expect("valid Size=");
    let starts = crate::map::waypoints::multiplayer_start_waypoints(&map.waypoints);
    assert!(!starts.is_empty());
    for start in &starts {
        assert_eq!(
            bounds.clamp(i32::from(start.rx), i32::from(start.ry)),
            (start.rx, start.ry),
            "start {}",
            start.index
        );
    }
    assert!(
        starts
            .iter()
            .any(|start| u32::from(start.rx) >= map.header.width
                || u32::from(start.ry) >= map.header.height),
        "some authored start lies outside the Size= rect"
    );
}

#[test]
fn untouched_bootstrap_cursors_match_fresh_descriptor_construction() {
    let seed = 0x51C0_1001;
    let expected = Simulation::from_descriptor(&descriptor(seed)).rng_state();
    let actual = ScenarioBootstrapRng::new(seed)
        .into_simulation(&descriptor(seed))
        .rng_state();

    assert_eq!(actual, expected);
}

#[test]
fn generated_mapgen_continuation_replaces_only_mapgen_cursor() {
    let match_seed = 0x51C0_1005;
    let map_seed: u16 = 0xBEEF;
    let mut expected_mapgen = SimRng::new(u64::from(map_seed));
    for _ in 0..353 {
        let _ = expected_mapgen.next_u32();
    }
    let generated = expected_mapgen.logical_state();

    let mut owner = ScenarioBootstrapRng::new(match_seed);
    owner.install_process_mapgen_continuation(MapGenRngContinuation::from_native_parts(
        generated.disabled,
        generated.words,
        usize::try_from(generated.index_a).expect("test MapGen cursor A is non-negative"),
        usize::try_from(generated.index_b).expect("test MapGen cursor B is non-negative"),
    ));
    let actual = owner.into_simulation(&descriptor(match_seed)).rng_state();

    assert_eq!(
        actual.scenario,
        SimRng::new(u64::from(match_seed)).logical_state()
    );
    assert_eq!(
        actual.main,
        SimRng::new(u64::from(match_seed)).logical_state()
    );
    assert_eq!(actual.mapgen, expected_mapgen.logical_state());
}

#[test]
fn headless_battle_runs_both_house_passes_for_the_eighth_ai() {
    use crate::skirmish_launch::AiDifficulty;

    let opponents = [(LaunchCountry::America, AiDifficulty::Normal); 8];
    let session = crate::headless_scenario::battle_session("nine-starts.map", &opponents);
    let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
    let starts: Vec<_> = (0..9u16)
        .map(|row| Waypoint {
            index: u32::from(row),
            rx: 32 + 8 * (row % 3),
            ry: 42 + 8 * (row / 3),
        })
        .collect();
    let map = prefix_map_with_starts(&starts);
    let plan =
        prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, 42).unwrap();

    assert_eq!(plan.final_gathered_starts().len(), 9);
    assert_eq!(
        (
            plan.first_house_timers().len(),
            plan.second_house_timers().len()
        ),
        (11, 11),
        "each pass constructs the human, all eight computers, Neutral and Special"
    );
}

#[test]
fn scenario_prefix_runs_two_identical_house_passes_before_fill() {
    use crate::skirmish_launch::{
        AiDifficulty, PreFillAiHouseSlot, PreFillHouseRoster, PreFillHumanHouse, SkirmishAiSlot,
    };

    let seed = (0..100_000u32)
        .find(|seed| {
            let mut rng = SimRng::new(u64::from(*seed));
            let before = rng.logical_state().index_a;
            let _ = rng
                .next_range_u32_inclusive(HOUSE_CONSTRUCTOR_TIMER_MIN, HOUSE_CONSTRUCTOR_TIMER_MAX);
            (rng.logical_state().index_a - before).rem_euclid(250) > 1
        })
        .expect("the native ranged domain has a rejection seed");
    let roster = PreFillHouseRoster::new(
        vec![
            PreFillHumanHouse {
                priority: 4,
                source_order: 1,
                observer: false,
            },
            PreFillHumanHouse {
                priority: -7,
                source_order: 0,
                observer: true,
            },
        ],
        vec![
            PreFillAiHouseSlot {
                slot_index: 3,
                valid: true,
            },
            PreFillAiHouseSlot {
                slot_index: 1,
                valid: false,
            },
            PreFillAiHouseSlot {
                slot_index: 0,
                valid: true,
            },
        ],
    );
    assert_eq!(roster.human_nodes()[0].priority, -7);
    assert_eq!(
        roster
            .ai_slots()
            .iter()
            .map(|slot| slot.slot_index)
            .collect::<Vec<_>>(),
        vec![0, 1, 3]
    );
    assert_eq!(roster.nonobserver_house_count(), 3);
    assert_eq!(roster.created_house_count(), 6);

    let mut session = one_player_battle_launch("prefix-roster.mmx")
        .session()
        .clone();
    session.local_slot_mut().start_position = LaunchStartPosition::Position(0);
    session.opponents = (0..2)
        .map(|index| SkirmishAiSlot {
            raw_slot_index: (index + 1) as u8,
            country: LaunchCountry::Russia,
            country_random: false,
            color_index: index + 1,
            color_random: false,
            start_position: LaunchStartPosition::Position(index + 1),
            team: LaunchTeam::None,
            difficulty: AiDifficulty::Easy,
        })
        .collect();
    session.ai_player_count = session.opponents.len() as i32;
    session.set_recorded_house_roster_for_test(roster);
    let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
    let starts = [
        Waypoint {
            index: 0,
            rx: 30,
            ry: 30,
        },
        Waypoint {
            index: 1,
            rx: 40,
            ry: 40,
        },
        Waypoint {
            index: 2,
            rx: 50,
            ry: 50,
        },
    ];
    let map = prefix_map_with_starts(&starts);
    let plan =
        prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed).unwrap();

    let native_plan =
        prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed).unwrap();
    let mut native_rules =
        crate::rules::process_owner::NativeRulesProcessOwner::from_cold_start_sources(
            IniFile::from_str(CONTROLLED_STOCK_COUNTRIES),
            None,
            IniFile::from_bytes(b"").unwrap(),
            std::sync::Arc::default(),
            Default::default(),
        )
        .unwrap();
    let native_load = native_rules
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None),
            &map.ini,
        )
        .unwrap();
    let (rules, _, _, receipt) = native_load.into_parts();
    let early_count = receipt.pre_reset().event_count() as u32;
    let first_super_count = receipt.pre_reset().allocated_super_weapon_type_count() as u32;
    let rebuilt_count = receipt.post_reset().event_count() as u32;
    let final_super_count = receipt.post_reset().allocated_super_weapon_type_count() as u32;
    let bound_native = native_plan
        .bind_native_rules_receipt(receipt, &launch, &rules)
        .unwrap();
    let native_checkpoints = bound_native.native_ids.checkpoints();
    let resize_count = map
        .header
        .height
        .wrapping_mul(map.header.width.wrapping_mul(2).wrapping_sub(1))
        .wrapping_add(1);
    assert_eq!(
        native_checkpoints.after_early_types,
        1_000_000 + early_count
    );
    assert_eq!(
        native_checkpoints.after_first_house_generation,
        native_checkpoints
            .after_early_types
            .wrapping_add(6 * (1 + first_super_count)),
        "observer, both valid AI rows, ordinary human, Neutral, and Special all construct"
    );
    assert_eq!(
        native_checkpoints.after_first_resize,
        native_checkpoints
            .after_first_house_generation
            .wrapping_add(resize_count),
        "the first native Cell generation uses [Map] Size and includes the dummy"
    );
    assert_eq!(
        native_checkpoints.after_rebuilt_types,
        native_checkpoints
            .after_first_resize
            .wrapping_add(rebuilt_count)
    );
    assert_eq!(
        native_checkpoints.after_final_house_generation,
        native_checkpoints
            .after_rebuilt_types
            .wrapping_add(6 * (1 + final_super_count))
    );
    assert_eq!(
        native_checkpoints.after_final_resize,
        native_checkpoints
            .after_final_house_generation
            .wrapping_add(resize_count),
        "the same live Cell identities reconstruct in the second Resize generation"
    );

    let mut reference = SimRng::new(u64::from(seed));
    let first_timers: Vec<_> = (0..6)
        .map(|_| {
            reference
                .next_range_u32_inclusive(HOUSE_CONSTRUCTOR_TIMER_MIN, HOUSE_CONSTRUCTOR_TIMER_MAX)
        })
        .collect();
    let after_first = reference.logical_state();
    let second_timers: Vec<_> = (0..6)
        .map(|_| {
            reference
                .next_range_u32_inclusive(HOUSE_CONSTRUCTOR_TIMER_MIN, HOUSE_CONSTRUCTOR_TIMER_MAX)
        })
        .collect();
    let checkpoints = plan.rng_checkpoints();
    assert_eq!(plan.first_house_timers(), first_timers);
    assert_eq!(plan.second_house_timers(), second_timers);
    assert_eq!(checkpoints.after_first_house_pass, after_first);
    assert_eq!(checkpoints.after_first_gather, after_first);
    assert_eq!(
        checkpoints.after_second_gather_and_chooser, after_first,
        "complete explicit starts make both callbacks draw-free"
    );
    assert_eq!(
        checkpoints.after_zero_draw_reset,
        checkpoints.after_second_gather_and_chooser
    );
    assert_eq!(
        checkpoints.after_second_house_pass,
        reference.logical_state()
    );

    let mut owner = ScenarioBootstrapRng::new(seed);
    owner.install_pre_fill_scenario_prefix_plan(plan).unwrap();
    let sim = owner.into_simulation(&descriptor(seed));
    assert!(
        sim.houses.is_empty(),
        "prefix emulation retains no disposable first-pass House state"
    );
}

#[test]
fn scenario_prefix_declared_thirty_starts_preserves_two_player_start29() {
    let mut source = String::from(
        "[Basic]\nMaxPlayers=30\n[Map]\nSize=0,0,64,64\nLocalSize=0,0,64,64\n[Waypoints]\n",
    );
    for index in 0..30 {
        source.push_str(&format!("{index}={}\n", 30030 + index));
    }
    source.push_str("[IsoMapPack5]\n1=CAAEABUAAAAAEQAA\n");
    let map = MapFile::from_bytes(source.as_bytes()).unwrap();
    let mut session = one_player_battle_launch("THIRTY.MAP").session().clone();
    session.humans[0].slot.start_position = LaunchStartPosition::Position(0);
    let mut guest = session.humans[0].clone();
    guest.name = "Guest".into();
    guest.raw_slot_index = Some(1);
    guest.slot.color_index = 1;
    guest.slot.start_position = LaunchStartPosition::Position(29);
    session.humans.push(guest);

    let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
    let plan =
        prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, 17).unwrap();
    assert_eq!(plan.first_gathered_starts().len(), 30);
    assert_eq!(plan.final_gathered_starts().len(), 30);
    assert_eq!(plan.assignment().placements[1].1, map.waypoints[&29]);
    assert_eq!(
        plan.rng_checkpoints().after_first_house_pass,
        plan.rng_checkpoints().after_second_gather_and_chooser
    );
}

#[test]
fn scenario_prefix_sparse_waypoints_preserve_only_entries_below_target() {
    let map = prefix_map_with_starts(&[]);
    let starts = HashMap::from([
        (
            0,
            Waypoint {
                index: 0,
                rx: 30,
                ry: 30,
            },
        ),
        (
            2,
            Waypoint {
                index: 2,
                rx: 50,
                ry: 50,
            },
        ),
    ]);
    let mut target_two_rng = SimRng::new(0x5200);
    let target_two =
        native_gather_pre_fill_start_positions(&starts, 2, &map, &mut target_two_rng).unwrap();
    assert_eq!(target_two.len(), 2);
    assert_eq!((target_two[0].rx, target_two[0].ry), (30, 30));
    assert!(
        !target_two
            .iter()
            .any(|start| (start.rx, start.ry) == (50, 50))
    );

    let mut target_three_rng = SimRng::new(0x5200);
    let target_three =
        native_gather_pre_fill_start_positions(&starts, 3, &map, &mut target_three_rng).unwrap();
    assert_eq!(target_three.len(), 3);
    assert_eq!((target_three[0].rx, target_three[0].ry), (30, 30));
    assert_eq!((target_three[1].rx, target_three[1].ry), (50, 50));
}

#[test]
fn scenario_prefix_all_stock_mode_families_gather_twice() {
    let map = prefix_map_with_starts(&[]);
    let modes = crate::skirmish_modes::stock_skirmish_modes();
    assert_eq!(modes.len(), 9);
    for mode in modes {
        let mut session = one_player_battle_launch("stock-mode.mmx").session().clone();
        session.mode = crate::skirmish_launch::SkirmishLaunchMode::from_game_mode(&mode);
        let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
        let expected_family = if mode.id == 3 {
            StockOfflineStartCallbackFamily::Cooperative
        } else {
            StockOfflineStartCallbackFamily::Battle
        };
        assert_eq!(
            stock_offline_start_callback_family(launch.session()).unwrap(),
            expected_family
        );
        let plan = prepare_stock_offline_scenario_prefix_plan(
            &launch,
            &map,
            &map.waypoints,
            0x5300 + mode.id as u32,
        )
        .unwrap_or_else(|err| panic!("stock mode {} prefix failed: {err}", mode.id));
        assert_eq!(plan.first_gathered_starts().len(), 1);
        assert_eq!(plan.final_gathered_starts().len(), 1);
        assert_ne!(
            plan.rng_checkpoints().after_first_gather,
            plan.rng_checkpoints().after_second_gather_and_chooser,
            "stock mode {} must execute its second deficient Gather",
            mode.id
        );
    }
}

#[test]
fn a_changed_or_added_mode_row_starts_as_its_class_does() {
    let mut session = one_player_battle_launch("mod-mode.mmx").session().clone();
    session.mode.id = 10;
    session.mode.override_file = "MPModBattleMD.ini".to_string();
    session.mode.must_ally = true;
    assert_eq!(
        stock_offline_start_callback_family(&session),
        Ok(StockOfflineStartCallbackFamily::Battle)
    );
    session.mode.class = crate::skirmish_modes::MpModeClass::Cooperative;
    assert_eq!(
        stock_offline_start_callback_family(&session),
        Ok(StockOfflineStartCallbackFamily::Cooperative)
    );
    session.mode.class = crate::skirmish_modes::MpModeClass::Siege;
    assert_eq!(
        stock_offline_start_callback_family(&session),
        Err(PreFillScenarioPrefixPlanError::UnsupportedSiegeMode { id: 10 })
    );
}

#[test]
fn scenario_prefix_rejects_invalid_map_cell_extents() {
    let launch = one_player_battle_launch("invalid-size.mmx");
    for (width, height) in [(0, 64), (64, 0), (513, 512), (u32::MAX, 1)] {
        let mut map = one_start_prefix_map(Waypoint {
            index: 0,
            rx: 30,
            ry: 30,
        });
        map.header.width = width;
        map.header.height = height;
        assert_eq!(
            prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, 0x5320,)
                .unwrap_err(),
            PreFillScenarioPrefixPlanError::InvalidMapCellExtent,
            "Size={width},{height}"
        );
    }

    let mut boundary = one_start_prefix_map(Waypoint {
        index: 0,
        rx: 30,
        ry: 30,
    });
    boundary.header.width = 256;
    boundary.header.height = 256;
    prepare_stock_offline_scenario_prefix_plan(&launch, &boundary, &boundary.waypoints, 0x5320)
        .expect("Size sum 512 is the inclusive native cell-array boundary");
}

#[test]
fn scenario_prefix_ignores_later_map_payload() {
    use crate::map::entities::{EntityCategory, MapEntity};
    use crate::map::map_file::MapCell;
    use crate::map::overlay::{OverlayEntry, TerrainObject};

    let start = Waypoint {
        index: 0,
        rx: 35,
        ry: 35,
    };
    let map_a = one_start_prefix_map(start);
    let mut map_b = one_start_prefix_map(start);
    map_b.header.fill = "Water".to_string();
    map_b.header.level = 9;
    map_b.cells.push(MapCell {
        legacy_ice_byte: 0,
        rx: 36,
        ry: 36,
        tile_index: 777,
        sub_tile: 3,
        z: 9,
    });
    map_b.overlays.push(OverlayEntry {
        rx: 36,
        ry: 36,
        overlay_id: 24,
        frame: 7,
    });
    map_b.terrain_objects.push(TerrainObject {
        rx: 37,
        ry: 37,
        name: "TREE01".to_string(),
    });
    map_b.entities.push(MapEntity {
        owner: "Neutral".to_string(),
        type_id: "CAOILD".to_string(),
        health: 256,
        cell_x: 38,
        cell_y: 38,
        facing: 0,
        category: EntityCategory::Structure,
        sub_cell: 0,
        veterancy: None,
        high: false,
        mission: None,
        recruitable_a: true,
        recruitable_b: true,
        group: -1,
        follows: None,
        structure_columns: Default::default(),
        structure_ai_sellable: false,
        structure_ai_repairable: false,
        structure_online: true,
        trigger_tag: None,
    });
    let launch = one_player_battle_launch("payload.mmx");
    let plan_a =
        prepare_stock_offline_scenario_prefix_plan(&launch, &map_a, &map_a.waypoints, 0x5400)
            .unwrap();
    let plan_b =
        prepare_stock_offline_scenario_prefix_plan(&launch, &map_b, &map_b.waypoints, 0x5400)
            .unwrap();
    assert_eq!(
        plan_a.first_gathered_starts(),
        plan_b.first_gathered_starts()
    );
    assert_eq!(
        plan_a.final_gathered_starts(),
        plan_b.final_gathered_starts()
    );
    assert_eq!(plan_a.assignment(), plan_b.assignment());
    assert_eq!(plan_a.scenario_rng_after, plan_b.scenario_rng_after);
}

#[test]
fn map_houses_take_a_ui_name_key_or_keep_the_type_name() {
    use crate::sim::house_state::HouseUiName;
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[Houses]\n0=Allies\n1=Soviets\n\
         [Allies]\nUIName=Name:AlliedHouse\n[Soviets]\nUIName=\n",
    );
    let roster = crate::map::houses::parse_house_roster(&ini, &[], None);
    let mut sim = Simulation::new();
    initialize_map_roster_houses(
        &mut sim,
        &roster,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        Some(&ini),
    );
    let name = |house: &str| {
        let id = sim.interner.get(house).unwrap();
        sim.houses[&id].ui_name().clone()
    };
    assert_eq!(name("Allies"), HouseUiName::Csf("Name:AlliedHouse".into()));
    assert_eq!(name("Soviets"), HouseUiName::TypeName);
}

#[test]
fn launch_houses_take_each_humans_handle_and_computer_ui_names() {
    use crate::sim::house_state::HouseUiName;
    // CreateHouses 688092..6880A4: each human its own node's handle (at
    // most 20 wide chars), even a duplicate; 688252..688270: computers
    // TXT_COMPUTER. Owner keys stay distinct.
    let mut session = one_player_battle_launch("names.mmx").session().clone();
    session.humans[0].name = "Twentyone-characters".to_string() + "X";
    let mut peer = session.humans[0].clone();
    peer.raw_slot_index = Some(1);
    peer.slot.color_index = 1;
    peer.name = "Remote".to_string();
    let mut twin = peer.clone();
    twin.raw_slot_index = Some(2);
    twin.slot.color_index = 2;
    session.humans.push(peer);
    session.humans.push(twin);

    let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
    let mut sim = Simulation::new();
    initialize_skirmish_launch_houses(
        &mut sim,
        &HouseRoster::default(),
        &techno_constructor_start_rules(),
        &launch,
    );
    let names: Vec<_> = normalized_launch_slots(launch.session())
        .iter()
        .map(|slot| {
            let owner = sim.interner.get(&slot.owner_name).unwrap();
            (
                slot.owner_name.clone(),
                sim.houses[&owner].ui_name().clone(),
            )
        })
        .collect();
    assert_eq!(
        names[0].1,
        HouseUiName::Handle("Twentyone-characters".into())
    );
    assert_eq!(
        names[1],
        ("Remote".into(), HouseUiName::Handle("Remote".into()))
    );
    assert_eq!(names[2].1, HouseUiName::Handle("Remote".into()));
    assert_ne!(names[2].0, "Remote");
    assert!(
        names[3..]
            .iter()
            .all(|(_, name)| *name == HouseUiName::Computer)
    );
    assert_eq!(
        HouseUiName::bounded("\u{1F600}".repeat(11).as_str())
            .chars()
            .count(),
        10
    );
}

#[test]
fn scenario_prefix_projection_is_draw_free() {
    let seed = 0x5500;
    let start = Waypoint {
        index: 0,
        rx: 40,
        ry: 40,
    };
    let map = one_start_prefix_map(start);
    let mut session = one_player_battle_launch("projection.mmx").session().clone();
    session.local_slot_mut().start_position = LaunchStartPosition::Position(0);
    let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
    let plan =
        prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed).unwrap();
    let mut owner = ScenarioBootstrapRng::new(seed);
    let projection = owner.install_pre_fill_scenario_prefix_plan(plan).unwrap();
    let mut sim = owner.into_simulation(&descriptor(seed));
    let rules = techno_constructor_start_rules();
    initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &launch);
    assert_eq!(sim.session.current_house(), Some(sim.houses.order()[0]));
    let before = sim.scenario_rng.logical_state();
    let slots = normalized_launch_slots(launch.session());
    project_pre_fill_start_assignment(&mut sim, &slots, projection.assignment());
    assert_eq!(sim.scenario_rng.logical_state(), before);
    assert_eq!(
        sim.houses
            .get(&sim.houses.order()[0])
            .and_then(|house| house.base_center()),
        Some((40, 40))
    );
}

/// The start becomes the base centre only: the House edge (+0x577C) keeps
/// the constructor's North (`0x004F5C79`) until a yard's DetermineEdge
/// (`0x0050DB00`), here where the start lies nearest East.
#[test]
fn scenario_prefix_start_leaves_the_house_edge_north() {
    let seed = 0x5501;
    let start = Waypoint {
        index: 0,
        rx: 60,
        ry: 30,
    };
    let map = one_start_prefix_map(start);
    let mut session = one_player_battle_launch("projection.mmx").session().clone();
    session.local_slot_mut().start_position = LaunchStartPosition::Position(0);
    let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
    let plan =
        prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed).unwrap();
    let mut owner = ScenarioBootstrapRng::new(seed);
    let projection = owner.install_pre_fill_scenario_prefix_plan(plan).unwrap();
    let mut sim = owner.into_simulation(&descriptor(seed));
    let bounds = crate::map::playfield::PlayfieldBounds::from_map_header(&map.header);
    sim.set_playfield_bounds_for_test(Some(bounds));
    let rules = techno_constructor_start_rules();
    initialize_skirmish_launch_houses(&mut sim, &HouseRoster::default(), &rules, &launch);
    let slots = normalized_launch_slots(launch.session());
    project_pre_fill_start_assignment(&mut sim, &slots, projection.assignment());
    let house = &sim.houses[&sim.houses.order()[0]];
    assert_eq!(house.base_center(), Some((60, 30)));
    assert_eq!(
        crate::sim::house_state::determine_waypoint_edge((60, 30), bounds),
        1
    );
    assert_eq!(house.waypoint_edge(), 0);
}

#[test]
fn scenario_prefix_cursor_reaches_base_plan_consumer() {
    use crate::sim::base_plan::BasePlanState;
    use crate::sim::base_plan_generation::recalc_base_plan;

    let seed = 0x5600;
    let start = Waypoint {
        index: 0,
        rx: 40,
        ry: 40,
    };
    let map = one_start_prefix_map(start);
    let mut session = one_player_battle_launch("base-plan.mmx").session().clone();
    session.local_slot_mut().start_position = LaunchStartPosition::Position(0);
    let launch = MatchLaunchDescriptor::from_resolved(session).unwrap();
    let plan =
        prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed).unwrap();
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\n\
         HarvesterUnit=HARV\n\
         AIExtraRefineries=1,1,1\n\
         AISlaveMinerNumber=1,1,1\n\
         AlliedBaseDefenseCounts=1,1,1\n\
         SovietBaseDefenseCounts=0,0,0\n\
         ThirdBaseDefenseCounts=0,0,0\n\
         [AI]\n\
         BuildConst=CON\nBuildPower=POW\nBuildRefinery=REF\n\
         BuildBarracks=BAR\nBuildWeapons=WEAP\nBuildRadar=RAD\nBuildTech=TECH\n\
         [Countries]\n0=Americans\n[Sides]\nAllied=Americans\n\
         [Americans]\nSide=Allied\n\
         [VehicleTypes]\n0=HARV\n[HARV]\nOwner=Americans\nStrength=1\n\
         [BuildingTypes]\n0=CON\n1=POW\n2=REF\n3=BAR\n4=WEAP\n5=RAD\n6=TECH\n\
         [CON]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nFoundation=1x1\n\
         [POW]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nFoundation=1x1\n\
         [REF]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nFoundation=1x1\n\
         [BAR]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nFoundation=1x1\n\
         [WEAP]\nOwner=Americans\nAIBuildThis=yes\nTechLevel=1\nFoundation=1x1\n\
         [RAD]\nOwner=Americans\nAIBuildThis=no\nFoundation=1x1\n\
         [TECH]\nOwner=Americans\nAIBuildThis=no\nFoundation=1x1\n",
    ))
    .unwrap();

    let mut owner = ScenarioBootstrapRng::new(seed);
    owner.install_pre_fill_scenario_prefix_plan(plan).unwrap();
    let mut sim = owner.into_simulation(&descriptor(seed));
    let mut expected_rng = SimRng::new(u64::from(seed));
    for _ in 0..6 {
        let _ = expected_rng
            .next_range_u32_inclusive(HOUSE_CONSTRUCTOR_TIMER_MIN, HOUSE_CONSTRUCTOR_TIMER_MAX);
    }
    let mut actual_plan = BasePlanState::default();
    let mut expected_plan = BasePlanState::default();
    recalc_base_plan(
        &mut actual_plan,
        &rules,
        &crate::sim::world::TypeTechLevels::default(),
        "Americans",
        0,
        HouseDifficulty::Normal,
        10,
        true,
        &mut sim.scenario_rng,
    );
    recalc_base_plan(
        &mut expected_plan,
        &rules,
        &crate::sim::world::TypeTechLevels::default(),
        "Americans",
        0,
        HouseDifficulty::Normal,
        10,
        true,
        &mut expected_rng,
    );
    assert_eq!(actual_plan.nodes(), expected_plan.nodes());
    assert_eq!(actual_plan.percent_built(), expected_plan.percent_built());
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected_rng.logical_state()
    );
}

#[test]
fn scenario_prefix_transfers_once_before_terrain() {
    let seed = 0x51C0_1004;
    let before = SimRng::new(u64::from(seed));
    let start = Waypoint {
        index: 0,
        rx: 40,
        ry: 40,
    };
    let map = one_start_prefix_map(start);
    let launch = one_player_battle_launch("mp01t4.map");
    let plan = prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed)
        .expect("stock Battle prefix");
    let after = plan.scenario_rng_after_cursor.clone();
    let mut owner = ScenarioBootstrapRng::new(seed);

    owner
        .install_pre_fill_scenario_prefix_plan(plan)
        .expect("fresh bootstrap owner accepts the plan prefix");
    let actual = owner.into_simulation(&descriptor(seed)).rng_state();

    assert_eq!(actual.scenario, after.logical_state());
    assert_eq!(actual.main, before.logical_state());
}

#[test]
fn scenario_prefix_rejects_a_tampered_prestate_before_transfer() {
    let seed = 0x51C0_1006;
    let start = Waypoint {
        index: 0,
        rx: 40,
        ry: 40,
    };
    let map = one_start_prefix_map(start);
    let launch = one_player_battle_launch("mp01t4.map");
    let plan = prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed)
        .expect("stock Battle prefix");
    let mut owner = ScenarioBootstrapRng::new(seed ^ 1);

    assert!(matches!(
        owner.install_pre_fill_scenario_prefix_plan(plan),
        Err(PreFillScenarioPrefixPlanError::ScenarioRngPrestateMismatch { .. })
    ));
}

#[test]
fn staged_simulation_owns_fill_and_generated_replay_without_reconstruction() {
    use crate::map::construction_trace::{RmgConstructionPhase, RmgConstructionTrace};

    let seed = 0x51C0_1005;
    let mut reference = SimRng::new(u64::from(seed));
    let owner = ScenarioBootstrapRng::new(seed);
    let mut sim = owner.into_simulation(&descriptor(seed));
    // This isolated RNG fixture omits the full native Rules prefix.
    sim.native_unique_ids =
        Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
    sim.session.set_binary_frame_for_test(77);
    let retained_handle = sim.substrate.allocate_stable_id();

    {
        let (mut scenario_fill, variant_main) = sim.terrain_load_draws();
        assert_eq!(
            scenario_fill.next_range_u32_inclusive(5, 17),
            reference.next_range_u32_inclusive(5, 17),
        );
        let _ = variant_main.next_u32();
    }

    let mut trace = RmgConstructionTrace::default();
    trace.push_emitted(
        RmgConstructionPhase::BridgeRepairHut,
        "CABHUT".to_string(),
        0,
        (10, 11),
    );
    let expected_word = (reference.next_u32() & 0xFFFF) as u16;
    let bindings = sim
        .replay_staged_generated_construction_trace(&trace)
        .expect("staged owner replays the accepted constructor event");

    assert_eq!(
        bindings
            .entry(0)
            .expect("emitted constructor binding")
            .techno_ctor_random_word,
        expected_word,
    );
    assert_eq!(sim.rng_state().scenario, reference.logical_state());
    assert_eq!(sim.session.binary_frame(), 77);
    assert_eq!(retained_handle, 1);
    assert_eq!(sim.substrate.allocate_stable_id(), 2);
}

#[test]
fn gsi_04_12_prefix_fill_and_generated_trace_share_one_scenario_owner() {
    use crate::map::construction_trace::{RmgConstructionPhase, RmgConstructionTrace};

    let seed = 0x51C0_1006;
    let before = SimRng::new(u64::from(seed));
    let start = Waypoint {
        index: 0,
        rx: 40,
        ry: 40,
    };
    let map = one_start_prefix_map(start);
    let launch = one_player_battle_launch("RandMap.SED");
    let plan = prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, seed)
        .expect("generated stock prefix");
    let mut reference = plan.scenario_rng_after_cursor.clone();
    let mut owner = ScenarioBootstrapRng::new(seed);
    owner
        .install_pre_fill_scenario_prefix_plan(plan)
        .expect("fresh launch owner accepts the Full-Init prefix");
    // The match load stages the Simulation here, before Fill.
    let mut sim = owner.into_simulation(&descriptor(seed));
    // This isolated RNG fixture omits the full native Rules prefix.
    sim.native_unique_ids =
        Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
    {
        let (mut scenario_fill, _main) = sim.terrain_load_draws();
        let actual = scenario_fill.next_range_u32_inclusive(5, 17);
        let expected = reference.next_range_u32_inclusive(5, 17);
        assert_eq!(actual, expected);
    }

    let mut trace = RmgConstructionTrace::default();
    trace.push_emitted(
        RmgConstructionPhase::BridgeRepairHut,
        "CABHUT".to_string(),
        0,
        (10, 11),
    );
    trace.push_discarded(RmgConstructionPhase::NeutralTech, "CAOILD".to_string());
    trace.push_emitted(
        RmgConstructionPhase::NeutralTech,
        "CATHOSP".to_string(),
        2,
        (12, 13),
    );
    let first_word = (reference.next_u32() & 0xFFFF) as u16;
    let _discarded_word = reference.next_u32();
    let second_word = (reference.next_u32() & 0xFFFF) as u16;

    let bindings = sim
        .replay_staged_generated_construction_trace(&trace)
        .expect("ordered trace binds emitted constructors");
    assert_eq!(
        bindings
            .entry(0)
            .expect("CABHUT binding")
            .techno_ctor_random_word,
        first_word
    );
    assert_eq!(
        bindings
            .entry(2)
            .expect("neutral-tech binding")
            .techno_ctor_random_word,
        second_word
    );
    assert!(bindings.entry(1).is_none(), "discarded row binds no entity");

    let actual = sim.rng_state();
    assert_eq!(actual.scenario, reference.logical_state());
    assert_eq!(actual.main, before.logical_state());
}

#[test]
fn gsi_04_12_generated_trace_continues_into_starting_techno_and_post_map_crate() {
    use crate::map::construction_trace::{RmgConstructionPhase, RmgConstructionTrace};
    use crate::sim::scenario_post_map::ScenarioPostMapInput;

    const SIZE: u16 = 80;
    let seed = 0x51C0_100B;
    let ini = launch_fixture_ini(
        "[General]\n\
         BaseUnit=MTNK\n\
         [InfantryTypes]\n\
         [VehicleTypes]\n\
         0=MTNK\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         0=CABHUT\n\
         1=CATHOSP\n\
         [MTNK]\n\
         Strength=300\n\
         Speed=6\n\
         Cost=100\n\
         TechLevel=1\n\
         Owner=Americans\n\
         [CABHUT]\n\
         Strength=100\n\
         Foundation=1x1\n\
         Owner=Neutral\n\
         [CATHOSP]\n\
         Strength=100\n\
         Foundation=1x1\n\
         Owner=Neutral\n\
         [OverlayTypes]\n\
         0=WOOD\n\
         1=WATER\n\
         [WOOD]\n\
         Crate=yes\n\
         [WATER]\n\
         Crate=yes\n\
         [CrateRules]\n\
         CrateImg=WOOD\n\
         WoodCrateImg=WOOD\n\
         WaterCrateImg=WATER\n\
         CrateMinimum=1\n\
         CrateMaximum=1\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("single-owner launch rules");
    let overlays = OverlayTypeRegistry::from_ini(&ini, None);
    let map = MapFile::from_bytes(
        b"[Map]\nTheater=TEMPERATE\nSize=0,0,40,40\nLocalSize=2,2,36,32\n\
          [Basic]\nName=GSI 04.12 owner fixture\nNewINIFormat=4\n\
          [Structures]\n\
          0=Neutral,CABHUT,256,30,30,0,None\n\
          1=Neutral,CATHOSP,256,32,32,0,None\n\
          [IsoMapPack5]\n1=CAAEABUAAAAAEQAA\n",
    )
    .expect("minimal generated-map-shaped input");
    let mut session = crate::headless_scenario::battle_session("RandMap.SED", &[]);
    session.local_slot_mut().start_position = LaunchStartPosition::Position(1);
    session.options.unit_count = 0;
    session.options.crates = true;
    session.options.bases = true;
    let launch = MatchLaunchDescriptor::from_resolved(session)
        .expect("fixture contains no unresolved shell choices");

    let start = Waypoint {
        index: 0,
        rx: 40,
        ry: 40,
    };
    let before = SimRng::new(u64::from(seed));
    let staged_starts = HashMap::from([(0, start)]);
    let plan = prepare_stock_offline_scenario_prefix_plan(&launch, &map, &staged_starts, seed)
        .expect("accepted generated prefix");
    let mut reference = plan.scenario_rng_after_cursor.clone();
    let mut owner = ScenarioBootstrapRng::new(seed);
    let projection = owner
        .install_pre_fill_scenario_prefix_plan(plan)
        .expect("one authoritative Full-Init prefix");
    // The match load stages the Simulation here, before Fill.
    let mut scenario = descriptor(seed);
    scenario.map_name = "RandMap.SED".to_string();
    scenario.theater = "TEMPERATE".to_string();
    scenario.rule_policy = crate::rules::rule_policy::RulePolicy::stock_multiplayer();
    scenario.map_width = SIZE;
    scenario.map_height = SIZE;
    scenario.mp_start_waypoints.insert(0, (start.rx, start.ry));
    let mut sim = owner.into_simulation(&scenario);
    // This isolated RNG fixture omits the full native Rules prefix.
    sim.native_unique_ids =
        Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
    {
        let (mut scenario_fill, _main) = sim.terrain_load_draws();
        let actual = scenario_fill.next_range_u32_inclusive(5, 17);
        let expected = reference.next_range_u32_inclusive(5, 17);
        assert_eq!(actual, expected, "terrain Fill continues the prefix");
    }

    let mut trace = RmgConstructionTrace::default();
    trace.push_emitted(
        RmgConstructionPhase::BridgeRepairHut,
        "CABHUT".to_string(),
        0,
        (30, 30),
    );
    trace.push_discarded(RmgConstructionPhase::NeutralTech, "CAOILD".to_string());
    trace.push_emitted(
        RmgConstructionPhase::NeutralTech,
        "CATHOSP".to_string(),
        1,
        (32, 32),
    );
    let first_trace_word = (reference.next_u32() & 0xFFFF) as u16;
    let _discarded_trace_word = reference.next_u32();
    let second_trace_word = (reference.next_u32() & 0xFFFF) as u16;
    let bindings = sim
        .replay_staged_generated_construction_trace(&trace)
        .expect("ordered trace binds emitted constructors");
    assert_eq!(
        bindings.entry(0).unwrap().techno_ctor_random_word,
        first_trace_word
    );
    assert_eq!(
        bindings.entry(1).unwrap().techno_ctor_random_word,
        second_trace_word
    );
    assert!(bindings.entry(2).is_none());
    assert_eq!(
        sim.rng_state().scenario,
        reference.logical_state(),
        "the independently walked intermediate cursor must match after replay"
    );

    let terrain = techno_constructor_flat_start_terrain(SIZE);
    sim.install_resolved_terrain_for_new_map(terrain.clone());
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(SIZE, SIZE));
    sim.install_playfield_from_map_header(&map.header);
    let house_roster = HouseRoster::default();
    initialize_skirmish_launch_houses(&mut sim, &house_roster, &rules, &launch);
    let before_projection = sim.scenario_rng.logical_state();
    assert_eq!(
        sim.spawn_generated_from_map_with_resolved(
            &map.entities,
            &rules,
            Some(&terrain),
            &bindings,
        )
        .expect("generated projection validates the replay bindings"),
        2
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        before_projection,
        "generated projection must install preconsumed words without drawing"
    );
    let installed_words = [("CABHUT", first_trace_word), ("CATHOSP", second_trace_word)];
    for (type_id, expected_word) in installed_words {
        let entity = sim
            .entities()
            .values()
            .find(|entity| sim.interner.resolve(entity.type_ref()) == type_id)
            .unwrap_or_else(|| panic!("generated {type_id} reached Simulation"));
        assert_eq!(entity.techno_ctor_random_word(), expected_word);
    }

    let starting_techno_word = (reference.next_u32() & 0xFFFF) as u16;
    let _starting_force_tail = reference.next_range_u32_inclusive(0, 0xFFFF);
    let launch_result = apply_pre_fill_scenario_prefix_launch_session(
        &mut sim,
        &map,
        &house_roster,
        &rules,
        &terrain,
        &launch,
        &projection,
    );
    assert_eq!(launch_result.spawned_mcvs, 1);
    let starting_techno = sim
        .entities()
        .values()
        .find(|entity| sim.interner.resolve(entity.type_ref()) == "MTNK")
        .expect("the production launch constructs its starting MCV");
    assert_eq!(
        starting_techno.techno_ctor_random_word(),
        starting_techno_word,
        "the first post-trace Techno constructor continues the same cursor"
    );

    let expected_crate_cell = (
        reference.next_range_u32_inclusive(1, u32::from(SIZE - 1)) as u16,
        reference.next_range_u32_inclusive(1, u32::from(SIZE - 1)) as u16,
    );
    assert!(
        crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (
                i32::from(expected_crate_cell.0),
                i32::from(expected_crate_cell.1),
            ),
            sim.playfield_bounds(),
            Some(&terrain),
        ),
        "controlled first crate draw must need no FNPC displacement"
    );
    let _crate_timer = reference.next_range_u32_inclusive(0, 0x7FFF_FFFE);
    // The queues are built where the load builds them, before the post-map
    // tail, which leaves them alone.
    let _ = crate::sim::runtime::initialize_native_tiberium_queues(
        &mut sim,
        &map.basic,
        &rules,
        &overlays,
        (SIZE, SIZE),
    );
    let post_map = |construction_rules| ScenarioPostMapInput {
        map_width: SIZE,
        map_height: SIZE,
        fill_silos: false,
        rules: &rules,
        overlay_registry: &overlays,
        house_roster: &house_roster,
        construction_rules,
        skirmish_session: Some(&launch),
        campaign_map: None,
        stage_home_global: None,
    };
    let stock = sim.session.rule_policy();
    let no_slots = sim
        .clone()
        .finalize_scenario_post_map(post_map(stock.with_uses_match_crates(false)))
        .unwrap();
    assert!(no_slots.crates.is_none());
    let output = sim.finalize_scenario_post_map(post_map(stock)).unwrap();
    assert_eq!(
        output.crates,
        Some(crate::sim::crates::CratePlacement {
            requested: 1,
            accepted: 1,
            visible: 1,
        })
    );
    let wood = overlays.id_for_name("WOOD").expect("WOOD crate overlay");
    let crate_cells = sim
        .overlay_grid
        .as_ref()
        .expect("post-map overlay authority")
        .iter_occupied()
        .filter_map(|(rx, ry, cell)| (cell.overlay_id == Some(wood)).then_some((rx, ry)))
        .collect::<Vec<_>>();
    assert_eq!(crate_cells, vec![expected_crate_cell]);
    assert_eq!(
        sim.scenario_rng.logical_state(),
        reference.logical_state(),
        "prefix, Fill, trace, starting Techno, and crate draws must form one stream"
    );
    assert_eq!(sim.main_rng.logical_state(), before.logical_state());
}

#[test]
fn generated_trace_rejects_bad_ordinal_before_spending_scenario() {
    let seed = 0x51C0_1007;
    let mut sim = ScenarioBootstrapRng::new(seed).into_simulation(&descriptor(seed));
    let mut trace = crate::map::construction_trace::RmgConstructionTrace::default();
    trace.push_discarded(
        crate::map::construction_trace::RmgConstructionPhase::NeutralTech,
        "CAOILD".to_string(),
    );
    trace.events[0].ordinal = 4;

    assert!(matches!(
        sim.replay_staged_generated_construction_trace(&trace),
        Err(
            crate::sim::world::GeneratedTechnoInitError::TraceOrdinalMismatch {
                expected: 0,
                found: 4,
            }
        )
    ));
    assert_eq!(
        sim.rng_state().scenario,
        SimRng::new(u64::from(seed)).logical_state()
    );
}

/// Two humans then two AIs, in the House order CreateHouses builds.
fn quick_coop_session(
    humans: [LaunchStartPosition; 2],
    ais: [LaunchStartPosition; 2],
) -> SkirmishLaunchSession {
    let mut session = one_player_battle_launch("quick-coop.map").session().clone();
    let mut partner = session.humans[0].clone();
    partner.raw_slot_index = Some(1);
    partner.name = "Partner".to_string();
    partner.slot.color_index = 1;
    session.humans.push(partner);
    for (human, start) in session.humans.iter_mut().zip(humans) {
        human.slot.start_position = start;
    }
    session.opponents = ais
        .into_iter()
        .enumerate()
        .map(
            |(index, start_position)| crate::skirmish_launch::SkirmishAiSlot {
                raw_slot_index: index as u8 + 2,
                country: LaunchCountry::Russia,
                country_random: false,
                color_index: index as u8 + 2,
                color_random: false,
                start_position,
                team: LaunchTeam::None,
                difficulty: crate::skirmish_launch::AiDifficulty::Easy,
            },
        )
        .collect();
    session.ai_player_count = 2;
    use crate::skirmish_launch::{PreFillAiHouseSlot, PreFillHouseRoster, PreFillHumanHouse};
    session.set_recorded_house_roster_for_test(PreFillHouseRoster::new(
        (0..2)
            .map(|order| PreFillHumanHouse {
                priority: i32::from(order),
                source_order: order,
                observer: false,
            })
            .collect(),
        (0..8)
            .map(|slot_index| PreFillAiHouseSlot {
                slot_index,
                valid: matches!(slot_index, 2 | 3),
            })
            .collect(),
    ));
    session
}

/// Starts on one row at x = 10, 20, 40, 80, 100.
fn quick_coop_row() -> Vec<Waypoint> {
    [10, 20, 40, 80, 100]
        .into_iter()
        .enumerate()
        .map(|(index, rx)| Waypoint {
            index: index as u32,
            rx,
            ry: 10,
        })
        .collect()
}

fn quick_coop_assign(session: &SkirmishLaunchSession, rng: &mut SimRng) -> Vec<(u16, usize)> {
    let starts = quick_coop_row();
    let preassignment = native_preassign_launch_start_table(session, starts.len());
    let assignment =
        native_assign_quick_coop_starts_from_preassignment(session, &starts, &preassignment, rng)
            .unwrap();
    assignment
        .placements
        .iter()
        .map(|(slot, start)| (start.rx, *slot))
        .collect()
}

#[test]
fn quick_coop_starts_seat_humans_first_and_ignore_preset_ai_starts() {
    use LaunchStartPosition::{Auto, Position};
    // Human A keeps preset start 2. With 2 and AI 1's preset 4 used,
    // human B takes the farthest free start (x 10). AI 1 then picks as
    // if it had no preset: farthest from 10, 40 and 100 is x 80. AI 2
    // takes the last free start.
    let session = quick_coop_session([Position(2), Auto], [Position(4), Auto]);
    let mut rng = SimRng::new(0x5EE9);
    let before = rng.state();
    let mut placements = quick_coop_assign(&session, &mut rng);
    placements.sort_by_key(|(_, slot)| *slot);
    assert_eq!(placements, vec![(40, 0), (10, 1), (80, 2), (20, 3)]);
    assert_eq!(rng.state(), before, "no picker branch here draws");

    let starts = quick_coop_row();
    let preassignment = native_preassign_launch_start_table(&session, starts.len());
    let assignment = native_assign_quick_coop_starts_from_preassignment(
        &session,
        &starts,
        &preassignment,
        &mut rng,
    )
    .unwrap();
    assert_eq!(
        assignment.start_table,
        vec![Some(1), Some(3), Some(0), Some(2), Some(2)]
    );
}

#[test]
fn quick_coop_starts_draw_for_the_first_human_and_the_first_ai() {
    use LaunchStartPosition::Auto;
    let session = quick_coop_session([Auto, Auto], [Auto, Auto]);
    let nearest = |rx: u16| match rx {
        10 => 20,
        20 => 10,
        40 => 20,
        80 => 100,
        100 => 80,
        _ => unreachable!(),
    };
    for seed in 0..32 {
        let mut rng = SimRng::new(seed);
        let mut expected = rng.clone();
        let first = expected.next_range_u32_inclusive(0, 4) as usize;
        let ai_draw = expected.next_range_u32_inclusive(0, 2) as usize;
        let placements = quick_coop_assign(&session, &mut rng);
        assert_eq!(rng.state(), expected.state(), "seed {seed}: two draws");

        let row: Vec<u16> = quick_coop_row().iter().map(|start| start.rx).collect();
        let start_of = |slot: usize| placements.iter().find(|(_, s)| *s == slot).unwrap().0;
        assert_eq!(start_of(0), row[first]);
        assert_eq!(start_of(1), nearest(row[first]));
        let free: Vec<u16> = row
            .iter()
            .copied()
            .filter(|rx| *rx != start_of(0) && *rx != start_of(1))
            .collect();
        assert_eq!(start_of(2), free[ai_draw]);
    }
}

#[test]
fn quick_coop_picker_third_house_of_three_takes_the_last_start_without_a_draw() {
    // N = 3, k = 2, AI: RandomRanged(0, 0) returns at min == max
    // (0x0065C7EF) without consuming the stream.
    let starts = &quick_coop_row()[..3];
    let mut rng = SimRng::new(9);
    let before = rng.state();
    assert_eq!(
        choose_quick_coop_start(starts, &[true, true, false], false, &mut rng),
        Some(2)
    );
    assert_eq!(rng.state(), before);
}

#[test]
fn quick_coop_picker_gives_an_ai_on_an_empty_table_the_first_start() {
    let starts = quick_coop_row();
    let mut rng = SimRng::new(7);
    let before = rng.state();
    assert_eq!(
        choose_quick_coop_start(&starts, &[false; 5], false, &mut rng),
        Some(0)
    );
    assert_eq!(rng.state(), before);
}

#[test]
fn quick_coop_starts_replace_the_mode_chooser_in_the_prefix() {
    use LaunchStartPosition::Auto;
    let session = quick_coop_session([Auto, Auto], [Auto, Auto]);
    let row: Vec<Waypoint> = [6, 12, 20, 30, 38]
        .into_iter()
        .enumerate()
        .map(|(index, rx)| Waypoint {
            index: index as u32,
            rx,
            ry: 20,
        })
        .collect();
    let map = prefix_map_with_starts(&row);
    let battle = MatchLaunchDescriptor::from_resolved(session).unwrap();
    let quick_coop = battle.clone().with_quick_coop_starts();
    for seed in 0x5EE0..0x5EF0 {
        for (launch, partner_nearest) in [(&battle, false), (&quick_coop, true)] {
            let plan =
                prepare_stock_offline_scenario_prefix_plan(launch, &map, &map.waypoints, seed)
                    .unwrap();
            let starts = plan.final_gathered_starts();
            let placements = &plan.projection.assignment.placements;
            let start_of = |slot: usize| {
                placements
                    .iter()
                    .find(|(placed, _)| *placed == slot)
                    .unwrap()
                    .1
            };
            let first = start_of(0);
            let others = || {
                starts
                    .iter()
                    .copied()
                    .filter(move |start| *start != first)
                    .map(move |start| (native_start_distance(first, start), start))
            };
            // Battle seats the second house farthest from the first;
            // AssignStartingPoints seats the partner nearest.
            let expected = if partner_nearest {
                others().min_by_key(|(distance, _)| *distance)
            } else {
                others().max_by_key(|(distance, _)| *distance)
            };
            assert_eq!(
                native_start_distance(first, start_of(1)),
                expected.unwrap().0,
                "seed {seed:#x}, quick co-op {partner_nearest}"
            );
        }
    }
}

/// Prepare a launch prefix on another thread and return its refusal. A start
/// assignment that loops forever fails the test after a minute instead of
/// hanging the run; the longest refusal takes a few seconds on a busy machine.
fn prefix_refusal_before_hang_guard(
    fixture: impl FnOnce() -> (MatchLaunchDescriptor, MapFile) + Send + 'static,
) -> PreFillScenarioPrefixPlanError {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (launch, map) = fixture();
        let refusal =
            prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, 0x5C2E).err();
        let _ = sender.send(refusal);
    });
    receiver
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("start assignment finishes")
        .expect("the launch is refused")
}

fn four_start_row_map() -> MapFile {
    let row: Vec<Waypoint> = [6, 12, 20, 30]
        .into_iter()
        .enumerate()
        .map(|(index, rx)| Waypoint {
            index: index as u32,
            rx,
            ry: 20,
        })
        .collect();
    prefix_map_with_starts(&row)
}

#[test]
fn quick_coop_refuses_a_launch_whose_preset_ai_start_leaves_an_ai_no_start() {
    use LaunchStartPosition::{Auto, Position};
    // Four starts for four players. AI 1's preset start 3 stays reserved
    // while AI 1 picks another, so AI 2 finds every start used.
    let session = quick_coop_session([Position(2), Auto], [Position(3), Auto]);
    let launch = MatchLaunchDescriptor::from_resolved(session)
        .unwrap()
        .with_quick_coop_starts();
    let map = four_start_row_map();
    assert_eq!(
        prepare_stock_offline_scenario_prefix_plan(&launch, &map, &map.waypoints, 0x5EE9)
            .unwrap_err(),
        PreFillScenarioPrefixPlanError::QuickCoopStartsExhausted { start_count: 4 }
    );
}

#[test]
fn cooperative_refuses_more_humans_than_the_maps_human_start_spots() {
    let refusal = prefix_refusal_before_hang_guard(|| {
        use LaunchStartPosition::Auto;
        let mut session = quick_coop_session([Auto, Auto], [Auto, Auto]);
        session.mode.class = crate::skirmish_modes::MpModeClass::Cooperative;
        let mut map = four_start_row_map();
        map.ini = IniFile::from_str("[Header]\nNumCoopHumanStartSpots=1\n");
        (MatchLaunchDescriptor::from_resolved(session).unwrap(), map)
    });
    assert_eq!(
        refusal,
        PreFillScenarioPrefixPlanError::CoopHumanStartSpotsFull {
            human_start_spots: 1,
            human_count: 2,
        }
    );
}

#[test]
fn cooperative_refuses_a_launch_whose_observer_takes_the_last_start() {
    let refusal = prefix_refusal_before_hang_guard(|| {
        use LaunchStartPosition::Auto;
        // One human, one observer, two AI: Gather keeps the three authored
        // starts for the three Houses that play, and the observer takes one.
        let mut session = quick_coop_session([Auto, Auto], [Auto, Auto]);
        session.mode.class = crate::skirmish_modes::MpModeClass::Cooperative;
        session.humans[1].set_observer(true);
        use crate::skirmish_launch::{PreFillAiHouseSlot, PreFillHouseRoster, PreFillHumanHouse};
        session.set_recorded_house_roster_for_test(PreFillHouseRoster::new(
            (0..2)
                .map(|order| PreFillHumanHouse {
                    priority: i32::from(order),
                    source_order: order,
                    observer: order == 1,
                })
                .collect(),
            (0..8)
                .map(|slot_index| PreFillAiHouseSlot {
                    slot_index,
                    valid: matches!(slot_index, 2 | 3),
                })
                .collect(),
        ));
        let starts: Vec<Waypoint> = [6, 20, 30]
            .into_iter()
            .enumerate()
            .map(|(index, rx)| Waypoint {
                index: index as u32,
                rx,
                ry: 20,
            })
            .collect();
        let mut map = prefix_map_with_starts(&starts);
        map.ini = IniFile::from_str("[Header]\nNumCoopHumanStartSpots=1\n");
        (MatchLaunchDescriptor::from_resolved(session).unwrap(), map)
    });
    assert_eq!(
        refusal,
        PreFillScenarioPrefixPlanError::CoopStartsExhausted {
            start_count: 3,
            house_count: 4,
        }
    );
}

#[test]
fn gather_refuses_a_map_with_no_room_for_a_missing_start() {
    let refusal = prefix_refusal_before_hang_guard(|| {
        // No authored start, and a zero-width LocalSize leaves no playfield
        // cell for the 8x8 rect's corner.
        let mut map = prefix_map_with_starts(&[]);
        map.header.local_width = 0;
        (one_player_battle_launch("no-room.map"), map)
    });
    assert_eq!(
        refusal,
        PreFillScenarioPrefixPlanError::NoRoomForStarts {
            found: 0,
            required: 1,
        }
    );
}
