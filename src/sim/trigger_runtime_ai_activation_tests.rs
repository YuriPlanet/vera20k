//! Trigger controls through the shared House activation and ordinary team owners.
use super::*;
use crate::sim::house_state::HouseAiActivationLatches;
use crate::util::save_codec;

#[test]
fn authored_ai_trigger_actions_change_only_the_selected_house_latch() {
    for (kind, initial, expected) in [
        (75, true, false),
        (74, false, true),
        (13, false, true),
        (13, true, true),
    ] {
        let (mut sim, rules, registry, houses) = super::tests::event_ingress_fixture();
        let priors = HouseAiActivationLatches {
            production: true,
            autocreate_allowed: kind == 13 && initial,
            ai_triggers_active: initial,
            auto_base_building: true,
        };
        for owner in houses {
            sim.houses
                .get_mut(&owner)
                .unwrap()
                .set_ai_activation_for_test(priors);
        }
        // Incoming House is Country0; the action's Country7 operand selects
        // House1 instead. 6E45E0 is the existing selector for both actions.
        super::tests::install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nAI=Country0,<none>,AI,0,1,1,1,0\n\
                [Events]\nAI=1,13,0,0\n\
                [Actions]\nAI=1,{kind},0,7,0,0,0,0,A\n\
                [Tags]\nAITAG=0,AI,AI\n"
            ),
        );
        let timers: Vec<_> = houses
            .iter()
            .map(|owner| {
                let house = &sim.houses[owner];
                (house.team_creation.timer(), house.strategy_timer)
            })
            .collect();
        let rng_before = (
            sim.scenario_rng.native_state_hex(),
            sim.main_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        );
        let _ = sim.advance_triggers(Some(&rules), Some(&registry), FrameEffects::default());
        for (index, owner) in houses.iter().enumerate() {
            let house = &sim.houses[owner];
            let wanted = HouseAiActivationLatches {
                ai_triggers_active: if index == 1 && kind != 13 {
                    expected
                } else {
                    initial
                },
                autocreate_allowed: if index == 1 && kind == 13 {
                    true
                } else {
                    priors.autocreate_allowed
                },
                ..priors
            };
            assert_eq!(house.ai_activation(), wanted, "Action{kind}, House{index}");
            assert_eq!(
                (house.team_creation.timer(), house.strategy_timer),
                timers[index],
                "Action{kind} does not reset House timers"
            );
        }
        assert_eq!(
            (
                sim.scenario_rng.native_state_hex(),
                sim.main_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex()
            ),
            rng_before,
            "Action{kind} does not draw RNG"
        );
    }
}

fn corpus() -> &'static serde_json::Value {
    let value = crate::test_fixture::json("tools/spatial_oracle/trigger_ai_activation.json");
    assert_eq!(
        value["native_sha256"],
        "3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600"
    );
    assert_eq!(value["actions"]["controls"].as_array().unwrap().len(), 26);
    assert_eq!(value["histories"].as_array().unwrap().len(), 14);
    value
}

fn number(value: &serde_json::Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn latches(value: &serde_json::Value) -> HouseAiActivationLatches {
    let bit = |index: usize| number(&value[index]) != 0;
    HouseAiActivationLatches {
        production: bit(0),
        autocreate_allowed: bit(1),
        ai_triggers_active: bit(2),
        auto_base_building: bit(3),
    }
}

fn supply_rng(sim: &mut Simulation, states: &serde_json::Value) {
    use crate::sim::rng::SimRng;
    sim.scenario_rng = SimRng::from_native_state_hex_for_test(states["scenario"].as_str().unwrap());
    sim.main_rng = SimRng::from_native_state_hex_for_test(states["main"].as_str().unwrap()).into();
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(states["mapgen"].as_str().unwrap());
}

fn assert_rng(sim: &Simulation, states: &serde_json::Value, context: &str) {
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        states["scenario"],
        "{context}: Scenario RNG"
    );
    assert_eq!(
        sim.main_rng.native_state_hex(),
        states["main"],
        "{context}: Main RNG"
    );
    assert_eq!(
        sim.mapgen_rng.native_state_hex(),
        states["mapgen"],
        "{context}: MapGen RNG"
    );
}

fn assert_house_summary(
    sim: &Simulation,
    owner: crate::sim::intern::InternedId,
    native: &serde_json::Value,
    context: &str,
) {
    let house = &sim.houses[&owner];
    assert_eq!(
        house.ai_activation(),
        latches(&native["latches"]),
        "{context}: activation bytes"
    );
    let timer = house.team_creation.timer();
    // CdTimer retains the two consumed words. Native's unused middle dword
    // is recorded in the corpus, not duplicated as simulation state.
    assert_eq!(
        [timer.start_frame(), timer.duration()],
        [
            number(&native["team_timer"][0]),
            number(&native["team_timer"][2])
        ],
        "{context}: team timer"
    );
}

fn run_native_action(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    houses: [crate::sim::intern::InternedId; 3],
    row: &serde_json::Value,
) {
    let input = &row["input"];
    let context = format!("Action{} {}", row["kind"], input["name"]);
    sim.houses
        .reorder_for_test(&input["order"].as_array().map_or_else(
            || houses.to_vec(),
            |order| {
                order
                    .iter()
                    .map(|index| houses[index.as_u64().unwrap() as usize])
                    .collect()
            },
        ));
    // Original Scenario+1180 storage supplied by the native fixture: slot0
    // resolves registered House1, slot1 House0, and slots2..7 are absent.
    sim.session.start_slot_houses_mut_for_test().clear();
    sim.session
        .start_slot_houses_mut_for_test()
        .insert(0, houses[1]);
    sim.session
        .start_slot_houses_mut_for_test()
        .insert(1, houses[0]);
    super::tests::install_event_ingress_map(
        sim,
        rules,
        &format!(
            "[Triggers]\nAI=Country0,<none>,AI,0,1,1,1,0\n\
            [Events]\nAI=1,8,0,0\n\
            [Actions]\nAI=1,{}\n[Tags]\nAITAG=2,AI,AI\n",
            row["native_read"]["raw"].as_str().unwrap()
        ),
    );
    let live = sim.trigger_runtime.trigger_order[0];
    sim.trigger_runtime.triggers[live]
        .as_mut()
        .unwrap()
        .retained_house = match input.get("trigger_owner") {
        None => Some(houses[2]),
        Some(value) => value.as_u64().map(|index| houses[index as usize]),
    };
    let typ = sim.trigger_runtime.trigger(live).unwrap().type_index;
    let action = sim.trigger_runtime.definitions.trigger_types[typ].actions[0].clone();
    assert_eq!(action.entry.kind, number(&row["native_read"]["kind"]));
    assert_eq!(action.value, Some(number(&row["native_read"]["value"])));
    let incoming_house = match input.get("incoming") {
        None => Some(houses[1]),
        Some(value) => value.as_u64().map(|index| houses[index as usize]),
    };
    for (index, owner) in houses.iter().enumerate() {
        assert_house_summary(sim, *owner, &row["houses_before"][index], &context);
    }
    let before: Vec<_> = houses
        .iter()
        .map(|owner| serde_json::to_value(&sim.houses[owner]).unwrap())
        .collect();
    let teams_before: Vec<_> = sim
        .team_script_vm
        .teams_in_order()
        .map(|team| save_codec::serialize(team).unwrap())
        .collect();
    let rng_before = sim.rng_state();
    let result = sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: (input["null_trigger"].as_bool() != Some(true)).then_some(live),
            incoming_house,
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        Some(rules),
        None,
        FrameEffects::default(),
    );
    assert_eq!(result, number(&row["result"]) != 0, "{context}: AL");
    for (index, owner) in houses.iter().enumerate() {
        let native = &row["houses_after"][index];
        assert_house_summary(sim, *owner, native, &context);
        let mut expected = before[index].clone();
        expected["ai_activation"] = serde_json::to_value(latches(&native["latches"])).unwrap();
        assert_eq!(
            serde_json::to_value(&sim.houses[owner]).unwrap(),
            expected,
            "{context}: all other retained House state"
        );
    }
    assert_eq!(sim.rng_state(), rng_before, "{context}: no draw");
    let teams_after: Vec<_> = sim
        .team_script_vm
        .teams_in_order()
        .map(|team| save_codec::serialize(team).unwrap())
        .collect();
    assert_eq!(teams_after, teams_before, "{context}: existing Teams");
    assert_eq!(row["rng_unchanged"], true);
    assert_eq!(row["existing_team_unchanged"], true);
}

fn install_team_scene(
    sim: &mut Simulation,
    owner: crate::sim::intern::InternedId,
) -> (u64, [crate::sim::intern::InternedId; 2]) {
    use crate::rules::{locomotor_type::MovementZone, team_ai_ini::TeamAiDefinitionSource};
    use crate::sim::team_script_vm::{
        TeamScriptDefinition, TeamTaskForceDefinition, TeamTypeDefinition,
    };
    let script = sim.interner.intern("SCRIPT");
    let task_force = sim.interner.intern("TASKFORCE");
    sim.team_script_vm.register_script(TeamScriptDefinition {
        id: script,
        actions: Vec::new(),
        source: TeamAiDefinitionSource::FixedAimd,
    });
    sim.team_script_vm
        .register_task_force(TeamTaskForceDefinition {
            id: task_force,
            group: -1,
            entries: Vec::new(),
            source: TeamAiDefinitionSource::FixedAimd,
        });
    let team_types = [sim.interner.intern("T0"), sim.interner.intern("T1")];
    for (index, id) in team_types.iter().copied().enumerate() {
        sim.team_script_vm.register_team_type(TeamTypeDefinition {
            id,
            script_id: script,
            task_force_id: task_force,
            priority: 0,
            is_base_defense: index == 1,
            suicide: false,
            aggressive: false,
            combined_movement_zone: MovementZone::Normal,
            base_zone_relation_enforced: false,
            transport_crossing_required: false,
        });
    }
    let trigger = sim.interner.intern("AI_TRIGGER");
    sim.team_script_vm
        .register_ai_trigger(crate::sim::ai_team_creation::tests::ai_trigger(
            trigger,
            team_types[0],
            None,
            crate::util::native_x87::NativeF64Bits::from_bits(100.0_f64.to_bits()),
        ));
    let old_team = sim
        .team_script_vm
        .create_team_from_type(owner, team_types[1], &[], 7);
    (old_team, team_types)
}

#[test]
fn ai_activation_actions_match_all_original_selection_and_state_controls() {
    let native = corpus();
    for row in native["actions"]["controls"].as_array().unwrap() {
        let (mut sim, rules, _, houses) = super::tests::event_ingress_fixture();
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        supply_rng(&mut sim, &native["actions"]["rng_full_seed31"]);
        install_team_scene(&mut sim, houses[0]);
        for (index, owner) in houses.iter().enumerate() {
            let prior = &row["houses_before"][index];
            let house = sim.houses.get_mut(owner).unwrap();
            house.set_ai_activation_for_test(latches(&prior["latches"]));
            house.team_creation.restart(
                number(&prior["team_timer"][0]),
                number(&prior["team_timer"][2]),
            );
        }
        run_native_action(&mut sim, &rules, houses, row);
        assert_rng(
            &sim,
            &native["actions"]["rng_full_seed31"],
            "standalone action",
        );
    }
}

#[test]
fn ai_activation_replays_original_action_to_timer_selector_histories() {
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::house_state::{HouseDifficulty, HouseState};
    let native = corpus();
    for row in native["histories"].as_array().unwrap() {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        // These are declared native fixture priors, not retail tuning.
        let cap = input["cap"].as_i64().unwrap_or(8);
        let mut ini = format!(
            "[General]\nTeamDelays=300,600,900\nTotalAITeamCap={cap},{cap},{cap}\nMaximumAIDefensiveTeams=2,2,2\nMinimumAIDefensiveTeams=9,2,9\nUseMinDefenseRule=no\n[Countries]\n"
        );
        for index in 0..18 {
            ini.push_str(&format!("{index}=Country{index}\n"));
        }
        for index in 0..18 {
            ini.push_str(&format!("[Country{index}]\n"));
        }
        let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
        let mut sim = Simulation::new();
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
                input["game_mode"].as_i64() != Some(0),
            ),
        );
        let houses = [
            sim.interner.intern("H0"),
            sim.interner.intern("H1"),
            sim.interner.intern("H2"),
        ];
        let countries: Vec<_> = input["countries"]
            .as_array()
            .map_or(vec![0, 7, 0], |array| {
                array.iter().map(number).collect::<Vec<_>>()
            });
        for (index, owner) in houses.iter().copied().enumerate() {
            let country = sim.interner.intern(&format!("Country{}", countries[index]));
            let mut house = HouseState::new(
                owner,
                index as u8,
                Some(country),
                input["human"].as_bool().unwrap_or(false),
                0,
                10,
            );
            house.player_control = input["control"].as_bool().unwrap_or(false);
            house.multiplay_passive = input["passive"].as_bool().unwrap_or(false);
            house.difficulty = HouseDifficulty::from_native(1).unwrap();
            house.set_ai_activation_for_test(HouseAiActivationLatches {
                production: false,
                autocreate_allowed: true,
                ai_triggers_active: input["initial_active"].as_bool().unwrap_or(true),
                auto_base_building: false,
            });
            let timer = input["timer"].as_array();
            house.team_creation.restart(
                timer.map_or(0, |t| number(&t[0])),
                timer.map_or(10, |t| number(&t[2])),
            );
            sim.houses.register(house);
        }
        sim.houses.get_mut(&houses[0]).unwrap().enemy_house = Some(houses[1]);
        sim.houses.reorder_for_test(&houses.to_vec());
        supply_rng(&mut sim, &row["initial_rng_full"]);
        let (old_team, team_types) = install_team_scene(&mut sim, houses[0]);
        let old_bytes = save_codec::serialize(sim.team_script_vm.team(old_team).unwrap()).unwrap();
        for (step_index, step) in row["steps"].as_array().unwrap().iter().enumerate() {
            let context = format!("{name} step{step_index}");
            if let Some(action) = step.get("action") {
                run_native_action(&mut sim, &rules, houses, action);
                continue;
            }
            let update = &step["update"];
            let owner = houses[update["house"].as_u64().unwrap() as usize];
            assert_house_summary(&sim, owner, &update["before"], &context);
            sim.session
                .set_binary_frame_for_test(number(&update["frame"]) as u32);
            let before_ids: Vec<_> = sim
                .team_script_vm
                .teams_in_order()
                .map(|team| team.id())
                .collect();
            let mut draw_check = sim.scenario_rng.clone();
            for call in update["native_calls"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|call| call["name"] == "rng_ranged")
            {
                let low = number(&call["args"][0]);
                let high = number(&call["args"][1]);
                assert_eq!(
                    draw_check.next_range_i32_inclusive(low, high),
                    number(&call["return_eax"]),
                    "{context}: original ranged return"
                );
            }
            // Independently consume the original recorded raw words. In the
            // seed0 rejection witness one ranged request consumes TWO words.
            let mut raw_check = sim.scenario_rng.clone();
            for word in update["raw_rng_words"].as_array().unwrap() {
                assert_eq!(
                    raw_check.next_u32(),
                    word.as_u64().unwrap() as u32,
                    "{context}: raw word"
                );
            }
            crate::sim::ai_team_creation::update_team_creation(
                &mut sim,
                &rules,
                owner,
                FrameEffects::default(),
            );
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                update["scenario_rng_after_full"],
                "{context}: complete Scenario stream"
            );
            assert_eq!(
                draw_check.native_state_hex(),
                sim.scenario_rng.native_state_hex(),
                "{context}: request sequence"
            );
            assert_eq!(
                raw_check.native_state_hex(),
                sim.scenario_rng.native_state_hex(),
                "{context}: raw sequence"
            );
            assert_house_summary(&sim, owner, &update["after"], &context);
            let actual_created: Vec<_> = sim
                .team_script_vm
                .teams_in_order()
                .filter(|team| !before_ids.contains(&team.id()))
                .map(|team| {
                    (
                        team_types
                            .iter()
                            .position(|id| Some(*id) == team.team_type_id())
                            .unwrap(),
                        houses.iter().position(|id| *id == team.owner()).unwrap(),
                    )
                })
                .collect();
            let native_created: Vec<_> = update["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|event| event["call"] == "create_team_boundary")
                .map(|event| {
                    (
                        event["team_type"].as_u64().unwrap() as usize,
                        event["house"].as_u64().unwrap() as usize,
                    )
                })
                .collect();
            assert_eq!(
                actual_created, native_created,
                "{context}: requests reach existing Team owner"
            );
            let autocreate: Vec<_> = team_types
                .iter()
                .map(|id| i32::from(sim.team_script_vm.team_type_ini(*id).unwrap().autocreate))
                .collect();
            let native_autocreate: Vec<_> = update["autocreate_after"]
                .as_array()
                .unwrap()
                .iter()
                .map(number)
                .collect();
            assert_eq!(autocreate, native_autocreate, "{context}: Autocreate marks");
            assert_eq!(
                save_codec::serialize(sim.team_script_vm.team(old_team).unwrap()).unwrap(),
                old_bytes,
                "{context}: existing Team retained"
            );
        }
        assert_rng(&sim, &row["final_rng_full"], name);
        assert_eq!(row["other_rng_streams_unchanged"], true);
        assert_eq!(row["code_unchanged"], true);
    }
}

#[test]
fn retail_all05_elapsed_stop_keeps_explicit_teams_and_saved_activation() {
    use crate::rules::{process_owner::NativeScenarioRulesPrefix, team_ai_ini::TeamAiIniRegistry};
    use crate::sim::house_state::HouseDifficulty;
    use crate::sim::snapshot::GameSnapshot;
    let native = corpus();
    let retail = &native["retail"];
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let raw = assets.get_ref("ALL05UMD.MAP").unwrap();
    assert_eq!(crate::util::sha256::sha256_hex(raw), retail["map_sha256"]);
    let map = crate::map::map_file::MapFile::from_bytes(raw).unwrap();
    let named = assets.resolve_ref("ALL05UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL05UMD.INI")
            .unwrap()
            .ini
    });
    let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, _, _, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let native_delays: Vec<_> = retail["rules_team_delays"]
        .as_array()
        .unwrap()
        .iter()
        .map(number)
        .collect();
    assert_eq!(rules.general.team_delays, native_delays);
    for (section, key) in [
        ("Triggers", "raw_trigger"),
        ("Events", "raw_event"),
        ("Actions", "raw_actions"),
    ] {
        assert_eq!(
            map.ini
                .section(section)
                .unwrap()
                .read_string("092A219C", "", 4096),
            retail[key]
        );
    }
    let fixed_ai = crate::rules::retail_sources::select_ini(&assets, "AIMD.INI")
        .unwrap()
        .ini;
    let teams = TeamAiIniRegistry::from_sources(&fixed_ai, &map.ini, false);
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let mut sim = Simulation::with_seed(31);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    crate::sim::scenario_bootstrap::initialize_map_roster_houses(
        &mut sim,
        &roster,
        Some(&rules),
        Some(&map.ini),
    );
    sim.session.install_waypoints(&map.waypoints);
    sim.install_team_ai_registry(&teams, &rules).unwrap();
    sim.initialize_trigger_definitions(&map, &teams, None, None);
    let targets: Vec<_> = retail["target_houses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|house| {
            let country = crate::rules::ruleset::CountryIdx(
                u16::try_from(number(&house["native_country"])).unwrap(),
            );
            let id = sim.house_by_country_index(country, &rules).unwrap();
            assert_eq!(sim.interner.resolve(id), house["name"].as_str().unwrap());
            id
        })
        .collect();
    assert_eq!(targets.len(), 2);
    let stop_type = sim
        .trigger_runtime
        .definitions
        .tag_type("092A6E8C")
        .unwrap();
    let stop_tag = sim.shared_trigger_tag(stop_type);
    let stop_live = sim.trigger_runtime.tag(stop_tag).unwrap().triggers[0];
    let stop_definition = sim.trigger_runtime.trigger(stop_live).unwrap().type_index;
    assert_eq!(
        sim.trigger_runtime.definitions.trigger_types[stop_definition]
            .actions
            .len(),
        5
    );
    let begin_type = sim
        .trigger_runtime
        .definitions
        .tag_type("077A6CAC")
        .unwrap();
    let begin_tag = sim.shared_trigger_tag(begin_type);
    let begin_live = sim.trigger_runtime.tag(begin_tag).unwrap().triggers[0];
    assert!(!sim.trigger_runtime.trigger(begin_live).unwrap().enabled);
    let begin_definition = sim.trigger_runtime.trigger(begin_live).unwrap().type_index;
    let begin_actions = sim.trigger_runtime.definitions.trigger_types[begin_definition]
        .actions
        .clone();
    // Supplied ready-state prior uses the actual two authored74 commands.
    // The disabled begin Trigger's Event55->53 prerequisite is source-bound
    // in the corpus, not claimed as a played ALL05 activation sequence here.
    let begin: Vec<_> = begin_actions
        .iter()
        .filter(|action| action.entry.kind == 74)
        .collect();
    assert_eq!(begin.len(), 2);
    for (index, action) in begin.iter().enumerate() {
        assert_eq!(
            action.value,
            Some(number(&retail["native_action74_reads"][index]["value"]))
        );
        assert!(sim.apply_live_trigger_action(
            action,
            TriggerActionContext {
                live_index: Some(begin_live),
                incoming_house: Some(targets[1]),
                input: TagEventInput::default()
            },
            &mut Vec::new(),
            Some(&rules),
            None,
            FrameEffects::default()
        ));
    }
    assert!(
        targets
            .iter()
            .all(|id| sim.houses[id].ai_activation().ai_triggers_active)
    );
    let (old_team, _) = install_team_scene(&mut sim, targets[0]);
    let old_bytes = save_codec::serialize(sim.team_script_vm.team(old_team).unwrap()).unwrap();
    assert!(!sim.dispatch_live_tag_event(
        stop_tag,
        TagEventInput {
            kind: EVENT_ANY,
            ..Default::default()
        },
        &mut Vec::new(),
        Some(&rules),
        None,
        FrameEffects::default()
    ));
    assert!(
        targets
            .iter()
            .all(|id| sim.houses[id].ai_activation().ai_triggers_active)
    );
    let timer = sim.trigger_runtime.trigger(stop_live).unwrap().timer;
    assert!(timer.duration() > 0);
    // This is a Rust integration deadline from the already ported timer,
    // not a new native240-second timer golden or whole-map scheduling claim.
    sim.session
        .set_binary_frame_for_test(timer.start_frame().wrapping_add(timer.duration()) as u32);
    let before = GameSnapshot::save(&sim, 0, 0, "ALL05UMD.MAP", 0);
    let mut restored = GameSnapshot::load(&before).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    for candidate in [&mut sim, &mut restored] {
        candidate.trigger_runtime.action_trace.clear();
        let siblings: Vec<_> = targets
            .iter()
            .map(|id| candidate.houses[id].ai_activation())
            .collect();
        let timers: Vec<_> = targets
            .iter()
            .map(|id| candidate.houses[id].team_creation.timer())
            .collect();
        assert!(candidate.dispatch_live_tag_event(
            stop_tag,
            TagEventInput {
                kind: EVENT_ANY,
                ..Default::default()
            },
            &mut Vec::new(),
            Some(&rules),
            None,
            FrameEffects::default()
        ));
        assert_eq!(
            candidate
                .trigger_runtime
                .action_trace
                .iter()
                .map(|action| action.0)
                .collect::<Vec<_>>(),
            [4, 53, 53, 75, 75]
        );
        for (index, id) in targets.iter().enumerate() {
            let expected = HouseAiActivationLatches {
                ai_triggers_active: false,
                ..siblings[index]
            };
            assert_eq!(candidate.houses[id].ai_activation(), expected);
            assert_eq!(candidate.houses[id].team_creation.timer(), timers[index]);
        }
        assert_eq!(
            save_codec::serialize(candidate.team_script_vm.team(old_team).unwrap()).unwrap(),
            old_bytes
        );
        let explicit = candidate
            .interner
            .get(retail["explicit_team"]["id"].as_str().unwrap())
            .unwrap();
        assert!(
            candidate
                .team_script_vm
                .teams_in_order()
                .any(|team| team.team_type_id() == Some(explicit))
        );
        // An actual independent authored Action4 remains allowed while75 is
        // off. New Team allocation/body parity is outside the native sink.
        let scripted = begin_actions
            .iter()
            .find(|action| {
                action.entry.kind == 4
                    && match action.reference {
                        Some(DefinitionReference::Team(index)) => {
                            candidate.trigger_runtime.definitions.team_types[index]
                                != retail["explicit_team"]["id"].as_str().unwrap()
                        }
                        _ => false,
                    }
            })
            .unwrap();
        let count = candidate.team_script_vm.teams_in_order().count();
        assert!(candidate.apply_live_trigger_action(
            scripted,
            TriggerActionContext {
                live_index: Some(begin_live),
                incoming_house: Some(targets[1]),
                input: TagEventInput::default()
            },
            &mut Vec::new(),
            Some(&rules),
            None,
            FrameEffects::default()
        ));
        assert!(candidate.team_script_vm.teams_in_order().count() > count);
        let frame = candidate.session.binary_frame() as i32;
        let mut rng_check = candidate.scenario_rng.clone();
        for id in &targets {
            let house = candidate.houses.get_mut(id).unwrap();
            house.difficulty = HouseDifficulty::from_native(1).unwrap();
            house.team_creation.restart(frame, 0);
            rng_check.next_range_i32_inclusive(1, 100);
            crate::sim::ai_team_creation::update_team_creation(
                candidate,
                &rules,
                *id,
                FrameEffects::default(),
            );
            let timer = candidate.houses[id].team_creation.timer();
            assert_eq!(
                [timer.start_frame(), timer.duration()],
                [frame, native_delays[1]]
            );
            assert!(!candidate.houses[id].ai_activation().ai_triggers_active);
        }
        assert_eq!(
            candidate.scenario_rng.native_state_hex(),
            rng_check.native_state_hex()
        );
        assert_eq!(
            save_codec::serialize(candidate.team_script_vm.team(old_team).unwrap()).unwrap(),
            old_bytes
        );
        let after = GameSnapshot::save(candidate, 0, 0, "ALL05UMD.MAP", 0);
        let mut loaded = GameSnapshot::load(&after).unwrap().sim;
        loaded.restore_after_snapshot_load().unwrap();
        for id in &targets {
            assert!(!loaded.houses[id].ai_activation().ai_triggers_active);
        }
        // Scenario load intentionally reseeds to0; Main/MapGen are process
        // cursors, supplied by the app after this deserialize-only boundary.
        // Compare the Scenario reset with an executed original seed0 prior,
        // rather than expecting all three live cursors to survive the save.
        let seed_zero = native["histories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == "stop_seed0_ranged_rejects_one_raw_word")
            .unwrap();
        assert_eq!(
            loaded.scenario_rng.native_state_hex(),
            seed_zero["initial_rng_full"]["scenario"]
        );
    }
}

// Steam 3e81a617..08600: Execute30/50C210's 21 caller controls and 15
// Execute76 controls, including 14 original due-selector/RNG histories.
// Recalc 5054B0 and ComputerParanoid are declared sinks in the 30 controls;
// production planner/retail checks below are Rust integration regressions.
fn house_control_action(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    owner: Option<crate::sim::intern::InternedId>,
    kind: i32,
    raw: &str,
) -> bool {
    let ini = crate::rules::ini_parser::IniFile::from_str(&format!(
        "[Actions]\nCONTROL=1,{kind},0,{raw},0,0,0,0,A\n"
    ));
    let entry = crate::map::actions::parse_actions(&ini)
        .remove("CONTROL")
        .unwrap()
        .entries()[0]
        .clone();
    let action = RuntimeDefinitions::default().resolve_action(entry, None, None);
    sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: None,
            incoming_house: owner,
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        Some(rules),
        None,
        FrameEffects::default(),
    )
}

fn house_base_control_rules() -> crate::rules::ruleset::RuleSet {
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    RuleSet::from_ini(&IniFile::from_str(
        "[General]\nHarvesterUnit=HARV\nAIExtraRefineries=0,0,0\nAISlaveMinerNumber=1,1,1\nAlliedBaseDefenseCounts=0,0,0\nSovietBaseDefenseCounts=0,0,0\nThirdBaseDefenseCounts=0,0,0\n\
         [AI]\nBuildConst=CON\nBuildPower=POW\nBuildRefinery=REF\nBuildBarracks=BAR\nBuildWeapons=WEAP\nBuildRadar=RAD\nBuildTech=TECH\n\
         [Countries]\n0=Americans\n[Sides]\nAllied=Americans\n[Americans]\nSide=Allied\n\
         [VehicleTypes]\n0=HARV\n[HARV]\nOwner=Americans\nStrength=1\n\
         [BuildingTypes]\n0=CON\n1=POW\n2=REF\n3=BAR\n4=WEAP\n5=RAD\n6=TECH\n\
         [CON]\nOwner=Americans\nStrength=500\nConstructionYard=yes\nFactory=BuildingType\nAIBuildThis=yes\nTechLevel=1\n\
         [POW]\nOwner=Americans\nStrength=100\nAIBuildThis=yes\nTechLevel=1\n\
         [REF]\nOwner=Americans\nStrength=100\nAIBuildThis=yes\nTechLevel=1\n\
         [BAR]\nOwner=Americans\nStrength=100\nAIBuildThis=yes\nTechLevel=1\n\
         [WEAP]\nOwner=Americans\nStrength=100\nAIBuildThis=yes\nTechLevel=1\n\
         [RAD]\nOwner=Americans\nStrength=100\nAIBuildThis=no\n\
         [TECH]\nOwner=Americans\nStrength=100\nAIBuildThis=no\n\
         [Clear]\nBuildable=yes\nFoot=100%\nTrack=100%\nWheel=100%\n"
    )).unwrap()
}

fn house_base_control_scene(
    rules: &crate::rules::ruleset::RuleSet,
    yard_type: &str,
    yards: bool,
) -> (Simulation, crate::sim::intern::InternedId) {
    let mut sim = Simulation::with_seed(31);
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
    let owner = sim.interner.intern("AIHouse");
    let country = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner,
            0,
            Some(country),
            false,
            10000,
            10,
        ));
    if yards {
        for (x, y) in [(12, 17), (19, 21)] {
            sim.spawn_object(yard_type, "AIHouse", x, y, 0, rules)
                .expect("control yard Unlimbo");
        }
        assert_eq!(sim.houses[&owner].build_const_order.len(), 2);
    }
    let house = sim.houses.get_mut(&owner).unwrap();
    house.set_base_center((3, 4));
    house.base_plan_center = (5, 6);
    house.base_plan.nodes.clear();
    house.base_plan.percent_built = 37;
    house.set_ai_activation_for_test(HouseAiActivationLatches {
        production: true,
        autocreate_allowed: true,
        ai_triggers_active: true,
        auto_base_building: true,
    });
    (sim, owner)
}

#[test]
fn house_trigger_base_low_byte_controls_keep_other_latches_and_restore_nested_scope() {
    let rules = house_base_control_rules();
    for (raw, enabled) in [
        ("0", false),
        ("1", true),
        ("2", true),
        ("255", true),
        ("256", false),
        ("257", true),
        ("-1", true),
        ("-256", false),
        ("-257", true),
        ("-2147483648", false),
        ("2147483647", true),
        (".2", false),
        ("257suffix", true),
    ] {
        let (mut sim, owner) = house_base_control_scene(&rules, "CON", true);
        let rng = sim.rng_state();
        let timer = sim.houses[&owner].team_creation.timer();
        sim.with_object_placement_scope(|sim| {
            sim.with_object_placement_scope(|sim| {
                sim.with_object_placement_scope(|sim| {
                    assert!(
                        house_control_action(sim, &rules, Some(owner), 30, raw),
                        "{raw}"
                    );
                    assert!(
                        sim.object_placement_scope_active(),
                        "50C311 restores its caller's scope"
                    );
                })
            })
        });
        assert!(!sim.object_placement_scope_active());
        let house = &sim.houses[&owner];
        assert_eq!(
            house.ai_activation(),
            HouseAiActivationLatches {
                production: true,
                autocreate_allowed: true,
                ai_triggers_active: true,
                auto_base_building: enabled,
            },
            "{raw}"
        );
        assert_eq!(house.team_creation.timer(), timer);
        assert_eq!(house.base_plan.percent_built, 37);
        if enabled {
            // Native caller controls choose first(12,17), then the reverse
            // general Building list chooses(19,21). The plan generator itself
            // is a sink there; this checks its existing production integration.
            assert_eq!(house.base_center, Some((19, 21)));
            assert_eq!(house.base_plan_center, (19, 21));
            assert_eq!(
                crate::sim::base_plan::unpack_base_plan_cell(house.base_plan.nodes[0].packed_cell),
                (19, 21)
            );
        } else {
            assert_eq!(house.base_center, Some((3, 4)));
            assert_eq!(house.base_plan_center, (5, 6));
            assert!(house.base_plan.nodes.is_empty());
            assert_eq!(sim.rng_state(), rng);
        }
    }
}

#[test]
fn house_trigger_existing_plan_keeps_node_zero_and_empty_yard_vector_does_not_skip_recalc() {
    let rules = house_base_control_rules();
    let (mut sim, owner) = house_base_control_scene(&rules, "CON", true);
    let node = crate::sim::base_plan::BasePlanNode {
        type_or_control: rules.building_type_index("CON").unwrap(),
        packed_cell: crate::sim::base_plan::pack_base_plan_cell(21, 22),
        filled: true,
        retry_count: 8,
    };
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .base_plan
        .nodes
        .push(node);
    let rng = sim.rng_state();
    assert!(house_control_action(&mut sim, &rules, Some(owner), 30, "1"));
    assert_eq!(sim.houses[&owner].base_center, Some((12, 17)));
    assert_eq!(sim.houses[&owner].base_plan_center, (12, 17));
    assert_eq!(sim.houses[&owner].base_plan.nodes, vec![node]);
    assert_eq!(sim.rng_state(), rng);
    // Declared native negative control: BuildConst vector empty while the
    // general Building list still contains the two supplied objects.
    let (mut sim, owner) = house_base_control_scene(&rules, "CON", true);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .build_const_order
        .clear();
    assert!(house_control_action(&mut sim, &rules, Some(owner), 30, "1"));
    assert_eq!(sim.houses[&owner].base_center, Some((19, 21)));
    assert_eq!(sim.houses[&owner].base_plan_center, (19, 21));
    assert!(!sim.houses[&owner].base_plan.nodes.is_empty());
    let (mut sim, owner) = house_base_control_scene(&rules, "CON", false);
    let rng = sim.rng_state();
    assert!(house_control_action(&mut sim, &rules, Some(owner), 30, "1"));
    assert_eq!(sim.houses[&owner].base_center, Some((3, 4)));
    assert_eq!(sim.houses[&owner].base_plan_center, (5, 6));
    assert!(sim.houses[&owner].base_plan.nodes.is_empty());
    assert_eq!(sim.rng_state(), rng);
}

#[test]
fn house_trigger_retail_base_plan_gate_and_anchor_survive_production_snapshot_restore() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    let (mut sim, owner) = house_base_control_scene(&rules, "GACNST", true);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_ai_activation_for_test(HouseAiActivationLatches::default());
    assert!(house_control_action(&mut sim, &rules, Some(owner), 30, "1"));
    let house = &sim.houses[&owner];
    assert!(house.ai_activation().auto_base_building);
    assert!(
        !house.ai_activation().production
            && !house.ai_activation().autocreate_allowed
            && !house.ai_activation().ai_triggers_active
    );
    assert_eq!(house.base_center, Some((19, 21)));
    assert_eq!(house.base_plan_center, (19, 21));
    assert_eq!(
        house.base_plan.nodes[0].type_or_control,
        rules.building_type_index("GACNST").unwrap()
    );
    assert_eq!(
        crate::sim::base_plan::unpack_base_plan_cell(house.base_plan.nodes[0].packed_cell),
        (19, 21)
    );
    // The existing early House update consumes the latch even at low IQ;
    // TAction30 itself does not co-enable its three sibling activation bytes.
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .update_ai_activation(false, i32::MAX);
    assert!(sim.houses[&owner].ai_activation().production);
    assert!(sim.houses[&owner].ai_activation().autocreate_allowed);
    assert!(!sim.houses[&owner].ai_activation().ai_triggers_active);
    let plan = sim.houses[&owner].base_plan.clone();
    let rng = sim.rng_state();
    assert!(house_control_action(
        &mut sim,
        &rules,
        Some(owner),
        30,
        "256"
    ));
    assert!(!sim.houses[&owner].ai_activation().auto_base_building);
    assert_eq!(sim.houses[&owner].base_plan, plan);
    assert_eq!(sim.rng_state(), rng);
    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "Disabled base", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    loaded.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
    loaded.reconstruct_cellclass_dummy_for_map_resize();
    loaded.restore_after_snapshot_load().unwrap();
    loaded.rebuild_caches_after_load(
        sim.resolved_terrain.as_ref().unwrap().clone(),
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &rules,
    );
    loaded
        .restore_map_authority_after_snapshot_load(&rules, &registry, None)
        .unwrap();
    loaded.resolve_type_handles(&rules);
    assert!(!loaded.houses[&owner].ai_activation().auto_base_building);
    assert_eq!(loaded.houses[&owner].base_plan, plan);
    assert_eq!(loaded.houses[&owner].base_center, Some((19, 21)));
    assert_eq!(loaded.houses[&owner].base_plan_center, (19, 21));
    let rng = loaded.rng_state();
    assert!(house_control_action(
        &mut loaded,
        &rules,
        Some(owner),
        30,
        "257"
    ));
    assert_eq!(loaded.houses[&owner].base_center, Some((12, 17)));
    assert_eq!(loaded.houses[&owner].base_plan_center, (12, 17));
    assert_eq!(
        loaded.houses[&owner].base_plan, plan,
        "an existing plan is not regenerated or reanchored"
    );
    assert_eq!(
        loaded.rng_state(),
        rng,
        "the existing-plan path makes no draw"
    );
}

#[test]
fn house_trigger_ratio_original_writer_and_due_selector_keep_signed_values_timers_and_rng() {
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::house_state::{HouseDifficulty, HouseState};
    let rules=RuleSet::from_ini(&IniFile::from_str(
        "[General]\nTeamDelays=300,600,900\nTotalAITeamCap=8,8,8\nMaximumAIDefensiveTeams=2,2,2\nMinimumAIDefensiveTeams=9,2,9\nUseMinDefenseRule=no\n"
    )).unwrap();
    for (raw, value, picked) in [
        ("0", 0, false),
        ("1", 1, false),
        ("50", 50, false),
        ("83", 83, false),
        ("84", 84, true),
        ("99", 99, true),
        ("100", 100, true),
        ("101", 101, true),
        ("256", 256, true),
        ("-1", -1, false),
        ("-2147483648", i32::MIN, false),
        ("2147483647", i32::MAX, true),
        (".2", 0, false),
        ("257suffix", 257, true),
    ] {
        let mut sim = Simulation::new();
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        let owner = sim.interner.intern("H0");
        let enemy = sim.interner.intern("H1");
        for id in [owner, enemy] {
            sim.houses
                .register(HouseState::new(id, 0, None, false, 0, 10));
        }
        let house = sim.houses.get_mut(&owner).unwrap();
        house.difficulty = HouseDifficulty::Normal;
        house.enemy_house = Some(enemy);
        house.set_ai_triggers_active(true);
        house.team_creation.restart(0, 10);
        let (old_team, team_types) = install_team_scene(&mut sim, owner);
        let old_bytes = save_codec::serialize(sim.team_script_vm.team(old_team).unwrap()).unwrap();
        supply_rng(&mut sim, &corpus()["actions"]["rng_full_seed31"]);
        let other = serde_json::to_value(&sim.houses[&enemy]).unwrap();
        let before_hash = sim.state_hash();
        assert!(house_control_action(&mut sim, &rules, Some(owner), 76, raw));
        assert_eq!(sim.houses[&owner].team_creation.ratio(), value);
        assert_eq!(
            sim.houses[&owner].team_creation.timer(),
            crate::sim::timer::CdTimer::started(0, 10)
        );
        assert_eq!(
            sim.state_hash() == before_hash,
            value == 100,
            "{raw}: hash before the queue runs"
        );
        assert_eq!(serde_json::to_value(&sim.houses[&enemy]).unwrap(), other);
        assert_rng(&sim, &corpus()["actions"]["rng_full_seed31"], raw);
        sim.session.set_binary_frame_for_test(9);
        crate::sim::ai_team_creation::update_team_creation(
            &mut sim,
            &rules,
            owner,
            FrameEffects::default(),
        );
        assert_rng(&sim, &corpus()["actions"]["rng_full_seed31"], "not yet due");
        sim.session.set_binary_frame_for_test(10);
        let main = sim.main_rng.native_state_hex();
        let mapgen = sim.mapgen_rng.native_state_hex();
        crate::sim::ai_team_creation::update_team_creation(
            &mut sim,
            &rules,
            owner,
            FrameEffects::default(),
        );
        // Original seed31: ratio request(1,100)->84 from raw2026076499;
        // admitted weight request(1,100)->22 from raw2287577493. The native
        // Team constructor is an explicit boundary sink in these histories.
        let expected = if picked {
            "f04f3ddbbbc963c9ab693cef92909cb4f81582ce4f0d8ecd1f8bc663e4dcbe51"
        } else {
            "455190bf327892273204c631e560be21409c1922f7166b2942534865fb3c29af"
        };
        assert_eq!(
            crate::util::sha256::sha256_hex(sim.scenario_rng.native_state_hex().as_bytes()),
            expected,
            "{raw}: complete native RNG state"
        );
        assert_eq!(sim.main_rng.native_state_hex(), main);
        assert_eq!(sim.mapgen_rng.native_state_hex(), mapgen);
        let created: Vec<_> = sim
            .team_script_vm
            .teams_in_order()
            .filter(|team| team.id() != old_team)
            .map(|team| team.team_type_id())
            .collect();
        assert_eq!(
            created,
            if picked {
                vec![Some(team_types[0])]
            } else {
                vec![]
            }
        );
        assert_eq!(
            save_codec::serialize(sim.team_script_vm.team(old_team).unwrap()).unwrap(),
            old_bytes
        );
        assert_eq!(
            sim.houses[&owner].team_creation.timer(),
            crate::sim::timer::CdTimer::started(10, 600)
        );
        let hash = sim.state_hash();
        assert!(house_control_action(
            &mut sim,
            &rules,
            None,
            76,
            "2147483647"
        ));
        assert_eq!(
            sim.state_hash(),
            hash,
            "native null-House76 succeeds without a write"
        );
    }
}

#[test]
fn house_team_controls_ordered_tag_history_preserves_native_due_selection_and_save() {
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::house_state::HouseDifficulty;
    let (mut sim, mut rules, registry, houses) = super::tests::event_ingress_fixture();
    // The declared skirmish priors of the existing original AI fixture.
    rules.general = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nTeamDelays=300,600,900\nTotalAITeamCap=8,8,8\nMaximumAIDefensiveTeams=2,2,2\nMinimumAIDefensiveTeams=9,2,9\nUseMinDefenseRule=no\n",
    )).unwrap().general;
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    for (index, owner) in houses.iter().enumerate() {
        let house = sim.houses.get_mut(owner).unwrap();
        house.set_ai_activation_for_test(
            [
                HouseAiActivationLatches {
                    production: true,
                    ai_triggers_active: true,
                    ..Default::default()
                },
                HouseAiActivationLatches {
                    auto_base_building: true,
                    ..Default::default()
                },
                HouseAiActivationLatches {
                    production: true,
                    autocreate_allowed: true,
                    ai_triggers_active: true,
                    auto_base_building: true,
                },
            ][index],
        );
        house.difficulty = HouseDifficulty::Normal;
        house.team_creation.restart(0, 10);
    }
    sim.houses.get_mut(&houses[0]).unwrap().enemy_house = Some(houses[1]);
    let (old_team, types) = install_team_scene(&mut sim, houses[0]);
    let old_team_bytes = save_codec::serialize(sim.team_script_vm.team(old_team).unwrap()).unwrap();
    super::tests::install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nAI=Country0,<none>,AI,0,1,1,1,0\n[Events]\nAI=1,13,0,0\n\
         [Actions]\nAI=4,13,0,7,0,0,0,0,A,77,0,-2147483648,0,0,0,0,A,78,0,2147483647,0,0,0,0,A,79,0,257suffix,0,0,0,0,A\n\
         [Tags]\nAITAG=0,AI,AI\n",
    );
    supply_rng(&mut sim, &corpus()["actions"]["rng_full_seed31"]);
    let rng = sim.rng_state();
    let main_before = sim.main_rng.native_state_hex();
    let mapgen_before = sim.mapgen_rng.native_state_hex();
    let untouched = save_codec::serialize(&sim.houses[&houses[2]]).unwrap();
    sim.advance_triggers(Some(&rules), Some(&registry), FrameEffects::default());
    // Original consecutive Read/Execute13/77/78/79, Steam3e81a617..08600:
    // 13 selects Country7, while all three ratio actions use incoming House0.
    assert_eq!(
        sim.houses[&houses[0]].scenario_team_ratios(),
        [i32::MIN, i32::MAX, 257]
    );
    assert_eq!(sim.houses[&houses[1]].scenario_team_ratios(), [75; 3]);
    assert!(!sim.houses[&houses[0]].ai_activation().autocreate_allowed);
    assert!(sim.houses[&houses[1]].ai_activation().autocreate_allowed);
    assert!(!sim.houses[&houses[1]].ai_activation().production);
    assert!(!sim.houses[&houses[1]].ai_activation().ai_triggers_active);
    assert!(sim.houses[&houses[1]].ai_activation().auto_base_building);
    assert_eq!(
        save_codec::serialize(&sim.houses[&houses[2]]).unwrap(),
        untouched
    );
    assert_eq!(sim.houses[&houses[0]].team_creation.ratio(), 100);
    assert!(houses.iter().all(
        |id| sim.houses[id].team_creation.timer() == crate::sim::timer::CdTimer::started(0, 10)
    ));
    assert_eq!(sim.rng_state(), rng);
    assert_eq!(sim.team_script_vm.teams_in_order().count(), 1);

    let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "Team controls", 0);
    let loaded = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    for owner in houses {
        assert_eq!(
            loaded.houses[&owner].scenario_team_ratios(),
            sim.houses[&owner].scenario_team_ratios()
        );
        assert_eq!(
            loaded.houses[&owner].ai_activation(),
            sim.houses[&owner].ai_activation()
        );
        assert_eq!(
            loaded.houses[&owner].team_creation,
            sim.houses[&owner].team_creation
        );
    }

    sim.session.set_binary_frame_for_test(10);
    crate::sim::ai_team_creation::update_team_creation(
        &mut sim,
        &rules,
        houses[0],
        FrameEffects::default(),
    );
    // Same original history: only the independent +565C/AITriggersActive
    // gate participates. Draws are (1,100)->84, then weight(1,100)->22;
    // native Team allocation is a boundary sink, Rust uses its actual VM.
    assert_eq!(
        crate::util::sha256::sha256_hex(sim.scenario_rng.native_state_hex().as_bytes()),
        "f04f3ddbbbc963c9ab693cef92909cb4f81582ce4f0d8ecd1f8bc663e4dcbe51"
    );
    let created: Vec<_> = sim
        .team_script_vm
        .teams_in_order()
        .filter(|team| team.id() != old_team)
        .map(|team| team.team_type_id())
        .collect();
    assert_eq!(created, vec![Some(types[0])]);
    assert_eq!(
        save_codec::serialize(sim.team_script_vm.team(old_team).unwrap()).unwrap(),
        old_team_bytes
    );
    assert_eq!(
        sim.houses[&houses[0]].team_creation.timer(),
        crate::sim::timer::CdTimer::started(10, 600)
    );
    // Main is a process stream, intentionally absent from snapshots. Compare
    // against this live process's prior, not the fresh load's placeholder.
    assert_eq!(sim.main_rng.native_state_hex(), main_before);
    assert_eq!(sim.mapgen_rng.native_state_hex(), mapgen_before);
}

#[test]
fn house_team_controls_signed_reader_hash_and_null_house_keep_other_state() {
    let rules = house_base_control_rules();
    for (kind, index) in [(77, 0), (78, 1), (79, 2)] {
        for (raw, expected) in [
            ("75", 75),
            ("-2147483648", i32::MIN),
            ("2147483647", i32::MAX),
            ("257suffix", 257),
            (".2", 0),
        ] {
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("House");
            sim.houses
                .register(crate::sim::house_state::HouseState::new(
                    owner, 0, None, false, 0, 10,
                ));
            let before_hash = sim.state_hash();
            let before = serde_json::to_value(&sim.houses[&owner]).unwrap();
            let rng = sim.rng_state();
            assert!(house_control_action(
                &mut sim,
                &rules,
                Some(owner),
                kind,
                raw
            ));
            let mut wanted = [75; 3];
            wanted[index] = expected;
            assert_eq!(sim.houses[&owner].scenario_team_ratios(), wanted);
            assert_eq!(sim.state_hash() == before_hash, expected == 75);
            let mut after = serde_json::to_value(&sim.houses[&owner]).unwrap();
            after["scenario_parameters"] = before["scenario_parameters"].clone();
            assert_eq!(after, before, "only the retained category ratio changes");
            let hash = sim.state_hash();
            assert!(house_control_action(&mut sim, &rules, None, kind, "99"));
            assert_eq!(
                sim.state_hash(),
                hash,
                "native null-House action succeeds without writes"
            );
            assert_eq!(sim.rng_state(), rng);
        }
    }
    // The shared 13 selector requires a live Trigger even for a valid Country.
    let mut sim = Simulation::new();
    assert!(!house_control_action(&mut sim, &rules, None, 13, "0"));
}
