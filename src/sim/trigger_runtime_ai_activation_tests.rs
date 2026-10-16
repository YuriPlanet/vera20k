//! TAction74/75 through the shared House activation and ordinary team owners.
use super::*;
use crate::sim::house_state::HouseAiActivationLatches;

#[test]
fn authored_ai_trigger_actions_change_only_the_selected_house_latch() {
    for (kind, initial, expected) in [(75, true, false), (74, false, true)] {
        let (mut sim, rules, registry, houses) = super::tests::event_ingress_fixture();
        let priors = HouseAiActivationLatches {
            production: true,
            autocreate_allowed: false,
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
        let map = super::tests::install_event_ingress_map(
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
        let _ = sim.advance_triggers(
            TriggerInputs {
                events: &map.events,
                actions: &map.actions,
                rules: Some(&rules),
            },
            Some(&registry),
            FrameEffects::default(),
        );
        for (index, owner) in houses.iter().enumerate() {
            let house = &sim.houses[owner];
            let wanted = HouseAiActivationLatches {
                ai_triggers_active: if index == 1 { expected } else { initial },
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

fn corpus() -> serde_json::Value {
    let value: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_ai_activation.json",
    ))
    .unwrap();
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
    sim.session.house_order = input["order"].as_array().map_or_else(
        || houses.to_vec(),
        |order| {
            order
                .iter()
                .map(|index| houses[index.as_u64().unwrap() as usize])
                .collect()
        },
    );
    // Original Scenario+1180 storage supplied by the native fixture: slot0
    // resolves registered House1, slot1 House0, and slots2..7 are absent.
    sim.session.start_slot_houses.clear();
    sim.session.start_slot_houses.insert(0, houses[1]);
    sim.session.start_slot_houses.insert(1, houses[0]);
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
        .map(|team| bincode::serialize(team).unwrap())
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
        .map(|team| bincode::serialize(team).unwrap())
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
        sim.session.game_mode_nonzero = true;
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
        sim.session.game_mode_nonzero = input["game_mode"].as_i64() != Some(0);
        let houses = [
            sim.interner.intern("H0"),
            sim.interner.intern("H1"),
            sim.interner.intern("H2"),
        ];
        let countries: Vec<_> = input["countries"]
            .as_array()
            .map_or(vec![0, 7, 0], |array| array.iter().map(number).collect());
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
            sim.houses.insert(owner, house);
        }
        sim.houses.get_mut(&houses[0]).unwrap().enemy_house = Some(houses[1]);
        sim.session.house_order = houses.to_vec();
        supply_rng(&mut sim, &row["initial_rng_full"]);
        let (old_team, team_types) = install_team_scene(&mut sim, houses[0]);
        let old_bytes = bincode::serialize(sim.team_script_vm.team(old_team).unwrap()).unwrap();
        for (step_index, step) in row["steps"].as_array().unwrap().iter().enumerate() {
            let context = format!("{name} step{step_index}");
            if let Some(action) = step.get("action") {
                run_native_action(&mut sim, &rules, houses, action);
                continue;
            }
            let update = &step["update"];
            let owner = houses[update["house"].as_u64().unwrap() as usize];
            assert_house_summary(&sim, owner, &update["before"], &context);
            sim.session.binary_frame = number(&update["frame"]) as u32;
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
                bincode::serialize(sim.team_script_vm.team(old_team).unwrap()).unwrap(),
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
    sim.session.game_mode_nonzero = false;
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
    let old_bytes = bincode::serialize(sim.team_script_vm.team(old_team).unwrap()).unwrap();
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
    sim.session.binary_frame = timer.start_frame().wrapping_add(timer.duration()) as u32;
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
            bincode::serialize(candidate.team_script_vm.team(old_team).unwrap()).unwrap(),
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
        let frame = candidate.session.binary_frame as i32;
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
            bincode::serialize(candidate.team_script_vm.team(old_team).unwrap()).unwrap(),
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
