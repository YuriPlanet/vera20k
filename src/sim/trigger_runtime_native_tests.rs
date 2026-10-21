//! Executed original histories, not a second trigger interpreter. Inputs set
//! up the supplied native storage; every operation calls the production owner.

use super::*;
use crate::sim::world::{DeferredDeleteQueue, DeferredDeleteTicket};
use crate::util::save_codec;
use serde_json::{Value, json};
use std::collections::HashMap;

fn integer(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn loss_threshold_event_corpus() -> &'static Value {
    let corpus =
        crate::test_fixture::json("tools/spatial_oracle/trigger_loss_threshold_events.json");
    corpus
}

#[test]
fn loss_threshold_events_match_original_incoming_house_and_signed_counters() {
    use super::tests::{event_ingress_fixture, install_event_ingress_map};
    use crate::sim::house_state::MatchStatistics;

    let corpus = loss_threshold_event_corpus();
    let cases = corpus["controls"].as_array().unwrap();
    assert_eq!(cases.len(), 34);
    for case in cases {
        let (mut sim, rules, _, houses) = event_ingress_fixture();
        for (house, losses) in houses.iter().zip(case["loss_counters"].as_array().unwrap()) {
            sim.houses.get_mut(house).unwrap().set_stats_for_test(
                MatchStatistics::from_totals_for_test(
                    0,
                    0,
                    integer(&losses[1]) as u32,
                    integer(&losses[0]) as u32,
                    0,
                ),
            );
        }
        install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nLOSS=Country0,<none>,Loss,0,1,1,1,0\n\
                 [Events]\nLOSS=1,{},0,{}\n[Tags]\nTAG=2,Loss,LOSS\n",
                integer(&case["kind"]),
                integer(&case["value"])
            ),
        );
        let typ = sim.trigger_runtime.definitions.tag_type("TAG").unwrap();
        let tag = sim.shared_trigger_tag(typ);
        let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
        let trigger_type = sim.trigger_runtime.trigger(live).unwrap().type_index;
        sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0].retained_house =
            Some(houses[2]);
        let timer = sim.trigger_runtime.trigger(live).unwrap().timer;
        let rng_before = (
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        let owner = case["incoming_house"]
            .as_u64()
            .map(|index| houses[index as usize]);
        let mut persistence = integer(&case["initial_persistence"]) != 0;
        let mut predicate = |sim: &mut Simulation| {
            sim.has_occurred(
                0,
                TagEventInput {
                    kind: integer(&case["raised"]),
                    ..Default::default()
                },
                owner,
                &mut persistence,
                &rules,
                live,
            )
        };
        let result = if case["scenario_init"].as_bool().unwrap() {
            sim.with_buildup_bypass(predicate)
        } else {
            predicate(&mut sim)
        };
        assert_eq!(result, integer(&case["result"]) != 0, "{case}");
        assert_eq!(persistence, integer(&case["persistence"]) != 0, "{case}");
        assert_eq!(sim.trigger_runtime.trigger(live).unwrap().timer, timer);
        assert_eq!(
            sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0].retained_house,
            Some(houses[2]),
            "loss conditions must not replace the retained House: {case}"
        );
        assert_eq!(
            (
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state()
            ),
            rng_before,
            "{case}"
        );
    }
    for classification in corpus["classification"].as_array().unwrap() {
        // The existing shared registration owner classifies both as House
        // polls. Their original StateA0/StateB1 does not latch completion.
        assert_eq!(classification["state_a"], 0);
        assert_eq!(classification["state_b"], 1);
        assert_eq!(event_classification(integer(&classification["kind"])), 8);
    }
}

#[test]
fn loss_threshold_events_bind_retail_all04_reinforcement_operands() {
    use crate::rules::{process_owner::NativeScenarioRulesPrefix, team_ai_ini::TeamAiIniRegistry};
    use crate::sim::house_state::MatchStatistics;

    let corpus = loss_threshold_event_corpus();
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let raw = assets.get_ref("ALL04DMD.MAP").unwrap();
    let map = crate::map::map_file::MapFile::from_bytes(raw).unwrap();
    let named = assets.resolve_ref("ALL04DMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL04DMD.INI")
            .unwrap()
            .ini
    });
    let mut rules_owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, _, _, _) = rules_owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let fixed_ai = crate::rules::retail_sources::select_ini(&assets, "AIMD.INI")
        .unwrap()
        .ini;
    let teams = TeamAiIniRegistry::from_sources(&fixed_ai, &map.ini, false);
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    crate::sim::scenario_bootstrap::initialize_map_roster_houses(
        &mut sim,
        &roster,
        &rules,
        Some(&map.ini),
    );
    sim.install_team_ai_registry(&teams, &rules).unwrap();
    sim.initialize_trigger_definitions(&map, &teams, None, None);
    let routes = corpus["retail_routes"].as_array().unwrap();
    assert_eq!(routes.len(), 2);
    for route in routes {
        assert_eq!(crate::util::sha256::sha256_hex(raw), route["map_sha256"]);
        let trigger_id = route["trigger_id"].as_str().unwrap();
        assert_eq!(
            map.ini
                .section("Events")
                .unwrap()
                .read_string(trigger_id, "", 512),
            route["raw_event"]
        );
        let tag_type = sim
            .trigger_runtime
            .definitions
            .tag_type(route["tag_id"].as_str().unwrap())
            .unwrap();
        let tag = sim.shared_trigger_tag(tag_type);
        let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
        let definition = &sim.trigger_runtime.definitions.trigger_types
            [sim.trigger_runtime.trigger(live).unwrap().type_index];
        assert_eq!(definition.id, trigger_id);
        let condition = &definition.events[0].condition;
        let kind = condition.kind;
        let threshold = condition.value;
        assert_eq!(kind, integer(&route["native_read"]["kind"]));
        assert_eq!(threshold, integer(&route["native_read"]["value"]));
        let owner = resolve_trigger_house(&sim, definition.owner.as_deref(), &rules)
            .expect("the authored trigger House exists in the campaign roster");
        assert_eq!(sim.interner.resolve(owner), route["team"]["House"]);
        let action = definition
            .actions
            .iter()
            .find(|action| action.entry.kind == 7)
            .unwrap();
        let Some(DefinitionReference::Team(team)) = action.reference else {
            panic!("the retail reinforcement must retain its actual Team reference");
        };
        let team_name = &sim.trigger_runtime.definitions.team_types[team];
        assert_eq!(team_name, route["team_id"].as_str().unwrap());
        assert!(
            sim.team_script_vm
                .team_type_definition(sim.interner.get(team_name).unwrap())
                .is_some()
        );
        // Bind actual retail operands and the existing downstream Team owner.
        // Supplied accounting priors isolate the predicate; the normal-frame
        // callback -> reinforcement test covers the connected ordinary path.
        // This is not a played ALL04 mission or original Team execution.
        for (losses, expected) in [(threshold - 1, false), (threshold, true)] {
            sim.houses.get_mut(&owner).unwrap().set_stats_for_test(
                MatchStatistics::from_totals_for_test(
                    0,
                    0,
                    if kind == 16 { losses as u32 } else { 0 },
                    if kind == 15 { losses as u32 } else { 0 },
                    0,
                ),
            );
            let mut persistence = false;
            assert_eq!(
                sim.has_occurred(
                    0,
                    TagEventInput {
                        kind: EVENT_ANY,
                        ..Default::default()
                    },
                    Some(owner),
                    &mut persistence,
                    &rules,
                    live,
                ),
                expected,
                "{trigger_id} losses{losses}"
            );
            assert!(!persistence);
        }
    }
}

#[test]
fn naval_land_events_match_original_signed_tracking_and_country_selection() {
    use super::tests::{
        event_ingress_fixture, install_event_ingress_map, spawn_event_ingress_unit,
    };
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_naval_land_events.json",
    ))
    .unwrap();
    assert_eq!(corpus["country_indices"], json!([0, 7, 0]));
    assert_eq!(corpus["code_unchanged"], true);
    assert_eq!(corpus["zero_predicate_rng_calls"], true);
    assert_eq!(corpus["tracking_replay"]["payload_matches_saved"], true);
    assert_eq!(corpus["tracking_replay"]["cases"], 36);
    for classification in corpus["classification"].as_array().unwrap() {
        assert_eq!(classification["state_a"], 0);
        assert_eq!(classification["state_b"], 1);
        assert_eq!(event_classification(integer(&classification["kind"])), 8);
    }
    let controls = corpus["controls"].as_array().unwrap();
    assert_eq!(controls.len(), 39);
    for case in controls {
        let (mut sim, rules, _, houses) = event_ingress_fixture();
        let actor = case["actor_owner"].as_u64().map(|index| {
            spawn_event_ingress_unit(&mut sim, &rules, houses[index as usize], (15, 15))
        });
        for (index, owner) in houses.iter().enumerate() {
            let counts = &case["tracked_counts"][index];
            sim.houses
                .get_mut(owner)
                .unwrap()
                .tracking_mut()
                .set_naval_land_for_test(
                    integer(&counts[0]),
                    integer(&counts[1]),
                    integer(&counts[2]),
                );
        }
        sim.houses.reorder_for_test(
            &case["house_order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|index| houses[index.as_u64().unwrap() as usize])
                .collect::<Vec<_>>(),
        );
        install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nNAVY=Country0,<none>,Navy,0,1,1,1,0\n\
             [Events]\nNAVY=1,{},0,{}\n[Tags]\nTAG=2,Navy,NAVY\n",
                integer(&case["kind"]),
                integer(&case["value"]),
            ),
        );
        let typ = sim.trigger_runtime.definitions.tag_type("TAG").unwrap();
        let tag = sim.shared_trigger_tag(typ);
        let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
        let trigger_type = sim.trigger_runtime.trigger(live).unwrap().type_index;
        sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0].retained_house =
            Some(houses[2]);
        let timer = sim.trigger_runtime.trigger(live).unwrap().timer;
        let rng_before = (
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        let mut persistence = integer(&case["initial_persistence"]) != 0;
        let mut evaluate = |sim: &mut Simulation| {
            sim.has_occurred(
                0,
                TagEventInput {
                    kind: integer(&case["raised"]),
                    actor,
                    source: None,
                    cell: (0, 0),
                    force: false,
                },
                case["incoming_house"]
                    .as_u64()
                    .map(|index| houses[index as usize]),
                &mut persistence,
                &rules,
                live,
            )
        };
        let result = if case["scenario_init"].as_bool().unwrap() {
            sim.with_buildup_bypass(evaluate)
        } else {
            evaluate(&mut sim)
        };
        assert_eq!(result, integer(&case["result"]) != 0, "{case}");
        assert_eq!(persistence, integer(&case["persistence"]) != 0, "{case}");
        assert_eq!(case["timer_after"], case["timer_before"], "{case}");
        assert_eq!(sim.trigger_runtime.trigger(live).unwrap().timer, timer);
        assert_eq!(integer(&case["event54"]) as u32, 0xABCD1234);
        assert_eq!(
            sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0].retained_house,
            Some(houses[2]),
            "55/56 leave Event+54 unchanged: {case}"
        );
        assert_eq!(
            (
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state()
            ),
            rng_before,
            "{case}"
        );
    }
}

#[test]
fn naval_event_retail_all05_removal_enables_black_eagle_support_once() {
    use crate::rules::{ini_parser::IniFile, process_owner::NativeScenarioRulesPrefix};
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_naval_land_events.json",
    ))
    .unwrap();
    let native = &corpus["retail"];
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let raw_map = assets.get_ref("ALL05UMD.MAP").unwrap();
    assert_eq!(
        crate::util::sha256::sha256_hex(raw_map),
        native["map_sha256"].as_str().unwrap()
    );
    let map = crate::map::map_file::MapFile::from_bytes(raw_map).unwrap();
    let named = assets.resolve_ref("ALL05UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL05UMD.INI")
            .unwrap()
            .ini
    });
    let mut rules_owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, _, _, _) = rules_owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let rules_bytes = assets.get_ref("RULESMD.INI").unwrap();
    assert_eq!(
        crate::util::sha256::sha256_hex(rules_bytes),
        native["rules_sha256"].as_str().unwrap()
    );
    let bsub_type = rules.object("BSUB").unwrap();
    let native_fields = &native["type_field_readers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["fields"];
    assert_eq!(i32::from(bsub_type.naval), integer(&native_fields["naval"]));
    assert_eq!(bsub_type.passengers, integer(&native_fields["passengers"]));
    let expected_countries = &native["country_registries"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["entries"];
    for country in expected_countries.as_array().unwrap() {
        assert_eq!(
            rules.country_name(crate::rules::ruleset::CountryIdx(
                integer(&country["index"]) as u16
            )),
            country["name"].as_str()
        );
    }
    let selected = native["selected_house_name"].as_str().unwrap();
    let source = native["trigger_id"].as_str().unwrap();
    let tag_id = native["tag_id"].as_str().unwrap();
    let bsub = map
        .entities
        .iter()
        .find(|entity| entity.owner == selected && entity.type_id == "BSUB")
        .unwrap();
    assert_eq!(
        bsub.health,
        integer(&native["authored_health"]),
        "authored fraction, not actual HP"
    );
    assert_eq!(
        map.ini.section("Events").unwrap().get_for_test(source),
        native["raw_event"].as_str()
    );
    assert_eq!(
        map.ini.section("Actions").unwrap().get_for_test(source),
        native["actions"].as_str()
    );
    assert_eq!(
        map.events[source].conditions()[0].kind,
        integer(&native["native_read"]["kind"])
    );
    assert_eq!(
        map.events[source].conditions()[0].value,
        integer(&native["native_read"]["value"])
    );

    // Retain physical records for the selected entry and its two enable
    // consumers. This isolates removal -> House polling -> announcement and
    // support enable, not the later aircraft execution or whole campaign.
    let chain = [source, "0E5CDB7C", "0778A83C"];
    let mut records = String::new();
    for section in ["Triggers", "Events", "Actions", "Tags"] {
        records.push_str(&format!("[{section}]\n"));
        for (key, value) in map.ini.section(section).unwrap().raw_entries() {
            if (section == "Tags"
                && map
                    .tags
                    .get(key)
                    .is_some_and(|tag| tag.trigger().is_some_and(|target| chain.contains(&target))))
                || (section != "Tags" && chain.contains(&key))
            {
                records.push_str(&format!("{key}={value}\n"));
            }
        }
    }
    let excerpt = crate::map::map_file::MapFile::from_bytes(
        format!(
            "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
                 [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n{records}"
        )
        .as_bytes(),
    )
    .unwrap();
    // Keep the selected Country, House and type data from the actual complete
    // scenario load; only the trigger arena is narrowed.
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let mut sim = Simulation::new();
    crate::sim::scenario_bootstrap::initialize_map_roster_houses(
        &mut sim,
        &roster,
        &rules,
        Some(&map.ini),
    );
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.get(selected).unwrap();
    assert_eq!(
        sim.house_by_country_index(crate::rules::ruleset::CountryIdx(17), &rules),
        Some(owner)
    );
    let eva = crate::rules::sound_ini::EvaRegistry::from_ini(
        &IniFile::from_bytes(assets.get_ref("EVAMD.INI").unwrap()).unwrap(),
    );
    let cue = eva.index_of("Mis_xA5_EvaBlackEagles").unwrap();
    sim.initialize_trigger_definitions(&excerpt, &Default::default(), Some(&eva), None);
    sim.classify_map_triggers(&rules);
    let tag_type = sim.trigger_runtime.definitions.tag_type(tag_id).unwrap();
    let tag = sim.shared_trigger_tag(tag_type);
    let enabled = |sim: &Simulation, id: &str| {
        let typ = sim
            .trigger_runtime
            .definitions
            .trigger_types
            .iter()
            .position(|typ| typ.id == id)
            .unwrap();
        sim.trigger_runtime.trigger_order.iter().any(|&live| {
            sim.trigger_runtime
                .trigger(live)
                .is_some_and(|trigger| trigger.type_index == typ && trigger.enabled)
        })
    };
    assert!(!enabled(&sim, chain[1]) && !enabled(&sim, chain[2]));
    let id = sim
        .spawn_object_limbo_at_height(
            "BSUB",
            selected,
            bsub.cell_x,
            bsub.cell_y,
            0,
            bsub.facing,
            &rules,
        )
        .unwrap();
    assert_eq!(sim.houses[&owner].tracking().navy(), 1);
    for (frame, remove, expected) in [
        (0, false, false),
        (7, true, false),
        (8, false, true),
        (16, false, true),
    ] {
        sim.session.set_binary_frame_for_test(frame);
        if remove {
            sim.uninit_with_rules(id, &rules);
            assert_eq!(sim.houses[&owner].tracking().navy(), 1);
            sim.flush_pending_delete();
        }
        sim.raise_house_tags(owner, &rules, None, FrameEffects::default());
        sim.flush_pending_delete();
        assert_eq!(enabled(&sim, chain[1]), expected, "frame{frame}");
        assert_eq!(enabled(&sim, chain[2]), expected, "frame{frame}");
        assert_eq!(
            sim.sound_events
                .iter()
                .filter(
                    |event| matches!(event, SimSoundEvent::TriggerEva { index } if *index == cue)
                )
                .count(),
            usize::from(expected)
        );
    }
    assert!(sim.trigger_runtime.tag(tag).is_none());
    assert_eq!(
        sim.trigger_runtime
            .action_trace
            .iter()
            .map(|(kind, _)| *kind)
            .collect::<Vec<_>>(),
        [53, 21, 53]
    );
}

fn funds_event_corpus() -> &'static Value {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/trigger_funds_events.json");
    corpus
}

#[test]
fn funds_events_match_original_signed_wallet_predicates() {
    use super::tests::{
        event_ingress_fixture, install_event_ingress_map, spawn_event_ingress_unit,
    };
    let corpus = funds_event_corpus();
    let cases = corpus["predicate_controls"].as_array().unwrap();
    assert_eq!(cases.len(), 32);
    for case in cases {
        let (mut sim, rules, _, houses) = event_ingress_fixture();
        let owner = case["incoming_house"]
            .as_u64()
            .map(|index| houses[index as usize]);
        if let Some(owner) = owner {
            crate::sim::credit_income::add_credits(
                &mut sim.houses,
                owner,
                integer(&case["balance"]),
            );
        }
        let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[2], (15, 15));
        install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nFUNDS=Country0,<none>,Funds,0,1,1,1,0\n\
                 [Events]\nFUNDS=1,{},0,{}\n[Tags]\nTAG=2,Funds,FUNDS\n",
                integer(&case["kind"]),
                integer(&case["threshold"])
            ),
        );
        let typ = sim.trigger_runtime.definitions.tag_type("TAG").unwrap();
        let tag = sim.shared_trigger_tag(typ);
        let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
        let trigger_type = sim.trigger_runtime.trigger(live).unwrap().type_index;
        sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0].retained_house =
            Some(houses[2]);
        let timer = sim.trigger_runtime.trigger(live).unwrap().timer;
        let rng_before = (
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        let mut persistence = integer(&case["initial_persistence"]) != 0;
        let mut predicate = |sim: &mut Simulation| {
            sim.has_occurred(
                0,
                TagEventInput {
                    kind: integer(&case["raised"]),
                    actor: Some(actor),
                    source: None,
                    cell: (0, 0),
                    force: false,
                },
                owner,
                &mut persistence,
                &rules,
                live,
            )
        };
        let result = if case["scenario_init"].as_bool().unwrap() {
            sim.with_buildup_bypass(predicate)
        } else {
            predicate(&mut sim)
        };
        assert_eq!(result, integer(&case["result"]) != 0, "{case}");
        assert_eq!(persistence, integer(&case["persistence"]) != 0, "{case}");
        assert_eq!(sim.trigger_runtime.trigger(live).unwrap().timer, timer);
        assert_eq!(
            sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0].retained_house,
            Some(houses[2]),
            "funds conditions must not replace the retained House: {case}"
        );
        assert_eq!(
            (
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state(),
            ),
            rng_before,
            "{case}"
        );
    }
}

#[test]
fn funds_events_match_original_house_logic_and_cleanup_histories() {
    use super::tests::{event_ingress_fixture, install_event_ingress_map};
    let corpus = funds_event_corpus();
    let histories = corpus["histories"].as_array().unwrap();
    assert_eq!(histories.len(), 5);
    for case in histories {
        let (mut sim, rules, overlay, houses) = event_ingress_fixture();
        let events = format!(
            "{},0,{}",
            integer(&case["kind"]),
            integer(&case["threshold"])
        );
        let mixed = case["with_elapsed"].as_bool().unwrap();
        let events = if mixed {
            format!("2,13,0,250,{events}")
        } else {
            format!("1,{events}")
        };
        install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nFUNDS=Country0,<none>,Funds,0,1,1,1,0\n\
                 [Events]\nFUNDS={events}\n[Actions]\nFUNDS=1,28,0,7,0,0,0,0,A\n\
                 [Tags]\nTAG={},Funds,FUNDS\n",
                integer(&case["repeat"])
            ),
        );
        crate::sim::credit_income::add_credits(
            &mut sim.houses,
            houses[0],
            integer(&case["initial"]),
        );
        let tag = sim.trigger_runtime.house_tags[&houses[0]][0];
        let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
        // The two other registered Houses include a second Country0 instance.
        // Registration and RegisterEvent must keep using its first House.
        assert!(!sim.trigger_runtime.house_tags.contains_key(&houses[2]));
        let rng_before = (
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        for step in case["steps"].as_array().unwrap() {
            sim.session
                .set_binary_frame_for_test(integer(&step["frame"]) as u32);
            if let Some(mutation) = step["mutation"].as_array() {
                let amount = integer(&mutation[1]);
                match mutation[0].as_str().unwrap() {
                    "spend_money" => {
                        crate::sim::credit_income::spend_money(&mut sim.houses, houses[0], amount)
                    }
                    "add_credits" => {
                        crate::sim::credit_income::add_credits(&mut sim.houses, houses[0], amount)
                    }
                    other => panic!("unknown native wallet operation {other}"),
                }
            }
            if mixed {
                // Logic runs before House; the original mixed trigger can
                // succeed on frame3750, which is not an eighth-frame poll.
                sim.advance_triggers(&rules, Some(&overlay), FrameEffects::default());
            }
            sim.raise_house_tags(houses[0], &rules, Some(&overlay), FrameEffects::default());
            let runtime = &sim.trigger_runtime;
            let trigger = runtime.trigger(live).unwrap();
            let actual = json!({
                "wallet": crate::sim::credit_income::available_money(&sim.houses, houses[0]),
                "spent": sim.houses[&houses[0]].economy().spent_credits(),
                "actions": runtime.action_trace.len(),
                "house_tag_count": runtime.house_tags.get(&houses[0]).map_or(0, Vec::len),
                "logic_tag_count": runtime.logic_tags.len(),
                "tag_pending": i32::from(runtime.tag(tag).unwrap().pending),
                "trigger_pending": i32::from(trigger.pending),
                "timer_start": trigger.timer.start_frame(),
                "timer_duration": trigger.timer.duration(),
                "completed": trigger.completed,
                "pending_count": sim.substrate.pending_delete.len()
            });
            for (key, value) in actual.as_object().unwrap() {
                assert_eq!(
                    value, &step[key],
                    "{} frame{}: {key}",
                    case["name"], step["frame"]
                );
            }
        }
        if integer(&case["repeat"]) == 0 {
            sim.process_pending_delete_with(
                &rules,
                Some(&overlay),
                FrameEffects::default(),
                0,
                false,
            );
            sim.session.set_binary_frame_for_test(40);
            sim.raise_house_tags(houses[0], &rules, Some(&overlay), FrameEffects::default());
        }
        let cleanup = json!({
            "wallet": crate::sim::credit_income::available_money(&sim.houses, houses[0]),
            "spent": sim.houses[&houses[0]].economy().spent_credits(),
            "house_tag_count": sim.trigger_runtime.house_tags.get(&houses[0]).map_or(0, Vec::len),
            "logic_tag_count": sim.trigger_runtime.logic_tags.len(),
            "pending_count": sim.substrate.pending_delete.len(),
            "free_order": sim.trigger_runtime.freed_trace,
            "tags_remaining": sim.trigger_runtime.tag_order.len()
        });
        assert_eq!(cleanup, case["cleanup"], "{}", case["name"]);
        assert_eq!(
            (
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state()
            ),
            rng_before,
            "{} must preserve all three RNG streams",
            case["name"]
        );
    }
}

#[test]
fn funds_event_sov06_retail_cash_taunt_reaches_named_eva_once() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let raw = assets.get_ref("SOV06LMD.MAP").unwrap();
    assert_eq!(
        crate::util::sha256::sha256_hex(raw),
        "f55b65fd33a53dd03f61b48259d7732d260a4f97170e6dcf0767a61af82403a4"
    );
    let map = crate::map::map_file::MapFile::from_bytes(raw).unwrap();
    let named = assets.resolve_ref("SOV06LMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "SOV06LMD.INI")
            .unwrap()
            .ini
    });
    let mut rules_owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, _, _, _) = rules_owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let eva = crate::rules::sound_ini::EvaRegistry::from_ini(
        &IniFile::from_bytes(assets.get_ref("EVAMD.INI").unwrap()).unwrap(),
    );
    let cue = eva.index_of("Mis_xs6_YuriCashTaunt").unwrap();
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let mut sim = Simulation::with_seed(31);
    // Use the production roster and Credits reader without a campaign option
    // adjustment. This isolates the retail taunt, not the whole mission.
    crate::sim::scenario_bootstrap::initialize_map_roster_houses(
        &mut sim,
        &roster,
        &rules,
        Some(&map.ini),
    );
    let owner = sim.interner.get("Russians").unwrap();
    assert_eq!(
        crate::sim::credit_income::available_money(&sim.houses, owner),
        70000
    );
    sim.initialize_trigger_definitions(&map, &Default::default(), Some(&eva), None);
    let typ = sim
        .trigger_runtime
        .definitions
        .tag_type("0D6BF7EC")
        .unwrap();
    let tag = sim.shared_trigger_tag(typ);
    let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
    let definition = &sim.trigger_runtime.definitions.trigger_types
        [sim.trigger_runtime.trigger(live).unwrap().type_index];
    assert_eq!(definition.id, "0D6BF95C");
    assert_eq!(
        (
            definition.events[0].condition.kind,
            definition.events[0].condition.value
        ),
        (52, 10000)
    );
    for (spend, expected_cues) in [(59999, 0), (1, 1), (0, 1)] {
        crate::sim::credit_income::spend_money(&mut sim.houses, owner, spend);
        sim.dispatch_live_tag_event(
            tag,
            TagEventInput {
                kind: EVENT_ANY,
                actor: None,
                source: None,
                cell: (0, 0),
                force: false,
            },
            &mut Vec::new(),
            &rules,
            None,
            FrameEffects::default(),
        );
        assert_eq!(
            sim.sound_events
                .iter()
                .filter(
                    |event| matches!(event, SimSoundEvent::TriggerEva { index } if *index == cue)
                )
                .count(),
            expected_cues
        );
    }
    assert_eq!(sim.trigger_runtime.action_trace, [(21, cue)]);
}

#[test]
fn building_loss_event_matches_original_tracked_count_and_country_order() {
    use super::tests::{
        event_ingress_fixture, install_event_ingress_map, spawn_event_ingress_unit,
    };
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_building_loss_events.json",
    ))
    .unwrap();
    let controls = corpus["controls"].as_array().unwrap();
    assert_eq!(controls.len(), 14);
    for case in controls {
        let (mut sim, rules, _, houses) = event_ingress_fixture();
        sim.houses.reorder_for_test(
            &case["house_order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|index| houses[integer(index) as usize])
                .collect::<Vec<_>>(),
        );
        for (index, owner) in houses.iter().enumerate() {
            sim.houses
                .get_mut(owner)
                .unwrap()
                .tracking_mut()
                .set_buildings_for_test(integer(&case["tracked_buildings"][index]));
        }
        let actor = case["actor_owner"].as_u64().map(|index| {
            spawn_event_ingress_unit(&mut sim, &rules, houses[index as usize], (15, 15))
        });
        install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nLOSS=Country0,<none>,Loss,0,1,1,1,0\n\
             [Events]\nLOSS=1,10,0,{}\n[Tags]\nTAG=2,Loss,LOSS\n",
                integer(&case["value"])
            ),
        );
        let typ = sim.trigger_runtime.definitions.tag_type("TAG").unwrap();
        let tag = sim.shared_trigger_tag(typ);
        let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
        let trigger_type = sim.trigger_runtime.trigger(live).unwrap().type_index;
        sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0].retained_house =
            Some(houses[1]);
        let timer = sim.trigger_runtime.trigger(live).unwrap().timer;
        let rng = (
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        let mut persistence = integer(&case["initial_persistence"]) != 0;
        let mut predicate = |sim: &mut Simulation| {
            sim.has_occurred(
                0,
                TagEventInput {
                    kind: integer(&case["raised"]),
                    actor,
                    source: None,
                    cell: (0, 0),
                    force: false,
                },
                case["incoming_house"]
                    .as_u64()
                    .map(|index| houses[index as usize]),
                &mut persistence,
                &rules,
                live,
            )
        };
        let result = if case["scenario_init"].as_bool().unwrap() {
            sim.with_buildup_bypass(predicate)
        } else {
            predicate(&mut sim)
        };
        assert_eq!(result, integer(&case["result"]) != 0, "{case}");
        assert_eq!(persistence, integer(&case["persistence"]) != 0, "{case}");
        assert_eq!(sim.trigger_runtime.trigger(live).unwrap().timer, timer);
        assert_eq!(
            sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0].retained_house,
            Some(houses[1]),
            "{case}"
        );
        assert_eq!(
            (
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state()
            ),
            rng
        );
    }
}

#[test]
fn building_loss_event_retail_all04_sets_the_rescue_route_local() {
    use crate::rules::{process_owner::NativeScenarioRulesPrefix, ruleset::CountryIdx};
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let raw = assets.get_ref("ALL04DMD.MAP").unwrap();
    assert_eq!(
        crate::util::sha256::sha256_hex(raw),
        "f1621a21e7221c1e475a52dccd4a04bb3125f8a095377c917e2122019f2157b2"
    );
    let map = crate::map::map_file::MapFile::from_bytes(raw).unwrap();
    let named = assets.resolve_ref("ALL04DMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL04DMD.INI")
            .unwrap()
            .ini
    });
    let mut rules_owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, _, _, _) = rules_owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let mut sim = Simulation::with_seed(31);
    crate::sim::scenario_bootstrap::initialize_map_roster_houses(
        &mut sim,
        &roster,
        &rules,
        Some(&map.ini),
    );
    let owner = sim.house_by_country_index(CountryIdx(9), &rules).unwrap();
    assert_eq!(sim.interner.resolve(owner), "YuriCountry");
    sim.initialize_trigger_definitions(&map, &Default::default(), None, None);
    let typ = sim
        .trigger_runtime
        .definitions
        .tag_type("07C973FC")
        .unwrap();
    let tag = sim.shared_trigger_tag(typ);
    let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
    let definition = &sim.trigger_runtime.definitions.trigger_types
        [sim.trigger_runtime.trigger(live).unwrap().type_index];
    assert_eq!(definition.id, "07C9797C");
    assert_eq!(
        (
            definition.events[0].condition.kind,
            definition.events[0].condition.value
        ),
        (10, 9)
    );
    assert!(!sim.trigger_runtime.locals_set.contains(&10));
    // Isolate the authored retail route. Actual constructor/Limbo/destructor
    // accounting is exercised separately through the ordinary master frame.
    for (buildings, expected) in [(1, false), (0, true), (1, true)] {
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .tracking_mut()
            .set_buildings_for_test(buildings);
        sim.dispatch_live_tag_event(
            tag,
            TagEventInput {
                kind: EVENT_ANY,
                ..Default::default()
            },
            &mut Vec::new(),
            &rules,
            None,
            FrameEffects::default(),
        );
        assert_eq!(sim.trigger_runtime.locals_set.contains(&10), expected);
    }
    assert_eq!(sim.trigger_runtime.action_trace, [(56, 10)]);
    // The later actual rescue trigger reads the same local. It is initially
    // disabled; this fixture does not claim to play the full rescue sequence.
    let rescue = sim
        .trigger_runtime
        .definitions
        .trigger_types
        .iter()
        .find(|typ| typ.id == "07BC56CC")
        .unwrap();
    assert!(
        rescue
            .events
            .iter()
            .any(|event| event.condition.kind == 36 && event.condition.value == 10)
    );
}

#[test]
fn damage_source_event44_matches_original_registered_house_index() {
    use super::tests::{
        event_ingress_fixture, install_event_ingress_map, spawn_event_ingress_unit,
    };
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_damage_source_events.json",
    ))
    .unwrap();
    assert_eq!(corpus["house_indices"], json!([0, 1, 2]));
    assert_eq!(corpus["country_indices"], json!([0, 7, 0]));
    assert_ne!(corpus["actor"], corpus["source_object"]);
    assert_eq!(corpus["code_unchanged"], true);
    assert_eq!(corpus["timer_bytes_unchanged"], true);
    let mut compared = 0;
    for case in corpus["predicate_controls"].as_array().unwrap() {
        let (mut sim, rules, _, houses) = event_ingress_fixture();
        assert_eq!(sim.houses.order(), houses);
        sim.session.set_binary_frame_for_test(123);
        let actor = case["inputs"]["actor_owner"].as_u64().map(|index| {
            spawn_event_ingress_unit(&mut sim, &rules, houses[index as usize], (15, 15))
        });
        let source = case["inputs"]["source_owner"].as_u64().map(|index| {
            spawn_event_ingress_unit(&mut sim, &rules, houses[index as usize], (17, 15))
        });
        if let (Some(actor), Some(source)) = (actor, source) {
            assert_ne!(
                actor, source,
                "the ABI's actor and source are separate objects"
            );
        }
        install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nDAMAGE=Country0,<none>,Damage,0,1,1,1,0\n\
                 [Tags]\nTAG=2,Damage,DAMAGE\n[Events]\nDAMAGE={}\n",
                case["raw"].as_str().unwrap()
            ),
        );
        let typ = sim.trigger_runtime.definitions.tag_type("TAG").unwrap();
        let tag = sim.shared_trigger_tag(typ);
        let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
        let trigger_type = sim.trigger_runtime.trigger(live).unwrap().type_index;
        sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0].retained_house =
            Some(houses[2]);
        let timer = sim.trigger_runtime.trigger(live).unwrap().timer;
        let mut persistence = false;
        let mut predicate = |sim: &mut Simulation| {
            sim.has_occurred(
                0,
                TagEventInput {
                    kind: integer(&case["inputs"]["raised"]),
                    actor,
                    source,
                    cell: (0, 0),
                    force: false,
                },
                case["inputs"]["incoming_house"]
                    .as_u64()
                    .map(|index| houses[index as usize]),
                &mut persistence,
                &rules,
                live,
            )
        };
        let result = if case["inputs"]["scenario_init"].as_bool().unwrap() {
            sim.with_buildup_bypass(predicate)
        } else {
            predicate(&mut sim)
        };
        assert_eq!(result, integer(&case["result"]) != 0, "{case}");
        assert_eq!(persistence, integer(&case["persistence"]) != 0, "{case}");
        assert_eq!(integer(&case["event54"]) as u32, 0xABCD1234, "{case}");
        assert_eq!(
            sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0].retained_house,
            Some(houses[2]),
            "original Event44 does not write its retained-House field: {case}"
        );
        assert_eq!(sim.trigger_runtime.trigger(live).unwrap().timer, timer);
        compared += 1;
    }
    // The original whole HasOccurred44 body and counted Event reader execute
    // on explicit source/actor/House priors. This invokes its production Rust
    // owner, rather than implementing a parallel index/Country comparison.
    // Source ownership writers, Object ReceiveDamage, RegisterEvent, Tag
    // actions and full campaign equivalence remain outside this comparison.
    assert_eq!(compared, 24);
}

#[test]
fn surviving_object_damage_tag_ingress_matches_original_call_admission() {
    use super::tests::{
        event_ingress_damage, event_ingress_fixture, install_event_ingress_map,
        spawn_event_ingress_unit,
    };
    let corpus: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/object_damage_tags.json",
    ))
    .unwrap();
    let mut compared = 0;
    for case in corpus.iter().filter(|case| {
        matches!(
            case["name"].as_str().unwrap(),
            "ordinary" | "no_source" | "no_tag" | "no_damage" | "healing" | "tag_detach"
        )
    }) {
        let (mut sim, rules, overlays, houses) = event_ingress_fixture();
        let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
        let source = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (17, 15));
        sim.substrate
            .entities
            .get_mut(victim)
            .unwrap()
            .health
            .set_for_test(integer(&case["input"]["current"]));
        let detach = case["name"] == "tag_detach";
        install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nDAMAGE=Country0,SOURCE,Damage,0,1,1,1,0\nSOURCE=Country0,<none>,Source,0,1,1,1,0\n\
             [Tags]\nTAG={},Damage,DAMAGE\n[Events]\nDAMAGE=1,6,0,0\nSOURCE=1,44,0,0\n\
             [Actions]\nDAMAGE=1,28,0,6,0,0,0,0,A\nSOURCE=1,28,0,44,0,0,0,0,A\n",
                if detach { 0 } else { 2 }
            ),
        );
        if case["input"]["tag_context"]["tag"].as_bool().unwrap() {
            sim.attach_object_trigger_tag(victim, Some("TAG"));
        }
        let source = if case["input"]["tag_context"]["source"].as_bool().unwrap() {
            source
        } else {
            crate::sim::combat::RAD_NO_ATTACKER
        };
        event_ingress_damage(
            &mut sim,
            &rules,
            &overlays,
            victim,
            source,
            integer(&case["input"]["damage"]),
            FrameEffects::default(),
        );
        assert_eq!(
            json!(sim.substrate.entities.get(victim).unwrap().health.current()),
            case["output"]["actual"],
            "{case}"
        );
        let native_kinds: Vec<_> = case["output"]["tag_calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|call| integer(&call["kind"]))
            .collect();
        let rust_kinds: Vec<_> = sim
            .trigger_runtime
            .dispatch_trace
            .iter()
            .map(|call| call.1)
            .collect();
        assert_eq!(rust_kinds, native_kinds, "{case}");
        assert_eq!(
            sim.trigger_runtime.globals_set,
            native_kinds.into_iter().map(|kind| kind as u32).collect(),
            "real Tag Spring actions must run through the world receiver, not just record a callback"
        );
        if detach {
            assert!(
                !sim.trigger_runtime
                    .attachments
                    .contains_key(&TagAttachment::Object(victim))
            );
            assert!(
                sim.substrate
                    .pending_delete
                    .retained_tickets()
                    .any(|ticket| matches!(ticket, DeferredDeleteTicket::Tag(_)))
            );
        }
        compared += 1;
    }
    // Only generic Object HP/call admission is native-compared here. Native
    // Tag6E53A0 is a recorded sink. Rust uses real predicates, Spring and
    // repeat0 cleanup; lethal RecordKill6/sourceNULL is a separate receiver
    // entry and the callback-only alive-clear control is deliberately excluded.
    assert_eq!(compared, 6);
}

#[test]
fn entered_cell_predicates_match_original_country_and_retained_house_identity() {
    use super::tests::{
        event_ingress_fixture, install_event_ingress_map, spawn_event_ingress_unit,
    };
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_entered_cell_events.json",
    ))
    .unwrap();
    let native_houses = corpus["houses"].as_array().unwrap();
    let mut compared = 0;
    for case in corpus["predicate_controls"].as_array().unwrap() {
        let (mut sim, rules, _, houses) = event_ingress_fixture();
        let actor = case["inputs"]["actor_owner"].as_u64().map(|index| {
            spawn_event_ingress_unit(&mut sim, &rules, houses[index as usize], (15, 15))
        });
        install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nENTER=Country0,<none>,Enter,0,1,1,1,0\n[Tags]\nTAG=2,Enter,ENTER\n\
             [Events]\nENTER={}\n",
                case["raw"].as_str().unwrap()
            ),
        );
        let typ = sim.trigger_runtime.definitions.tag_type("TAG").unwrap();
        let tag = sim.shared_trigger_tag(typ);
        let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
        let trigger_type = sim.trigger_runtime.trigger(live).unwrap().type_index;
        sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0].retained_house =
            Some(houses[1]);
        let timer = sim.trigger_runtime.trigger(live).unwrap().timer;
        let mut persistence = false;
        let mut predicate = |sim: &mut Simulation| {
            sim.has_occurred(
                0,
                TagEventInput {
                    kind: integer(&case["inputs"]["raised"]),
                    actor,
                    source: None,
                    cell: (0, 0),
                    force: false,
                },
                case["inputs"]["incoming_house"]
                    .as_u64()
                    .map(|index| houses[index as usize]),
                &mut persistence,
                &rules,
                live,
            )
        };
        let result = if case["inputs"]["scenario_init"].as_bool().unwrap() {
            sim.with_buildup_bypass(predicate)
        } else {
            predicate(&mut sim)
        };
        assert_eq!(result, integer(&case["result"]) != 0, "{case}");
        assert_eq!(persistence, integer(&case["persistence"]) != 0, "{case}");
        let native_retained = native_houses.iter().position(|pointer| {
            pointer.as_u64().unwrap() as u32 == integer(&case["event54"]) as u32
        });
        if native_retained.is_none() {
            assert_eq!(
                integer(&case["event54"]) as u32,
                0xABCD1234,
                "unmapped native Event+54 must remain the supplied sentinel: {case}"
            );
        }
        let expected_retained = native_retained.map_or(houses[1], |index| houses[index]);
        assert_eq!(
            sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0].retained_house,
            Some(expected_retained),
            "{case}"
        );
        assert_eq!(sim.trigger_runtime.trigger(live).unwrap().timer, timer);
        compared += 1;
    }
    // Native HasOccurred71E940 only, with explicit House/actor storage priors.
    // Actual PerCell -> Tag -> 8997 actions are covered in the sibling test.
    assert_eq!(compared, 16);
}

#[test]
fn per_cell_arrival_uses_native_bridge_admission_and_keeps_the_cached_tag() {
    use super::tests::{
        event_ingress_fixture, install_event_ingress_map, spawn_event_ingress_unit,
    };
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_entered_cell_events.json",
    ))
    .unwrap();
    let mut compared = 0;
    for case in corpus["arrival_controls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| {
            !matches!(
                case["name"].as_str().unwrap(),
                "reread_cell_after_move" | "no_alive_gate_between_raises"
            )
        })
    {
        let (mut sim, rules, overlays, houses) = event_ingress_fixture();
        let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
        sim.substrate
            .entities
            .get_mut(actor)
            .unwrap()
            .set_on_bridge(case["inputs"]["on_bridge"].as_bool().unwrap());
        let cached = case["name"] == "cached_after_detach";
        install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nENTER=Country0,LATER,Enter,0,1,1,1,0\nLATER=Country0,<none>,Later,0,1,1,1,0\n\
             [Tags]\nTAG={},Enter,ENTER\n[Events]\nENTER=1,1,0,0\nLATER=1,59,0,0\n\
             [Actions]\nENTER=1,28,0,1,0,0,0,0,A\nLATER=1,28,0,9,0,0,0,0,A\n\
             [CellTags]\n{}",
                if cached { 0 } else { 2 },
                if case["inputs"]["tag"].as_bool().unwrap() {
                    "15015=TAG\n"
                } else {
                    ""
                }
            ),
        );
        let cell =
            crate::sim::world::Simulation::trigger_cell_identity(&sim.resolved_terrain, (15, 15));
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .write_native_cell_flags(cell, case["inputs"]["flags"].as_u64().unwrap() as u32);
        sim.per_cell_process(
            actor,
            crate::sim::movement::PerCellReason::TurnComplete,
            &rules,
            Some(&overlays),
            FrameEffects::default(),
        )
        .unwrap();
        assert!(sim.trigger_runtime.dispatch_trace.is_empty());
        sim.per_cell_process(
            actor,
            crate::sim::movement::PerCellReason::Arrival,
            &rules,
            Some(&overlays),
            FrameEffects::default(),
        )
        .unwrap();
        let native: Vec<_> = case["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|call| integer(&call["kind"]))
            .collect();
        let actual: Vec<_> = sim
            .trigger_runtime
            .dispatch_trace
            .iter()
            .map(|call| call.1)
            .collect();
        assert_eq!(actual, native, "{case}");
        // These are the two authored action values, not a second admission
        // calculation. Queuing a zero-ref Tag does not destroy it inline, so
        // its other linked Event59 trigger still springs from the cached pointer.
        let expected_effects: BTreeSet<u32> = native
            .into_iter()
            .map(|kind| if kind == 1 { 1 } else { 9 })
            .collect();
        assert_eq!(sim.trigger_runtime.globals_set, expected_effects, "{case}");
        if cached {
            assert!(!sim.trigger_runtime.cell_tags.contains_key(&cell));
            assert_eq!(
                sim.trigger_runtime.dispatch_trace[0].0, sim.trigger_runtime.dispatch_trace[1].0,
                "59 must call the retained Tag after its attachment was removed by real repeat0 cleanup"
            );
        }
        compared += 1;
    }
    // The native slice records only Tag call admission. Rust starts at the
    // actual Unit per-cell override and runs real Tag actions; callback-driven
    // coordinate/alive mutations in two native sink rows remain outside this
    // comparison. No Fly ingress or complete campaign parity is claimed.
    assert_eq!(compared, 6);
}

#[test]
fn fresh_line_marker_writer_and_arrival_consumers_match_original_sparse_cells() {
    use super::tests::{install_event_ingress_map, sparse_line_ingress_fixture};
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_line_events.json",
    ))
    .unwrap();
    let mut compared_markers = 0;
    let mut compared_calls = 0;
    for case in corpus["controls"].as_array().unwrap() {
        let (mut sim, rules, _, actor) = sparse_line_ingress_fixture();
        let cells: Vec<_> = case["input"]["cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|xy| (integer(&xy[0]), integer(&xy[1])))
            .collect();
        for &(x, y) in &cells {
            let identity = crate::sim::world::Simulation::trigger_cell_identity(
                &sim.resolved_terrain,
                (x as i16, y as i16),
            );
            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .write_native_cell_flags(identity, 4);
        }
        let mut triggers = String::new();
        let mut events = String::new();
        let mut actions = String::new();
        let mut tags = String::new();
        let mut attachments = String::new();
        for (ordinal, definition) in case["input"]["definitions"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let kinds = definition["events"].as_array().unwrap();
            for (node, kind) in kinds.iter().enumerate() {
                let id = format!("LINE{ordinal}_{node}");
                let next = if node + 1 < kinds.len() {
                    format!("LINE{ordinal}_{}", node + 1)
                } else {
                    "<none>".to_owned()
                };
                triggers.push_str(&format!("{id}=Country0,{next},Line,0,1,1,1,0\n"));
                events.push_str(&format!("{id}=1,{},0,0\n", integer(kind)));
                actions.push_str(&format!("{id}=1,28,0,{},0,0,0,0,A\n", 20 + ordinal));
            }
            tags.push_str(&format!("TAG{ordinal}=2,Line{ordinal},LINE{ordinal}_0\n"));
            attachments.push_str(&format!(
                "{}=TAG{ordinal}\n",
                integer(&definition["cell"][1]) * 1000 + integer(&definition["cell"][0])
            ));
        }
        install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\n{triggers}[Events]\n{events}[Actions]\n{actions}[Tags]\n{tags}[CellTags]\n{attachments}"
            ),
        );
        let mut terrain = sim.resolved_terrain.take().unwrap();
        let bounds =
            crate::sim::scenario_bootstrap::NativeStartBounds::from_session(&sim, &terrain);
        sim.trigger_runtime.initialize_cell_trigger_lines(
            bounds,
            &mut terrain,
            cells.iter().copied(),
        );
        let flags: Vec<_> = cells
            .iter()
            .map(|&(x, y)| {
                terrain.native_cell_flags(terrain.native_cell_identity((x as i16, y as i16)))
            })
            .collect();
        assert_eq!(json!(flags), case["line_flags"], "{case}");
        sim.resolved_terrain = Some(terrain);
        compared_markers += 1;
        if case["input"]["mutation"] != "none" {
            continue;
        }
        let tag_ids: Vec<_> = (0..case["input"]["definitions"].as_array().unwrap().len())
            .map(|ordinal| {
                let typ = sim
                    .trigger_runtime
                    .definitions
                    .tag_type(&format!("TAG{ordinal}"))
                    .unwrap();
                sim.shared_trigger_tag(typ)
            })
            .collect();
        sim.per_cell_process(
            actor,
            crate::sim::movement::PerCellReason::Arrival,
            &rules,
            None,
            FrameEffects::default(),
        )
        .unwrap();
        // Native starts at the line slice, after direct1/59. Remove only the
        // preceding direct calls from the Rust whole-PerCell observation.
        let actual: Vec<_> = sim
            .trigger_runtime
            .dispatch_trace
            .iter()
            .filter(|call| matches!(call.1, 25 | 26))
            .map(|&(tag, kind)| json!([tag_ids.iter().position(|id| *id == tag).unwrap(), kind]))
            .collect();
        let expected: Vec<_> = case["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|call| json!([call["tag"], call["kind"]]))
            .collect();
        assert_eq!(actual, expected, "{case}");
        let expected_globals: BTreeSet<_> = case["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|call| 20 + integer(&call["tag"]) as u32)
            .collect();
        assert_eq!(sim.trigger_runtime.globals_set, expected_globals, "{case}");
        compared_calls += 1;
    }
    // Writer/classifier/Map lookup and caller order run natively; native Tag
    // actions are a sink. Rust exercises actual actions in seven unmoved rows.
    // No Fly producer, zone24 or whole InitCellAttributes comparison is claimed.
    assert_eq!((compared_markers, compared_calls), (9, 7));
}

fn native_rng_hash(hex: &str) -> String {
    let bytes: Vec<_> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    crate::util::sha256::sha256_hex(&bytes)
}

struct All01ChargeInputs {
    rules: crate::rules::ruleset::RuleSet,
    map: crate::map::map_file::MapFile,
    assets: crate::assets::asset_manager::AssetManager,
    fixed_art: crate::rules::ini_parser::IniFile,
}

#[test]
fn all01_due_charge_and_damage_share_the_production_house_power_phase() {
    use crate::map::{entities::EntityCategory, resolved_terrain, waypoints::Waypoint};
    use crate::rules::{ini_parser::IniFile, team_ai_ini::TeamAiIniRegistry};
    use crate::sim::world::TickLane;
    let charge: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_super_charge_percent.json",
    ))
    .unwrap();
    let damage: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_damage_at_waypoint.json",
    ))
    .unwrap();
    let Some(All01ChargeInputs {
        mut rules,
        map,
        mut assets,
        fixed_art,
    }) = all01_charge_inputs(&charge)
    else {
        return;
    };
    assert_eq!(
        charge["retail_inputs"]["map_sha256"],
        damage["retail_inputs"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["file"] == "ALL01UMD.MAP")
            .unwrap()["sha256"]
    );
    let theater = crate::map::theater::load_theater(&mut assets, &map.header.theater)
        .expect("retail theater assets");
    rules.install_art_data(crate::rules::retail_ini_fixture::art_registry(&fixed_art));
    let mut roots = Vec::new();
    for name in ["NANRCT", "YAPPET"] {
        let object = rules.object(name).unwrap();
        let art = rules
            .art()
            .resolve_metadata_entry(&object.id, &object.image)
            .unwrap();
        for slot in &art.building_anims {
            roots.push(slot.anim_type.clone());
            roots.extend(
                slot.damaged_variant
                    .iter()
                    .map(|variant| variant.anim_type.clone()),
            );
            roots.extend(
                slot.garrisoned_variant
                    .iter()
                    .map(|variant| variant.anim_type.clone()),
            );
        }
    }
    roots.extend(
        rules
            .general
            .damage_fire_types
            .iter()
            .map(|anim| anim.name.clone()),
    );
    roots.sort();
    roots.dedup();
    rules
        .bind_scheduler_anim_assets(&roots, &assets, theater.extension, &map.header.theater)
        .expect("physical selected building SHP bounds");
    for name in ["YAPPET_E", "YAPPET_F", "YAPPET_G", "YAPPET_H"] {
        assert!(
            rules
                .art()
                .anim_runtime_config(name)
                .unwrap()
                .raw_shp_frame_count
                .is_some_and(|count| count > 0),
            "{name}"
        );
    }

    // Explicit bounded prior: actual authored records translated together to
    // a clear level0 32x32 map, remaining entities and unrelated actions absent.
    // Startup/difficulty, whole-frame RNG and rendered parity are not claimed.
    let mut records: Vec<_> = map
        .entities
        .iter()
        .filter(|entity| {
            entity.category == EntityCategory::Structure
                && entity.owner == "Confederation"
                && matches!(entity.type_id.as_str(), "NANRCT" | "YAPPET")
        })
        .cloned()
        .collect();
    assert_eq!(records.len(), 2);
    for record in &mut records {
        record.cell_x = record.cell_x.checked_sub(36).unwrap();
        record.cell_y = record.cell_y.checked_sub(59).unwrap();
    }
    let mut sim = Simulation::with_seed(31);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let roster = crate::map::houses::parse_house_roster(&map.ini, &[], Some(&rules));
    crate::sim::scenario_bootstrap::initialize_map_roster_houses(
        &mut sim,
        &roster,
        &rules,
        Some(&map.ini),
    );
    let aimd = IniFile::from_bytes(assets.get_ref("AIMD.INI").unwrap()).unwrap();
    let teams = TeamAiIniRegistry::from_sources(&aimd, &map.ini, false);
    let audio = crate::rules::audio_sources::AudioDefinitions::select(&assets);
    sim.initialize_trigger_definitions(
        &map,
        &teams,
        Some(audio.eva()),
        Some(audio.sounds().as_ref()),
    );
    // Preserve every actual list position. Shrinking the list would change
    // repeat0's synchronous removal/cursor advancement at indices72/107.
    for typ in &mut sim.trigger_runtime.definitions.trigger_types {
        if !matches!(typ.id.as_str(), "0B7C0AEC" | "087857CC") {
            typ.enabled = false;
        }
    }
    let terrain = resolved_terrain::test_grid(32, 32, resolved_terrain::test_clear_cell);
    sim.install_resolved_terrain_for_new_map(terrain.clone());
    let waypoint = &map.waypoints[&364];
    sim.session.install_waypoints(&HashMap::from([(
        364,
        Waypoint {
            index: 364,
            rx: waypoint.rx.checked_sub(36).unwrap(),
            ry: waypoint.ry.checked_sub(59).unwrap(),
        },
    )]));
    assert_eq!(
        sim.spawn_from_map_with_resolved(&records, Some(&rules), Some(&terrain)),
        2
    );
    sim.classify_map_triggers(&rules);
    for (position, expected) in [(72, "0B7C03FC"), (107, "0878524C")] {
        let tag = sim.trigger_runtime.tags[sim.trigger_runtime.logic_tags[position]]
            .as_ref()
            .unwrap();
        assert_eq!(
            sim.trigger_runtime.definitions.tag_types[tag.type_index].id,
            expected
        );
    }
    let owner = sim.interner.get("Confederation").unwrap();
    let dominator = sim.interner.get("PsychicDominatorSpecial").unwrap();
    let reactor = sim
        .entities()
        .iter_sorted()
        .find(|(_, entity)| sim.interner.resolve(entity.type_ref()) == "NANRCT")
        .unwrap()
        .0;
    let building = sim
        .entities()
        .iter_sorted()
        .find(|(_, entity)| sim.interner.resolve(entity.type_ref()) == "YAPPET")
        .unwrap()
        .0;
    let identity = sim.super_weapons[&owner][&dominator].native_unique_id();
    for _ in 0..15 {
        sim.advance_app_frame(
            &[],
            &rules,
            None,
            67,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
    }
    assert_eq!(sim.session.binary_frame(), 15);
    assert_eq!(sim.entities().get(reactor).unwrap().health.current(), 769);
    assert_eq!(
        (
            sim.power_states[&owner].total_output(),
            sim.power_states[&owner].total_drain()
        ),
        (1537, 1000)
    );
    assert!(!sim.super_weapons[&owner][&dominator].is_suspended());
    sim.trigger_runtime.action_trace.clear();
    sim.advance_app_frame(
        &[],
        &rules,
        None,
        67,
        TickLane::Ordinary,
        FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(sim.trigger_runtime.action_trace, vec![(129, 7), (63, 364)]);
    let reactor = sim.entities().get(reactor).unwrap();
    assert_eq!(
        (
            reactor.health.current(),
            reactor.building_power_health_sample()
        ),
        (269, Some(269))
    );
    assert!(reactor.building_art.damage_state_active());
    let golden = charge["house_refresh_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["power_drop_frame"] == 15 && row["input_low_power_at_grant"] == false)
        .unwrap()["rows"][0]
        .clone();
    assert_eq!(
        json!(sim.power_states[&owner].total_output()),
        golden["retained_output"]
    );
    assert_eq!(
        json!(sim.power_states[&owner].total_drain()),
        golden["retained_drain"]
    );
    assert!(sim.power_states[&owner].is_low_power());
    let instance = &sim.super_weapons[&owner][&dominator];
    assert_eq!(
        json!({"start": instance.charge_start_tick(), "duration": instance.charge_duration(),
        "granted": instance.is_active(), "ready": instance.is_ready(), "held": instance.is_suspended(),
        "custom_recharge": -1}),
        golden["output"]
    );
    assert_eq!(instance.native_unique_id(), identity);
    let super_slots = |sim: &Simulation| {
        sim.entities()
            .get(building)
            .unwrap()
            .building_art
            .anim_slots()
            .iter()
            .enumerate()
            .filter(|(slot, _)| (14..=17).contains(slot))
            .filter_map(|(_, id)| {
                (*id).and_then(|id| sim.anim(id)).map(|anim| {
                    (
                        anim.type_id(),
                        anim.runtime().current_frame,
                        anim.runtime().paused,
                    )
                })
            })
            .collect::<Vec<_>>()
    };
    let before_pause = super_slots(&sim);
    assert!(!before_pause.is_empty());
    assert!(before_pause.iter().all(|(_, _, paused)| !paused));
    sim.trigger_runtime.action_trace.clear();
    sim.advance_app_frame(
        &[],
        &rules,
        None,
        67,
        TickLane::Ordinary,
        FrameEffects::default(),
    )
    .unwrap();
    let paused = super_slots(&sim);
    assert!(!paused.is_empty());
    assert!(paused.iter().all(|(_, _, paused)| *paused));
    assert!(sim.trigger_runtime.action_trace.is_empty());
    assert_eq!(
        sim.super_weapons[&owner][&dominator].charge_duration(),
        integer(&golden["output"]["duration"])
    );
}

fn all01_charge_inputs(corpus: &Value) -> Option<All01ChargeInputs> {
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let raw_map = assets.get_ref("ALL01UMD.MAP").expect("retail ALL01");
    let map = crate::map::map_file::MapFile::from_bytes(raw_map).unwrap();
    assert_eq!(
        crate::util::sha256::sha256_hex(raw_map),
        corpus["retail_inputs"]["map_sha256"]
    );
    for (name, record) in [("RULESMD.INI", "rulesmd"), ("ARTMD.INI", "artmd")] {
        assert_eq!(
            crate::util::sha256::sha256_hex(assets.get_ref(name).unwrap()),
            corpus["retail_inputs"][record]["sha256"],
            "{name} byte identity"
        );
    }
    let language = corpus["retail_inputs"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["file"] == "LANGRULE.INI")
        .unwrap();
    let language_bytes = assets.get_ref("LANGRULE.INI");
    assert_eq!(
        language_bytes.is_none(),
        language["absent"].as_bool().unwrap_or(false)
    );
    if let Some(bytes) = language_bytes {
        assert_eq!(crate::util::sha256::sha256_hex(bytes), language["sha256"]);
    }
    // The ordinary campaign owner includes the optional scenario-basename
    // prepass. Do not confuse a missing extracted file with archive absence.
    let named = assets.resolve_ref("ALL01UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL01UMD.INI")
            .expect("present campaign INI parses")
            .ini
    });
    assert_eq!(
        json!(named.is_some()),
        corpus["retail_inputs"]["scenario_ini"]["selected_input_present"],
        "campaign INI selection matches the native input boundary"
    );
    let mut rules_owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, _, fixed_art, _) = rules_owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    Some(All01ChargeInputs {
        rules,
        map,
        assets,
        fixed_art,
    })
}

#[test]
fn native_all01_super_charge_dispatch_uses_retail_type_and_retained_operands() {
    use crate::sim::{house_state::HouseState, superweapon::SuperWeaponInstance};
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_super_charge_percent.json",
    ))
    .unwrap();
    let Some(All01ChargeInputs { rules, map, .. }) = all01_charge_inputs(&corpus) else {
        return;
    };
    let dominator_name = &rules.super_weapon_order[7];
    let sw = rules.super_weapon(dominator_name).unwrap();
    assert_eq!(dominator_name, "PsychicDominatorSpecial");
    assert_eq!(sw.recharge_time_frames, 9000);
    assert_eq!(
        json!(rules.super_weapon_order),
        corpus["retail_inputs"]["super_type_registry"]
    );
    // The physical map and original Read controls share exactly the same
    // kind/index/auxiliary fields, rather than a hand-written game tuning.
    let authored = map
        .actions
        .values()
        .flat_map(|action| action.entries())
        .find(|entry| entry.kind == ACTION_SET_SUPER_CHARGE_PERCENTAGE)
        .unwrap();
    assert_eq!(
        (authored.literal_value(), authored.auxiliary_value()),
        (Some(7), 90)
    );

    let controls = corpus["dispatch_controls"].as_array().unwrap();
    assert_eq!(controls.len(), 12);
    for row in controls {
        let input = &row["input"];
        let mut sim = Simulation::with_seed(31);
        sim.intern_rule_type_ids(&rules);
        sim.session
            .set_binary_frame_for_test(integer(&input["frame"]) as u32);
        let owner = sim.interner.intern("RetainedHouse");
        let country = sim.interner.intern("Confederation");
        if input["owner_present"].as_bool().unwrap() {
            sim.houses
                .register(HouseState::new(owner, 0, Some(country), false, 0, 10));
        }
        for name in &rules.super_weapon_order {
            let id = sim.interner.intern(name);
            sim.super_weapons
                .map_mut_for_test()
                .entry(owner)
                .or_default()
                .insert(
                    id,
                    SuperWeaponInstance::new(id, owner, sim.session.binary_frame() as i32),
                );
        }
        let id = sim.interner.get(dominator_name).unwrap();
        let instance = sim
            .super_weapons
            .map_mut_for_test()
            .get_mut(&owner)
            .unwrap()
            .get_mut(&id)
            .unwrap();
        instance.set_is_active_for_test(input["granted"].as_bool().unwrap());
        instance.set_is_ready_for_test(input["ready"].as_bool().unwrap());
        instance.set_is_suspended_for_test(input["held"].as_bool().unwrap());
        instance.set_charge_start_tick_for_test(integer(&input["start"]));
        instance.set_charge_duration_for_test(integer(&input["duration"]));
        assert_eq!(integer(&input["recharge"]), sw.recharge_time_frames);
        assert_eq!(
            integer(&input["count"]) as usize,
            rules.super_weapon_order.len()
        );

        let mut definitions = RuntimeDefinitions::default();
        let typ = definitions.trigger_type("CHARGE").unwrap();
        definitions.trigger_types[typ].owner = Some("Confederation".to_owned());
        let ini = crate::rules::ini_parser::IniFile::from_str(&format!(
            "[Actions]\nCHARGE=1,{}\n",
            row["read"]["raw"].as_str().unwrap()
        ));
        let entry = crate::map::actions::parse_actions(&ini)
            .remove("CHARGE")
            .unwrap()
            .entries()[0]
            .clone();
        assert_eq!(json!(entry.auxiliary_value()), row["read"]["auxiliary"]);
        let action = definitions.resolve_action(entry, None, None);
        assert_eq!(json!(action.value), row["read"]["value"]);
        sim.trigger_runtime.definitions = definitions;
        let rng_before = (
            sim.main_rng.native_state_hex(),
            sim.scenario_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        );
        assert_eq!(row["rng_before"], row["rng_after"]);
        assert_eq!(row["rng_calls"], json!([]));
        let returned = sim.apply_live_trigger_action(
            &action,
            sim.trigger_action_context(typ, None, TagEventInput::default(), &rules),
            &mut Vec::new(),
            &rules,
            None,
            FrameEffects::default(),
        );
        assert_eq!(
            u8::from(returned),
            row["returned"].as_u64().unwrap() as u8,
            "{row}"
        );
        let instance = &sim.super_weapons[&owner][&id];
        assert_eq!(
            json!({"start": instance.charge_start_tick(),
            "duration": instance.charge_duration(), "granted": instance.is_active(),
            "ready": instance.is_ready(), "held": instance.is_suspended(),
            "custom_recharge": -1}),
            row["output"],
            "{row}"
        );
        assert_eq!(
            (
                sim.main_rng.native_state_hex(),
                sim.scenario_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex()
            ),
            rng_before
        );
    }
}

#[test]
fn native_damage_at_waypoint_dispatch_matches_original_controls() {
    use crate::map::waypoints::Waypoint;
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_damage_at_waypoint.json",
    ))
    .unwrap();
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[CombatDamage]\nC4Warhead=Super\n[Warheads]\n0=Super\n[Super]\nCellSpread=0\n",
    ))
    .unwrap();
    let controls = corpus["dispatch_controls"].as_array().unwrap();
    assert_eq!(controls.len(), 6);
    for row in controls {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let x = integer(&input["cell"][0]) as u16;
        let y = integer(&input["cell"][1]) as u16;
        let level = input["level"].as_i64().unwrap_or(0) as u8;
        let slope = input["slope"].as_i64().unwrap_or(0) as u8;
        let flags = input["flags"].as_u64().unwrap_or(0) as u32;
        let mut terrain = crate::map::resolved_terrain::test_grid(32, 32, |rx, ry| {
            let mut cell = crate::map::resolved_terrain::test_clear_cell(rx, ry);
            if (rx, ry) == (x, y) {
                cell.level = level;
                cell.slope_type = slope;
                cell.bridge_facts.raw_flags = flags;
            }
            cell
        });
        let identity = terrain.native_cell_identity((x as i16, y as i16));
        terrain.write_native_cell_flags(identity, flags);
        let mut sim = Simulation::with_seed(31);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.install_resolved_terrain_for_new_map(terrain);
        let index = integer(&row["read"]["value"]) as u32;
        sim.session.install_waypoints(&HashMap::from([
            (
                index,
                Waypoint {
                    index,
                    rx: x,
                    ry: y,
                },
            ),
            (
                0,
                Waypoint {
                    index: 0,
                    rx: 20,
                    ry: 20,
                },
            ),
        ]));
        let mut definitions = RuntimeDefinitions::default();
        let typ = definitions.trigger_type("DAMAGE").unwrap();
        let ini = IniFile::from_str(&format!(
            "[Actions]\nDAMAGE=1,{}\n",
            row["read"]["raw"].as_str().unwrap()
        ));
        let entry = crate::map::actions::parse_actions(&ini)
            .remove("DAMAGE")
            .unwrap()
            .entries()[0]
            .clone();
        let action = definitions.resolve_action(entry, None, None);
        assert_eq!(json!(action.value), row["read"]["value"], "{name}: +90");
        sim.trigger_runtime.definitions = definitions;
        let before = (
            sim.main_rng.native_state_hex(),
            sim.scenario_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        );
        let (returned, visits) = crate::sim::combat::receiver_fixture::trace_area_visits(|| {
            sim.apply_live_trigger_action(
                &action,
                sim.trigger_action_context(typ, None, TagEventInput::default(), &rules),
                &mut Vec::new(),
                &rules,
                None,
                FrameEffects::default(),
            )
        });
        assert_eq!(
            u8::from(returned),
            row["returned"].as_u64().unwrap() as u8,
            "{name}"
        );
        let native: Vec<_> = row["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call["kind"] == "area")
            .collect();
        assert_eq!(visits.len(), native.len(), "{name}");
        for (actual, expected) in visits.iter().zip(native) {
            assert_eq!(actual["coord"], expected["coord"], "{name}");
            assert_eq!(actual["damage"], expected["damage"], "{name}");
            assert_eq!(actual["warhead"], "Super", "{name}");
            assert_eq!(actual["no_source"], true, "{name}");
            assert_eq!(actual["no_source_house"], true, "{name}");
        }
        // Native downstream damage/bridge answers are explicit seams in this
        // dispatch family. Real NANRCT receivers are compared in the next test.
        assert_eq!(row["rng_before"], row["rng_after"], "{name}");
        assert_eq!(
            before,
            (
                sim.main_rng.native_state_hex(),
                sim.scenario_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex()
            ),
            "{name}: empty world RNG"
        );
    }
}

#[test]
fn native_all01_reactor_damage_uses_retail_rules_and_shared_power_owner() {
    use crate::map::entities::EntityCategory;
    use crate::map::waypoints::Waypoint;
    use crate::rules::{
        ini_parser::IniFile,
        native_processing::{RulesLayerKind, RulesLayerStack},
        ruleset::RuleSet,
    };
    use crate::sim::{
        building_construction::BuildingBodyMode, combat::Health, game_entity::GameEntity,
        house_state::HouseState, native_identity::NativeUniqueIdCursor, rng::SimRng,
    };
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_damage_at_waypoint.json",
    ))
    .unwrap();
    let Some(raw_rules) = crate::rules::retail_ini_fixture::retail_ini_bytes("rulesmd.ini") else {
        return;
    };
    let Some(raw_art) = crate::rules::retail_ini_fixture::retail_ini_bytes("artmd.ini") else {
        return;
    };
    let Some(raw_map) = crate::rules::retail_ini_fixture::retail_archive_bytes("ALL01UMD.MAP")
    else {
        return;
    };
    let map = crate::map::map_file::MapFile::from_bytes(&raw_map).unwrap();
    for (name, raw) in [("RULESMD.INI", &raw_rules), ("ALL01UMD.MAP", &raw_map)] {
        let native = corpus["retail_inputs"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["file"] == name)
            .unwrap();
        assert_eq!(
            crate::util::sha256::sha256_hex(raw),
            native["sha256"],
            "{name}"
        );
    }
    assert_eq!(
        crate::util::sha256::sha256_hex(&raw_art),
        corpus["retail_inputs"]["artmd"]["sha256"]
    );
    let mut layers = RulesLayerStack::new(IniFile::from_bytes(&raw_rules).unwrap());
    let lang = std::path::PathBuf::from(std::env::var("RA2_DIR").unwrap()).join("LANGRULE.INI");
    if lang.is_file() {
        layers.push(
            RulesLayerKind::LangRule,
            IniFile::from_bytes(&std::fs::read(lang).unwrap()).unwrap(),
        );
    } else {
        assert!(
            corpus["retail_inputs"]["layers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["file"] == "LANGRULE.INI" && row["absent"] == true)
        );
    }
    layers.push(RulesLayerKind::Scenario, map.ini.clone());
    let processed = layers
        .process_with_fixed_art(&IniFile::from_bytes(&raw_art).unwrap())
        .unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    let object = rules.object("NANRCT").unwrap();
    let dimensions = crate::rules::foundation::foundation_dimensions(&object.foundation);
    assert_eq!(
        json!([dimensions.0, dimensions.1]),
        corpus["retail_inputs"]["foundation_dimensions"]
    );
    let authored = map
        .entities
        .iter()
        .find(|entity| entity.type_id == "NANRCT" && entity.owner == "Confederation")
        .unwrap();
    let native = corpus["receiver_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["route"] == "area")
        .unwrap();
    assert_eq!(
        json!(authored.health),
        native["authored_health_native"]["input"]["authored"]
    );
    assert_eq!(
        json!(object.strength),
        native["authored_health_native"]["input"]["strength"]
    );
    let initial = integer(&native["authored_health_native"]["output"]["actual"]);

    // Match the native control's translated, already-admitted single reactor
    // at10,10, null Tag and empty animation slots. This is a receiver/power
    // comparison, not full campaign admission, difficulty or rendered parity.
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_grid(
        32,
        32,
        crate::map::resolved_terrain::test_clear_cell,
    ));
    sim.session
        .set_binary_frame_for_test(integer(&native["frame"]) as u32);
    let owner = sim.interner.intern(&authored.owner);
    sim.houses
        .register(HouseState::new(owner, 0, None, false, 0, 10));
    let type_id = sim.interner.intern(&authored.type_id);
    assert_eq!(sim.substrate.allocate_stable_id(), 1);
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        1,
        10,
        10,
        0,
        0,
        owner,
        Health::new(initial),
        type_id,
        EntityCategory::Structure,
        0,
        0,
        false,
    );
    entity.set_native_unique_id_for_test(integer(&native["reactor_native_unique_id"]));
    entity
        .type_facts
        .set_foundation_for_test(object.foundation.clone());
    entity.begin_building_body(BuildingBodyMode::Idle, 0);
    sim.substrate.entities.insert(entity);
    sim.add_entity_occupancy(1);
    crate::sim::world::Simulation::sample_building_health_for_house_update(
        &mut sim.substrate,
        &sim.houses,
        &mut sim.power_states,
        1,
    );
    crate::sim::power_system::assess_house_power(
        sim.power_states.get_mut(&owner).unwrap(),
        &sim.substrate.entities,
        &rules,
        owner,
        &sim.interner,
        sim.session.binary_frame(),
    );
    assert_eq!(
        json!(sim.power_states[&owner].total_output()),
        native["power_consumer"]["initial_power"]
    );
    sim.session.install_waypoints(&HashMap::from([(
        364,
        Waypoint {
            index: 364,
            rx: 10,
            ry: 10,
        },
    )]));
    let mut definitions = RuntimeDefinitions::default();
    let typ = definitions.trigger_type("DAMAGE").unwrap();
    let entry = map.actions["087857CC"]
        .entries()
        .iter()
        .find(|entry| entry.kind == ACTION_DAMAGE_AT_WAYPOINT)
        .unwrap()
        .clone();
    let action = definitions.resolve_action(entry, None, None);
    sim.trigger_runtime.definitions = definitions;
    // Import the original post-constructor boundary. Damage crossing yellow
    // must retain the shared smoke owner's one Scenario selection draw.
    sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(integer(
        &native["reactor_native_unique_id"],
    ) as u32));
    let rng_before = &native["rng_state_before"];
    sim.main_rng =
        SimRng::from_native_state_hex_for_test(rng_before["main"].as_str().unwrap()).into();
    sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(rng_before["scenario"].as_str().unwrap());
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(rng_before["mapgen"].as_str().unwrap());
    let (returned, visits) = crate::sim::combat::receiver_fixture::trace_area_visits(|| {
        sim.apply_live_trigger_action(
            &action,
            sim.trigger_action_context(typ, None, TagEventInput::default(), &rules),
            &mut Vec::new(),
            &rules,
            None,
            FrameEffects::default(),
        )
    });
    assert!(returned);
    let rows = native["rows"].as_array().unwrap();
    assert_eq!(visits.len(), rows.len());
    for (index, visit) in visits.iter().enumerate() {
        let prior = if index == 0 {
            initial
        } else {
            integer(&rows[index - 1]["health"])
        };
        assert_eq!(
            visit["health_before"],
            json!([[1, prior]]),
            "native receiver visit{}",
            index + 1
        );
    }
    let last = rows.last().unwrap();
    let reactor = sim.substrate.entities.get(1).unwrap();
    assert_eq!(json!(reactor.health.current()), last["health"]);
    assert_eq!(json!(reactor.estimated_health.get()), last["estimated"]);
    assert_eq!(
        u8::from(reactor.building_art.damage_state_active()),
        last["damaged_mode"].as_u64().unwrap() as u8
    );
    assert_eq!(
        json!(reactor.building_power_health_sample()),
        native["power_consumer"]["before_sample"]["sampled_health"]
    );
    assert_eq!(
        json!(sim.power_states[&owner].total_output()),
        native["power_consumer"]["before_sample"]["house_power"]
    );
    crate::sim::world::Simulation::sample_building_health_for_house_update(
        &mut sim.substrate,
        &sim.houses,
        &mut sim.power_states,
        1,
    );
    assert_eq!(
        json!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .building_power_health_sample()
        ),
        native["power_consumer"]["after_sample"]["sampled_health"]
    );
    assert!(sim.power_states[&owner].power_recheck_for_test());
    crate::sim::power_system::assess_house_power(
        sim.power_states.get_mut(&owner).unwrap(),
        &sim.substrate.entities,
        &rules,
        owner,
        &sim.interner,
        sim.session.binary_frame(),
    );
    assert_eq!(
        json!(sim.power_states[&owner].total_output()),
        native["power_consumer"]["after_power"]["house_power"]
    );
    assert!(!sim.power_states[&owner].power_recheck_for_test());
    let smoke = &last["damage_smoke"];
    assert_eq!(
        json!(sim.particle_systems().len()),
        smoke["registered_count"]
    );
    let (id, system) = sim.particle_systems().iter().next().unwrap();
    assert_eq!(
        system.type_id(),
        rules
            .ps_type_id_by_name(smoke["type_name"].as_str().unwrap())
            .unwrap()
    );
    assert_eq!(json!(system.coords().to_array()), smoke["coordinate"]);
    assert_eq!(json!(system.native_unique_id()), smoke["native_unique_id"]);
    assert_eq!(system.owner_entity(), Some(1));
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .damage_smoke_system_id
            .system(),
        Some(*id)
    );
    assert!(system.particles().is_empty(), "no particle AI visit yet");
    for (stream, actual) in [
        ("main", sim.main_rng.native_state_hex()),
        ("scenario", sim.scenario_rng.native_state_hex()),
        ("mapgen", sim.mapgen_rng.native_state_hex()),
    ] {
        assert_eq!(
            actual,
            native["rng_state_after"][stream].as_str().unwrap(),
            "{stream}"
        );
    }
}

/// Original Execute17→6E0FE0→5678E0→653830→4A9CA0 with the native
/// constructor's empty-resident, FogOfWar-clear cell priors. The production
/// action uses the retained+90 waypoint, current PlayerPtr and Fog+240 owner;
/// geometry, both counter passes, dummy writes and all three RNG states are
/// compared with original executable observations. Resident discovery and
/// GPU output are separately integrated/validated, not certified by this row.
#[test]
fn native_trigger_reveal_action_matches_original_controls() {
    use crate::map::cell_index::NativeCellIdentity;
    use crate::map::resolved_terrain::{NativeCellQuery, test_clear_cell, test_grid};
    use crate::map::waypoints::Waypoint;
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::rng::SimRng;
    use crate::sim::vision::{FogState, OwnerVisibility};
    use std::collections::BTreeSet;

    let corpus: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_action_values.json",
    ))
    .unwrap();
    let native = corpus
        .iter()
        .find(|row| row["case_kind"] == "trigger_reveal")
        .unwrap();
    let controls = native["controls"].as_array().unwrap();
    assert_eq!(controls.len(), 16);
    for row in controls {
        let name = row["name"].as_str().unwrap();
        let input = &row["input"];
        let source = (
            integer(&input["cell"][0]) as i16,
            integer(&input["cell"][1]) as i16,
        );
        let level = input["level"].as_i64().unwrap_or(0) as i8;
        let radius = input["radius"].as_i64().unwrap_or(9) as i32;
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\nRevealTriggerRadius={radius}\nRevealByHeight=yes\n"
        )))
        .unwrap();
        let mut terrain = test_grid(128, 128, test_clear_cell);
        let identity = if input["missing"] == true {
            NativeCellIdentity::Dummy
        } else {
            terrain.native_cell_identity(source)
        };
        terrain.write_native_cell_level(identity, level as u8);
        terrain.write_native_cell_flags(identity, input["flags"].as_u64().unwrap_or(0) as u32);
        let mut sim = Simulation::with_seed(31);
        let path = crate::sim::pathfinding::PathGrid::from_resolved_terrain(&terrain);
        sim.install_resolved_terrain_for_new_map(terrain);
        sim.install_fixture_path_grid(Some(&path));
        sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
            theater: "TEMPERATE".into(),
            fill: "Clear".into(),
            level: 0,
            width: integer(&row["size"][0]) as u32,
            height: integer(&row["size"][1]) as u32,
            local_left: 0,
            local_top: 0,
            local_width: integer(&row["size"][0]) as u32,
            local_height: integer(&row["size"][1]) as u32,
        });
        let player = sim.interner.intern("Player");
        let other = sim.interner.intern("Other");
        sim.session.set_current_house_for_test(Some(player));
        sim.fog.set_width_for_test(128);
        sim.fog.set_height_for_test(128);
        for (owner, latch) in [(player, &input["latch"]), (other, &input["other_latch"])] {
            if latch.as_u64().unwrap_or(0) != 0 {
                sim.fog.reveal_all_for_owner(owner);
            }
            // Native fixture supplies House240 independently of the Cell
            // constructor's untouched planes. Seed them through their owner.
            sim.fog
                .by_owner_mut_for_test()
                .insert(owner, OwnerVisibility::new(128, 128));
        }
        let mut definitions = RuntimeDefinitions::default();
        let typ = definitions.trigger_type("REVEAL").unwrap();
        definitions.trigger_types[typ].owner = Some(
            if input["trigger_house"] == "player" {
                "Player"
            } else {
                "Other"
            }
            .into(),
        );
        let ini = IniFile::from_str(&format!(
            "[Actions]\nREVEAL={}\n",
            row["read"]["read"].as_str().unwrap()
        ));
        let entry = crate::map::actions::parse_actions(&ini)
            .remove("REVEAL")
            .unwrap()
            .entries()[0]
            .clone();
        let action = definitions.resolve_action(entry, None, None);
        assert_eq!(
            json!(action.value),
            row["read"]["value"],
            "{name}: retained+90"
        );
        assert_eq!(
            json!(action.entry.waypoint_index),
            row["read"]["waypoint"],
            "{name}: independent+44"
        );
        let mut waypoints = HashMap::new();
        let literal = integer(&row["read"]["value"]) as u32;
        if let Some(tail) = action.entry.waypoint_index {
            waypoints.insert(
                tail,
                Waypoint {
                    index: tail,
                    rx: 48,
                    ry: 76,
                },
            );
        }
        waypoints.insert(
            literal,
            Waypoint {
                index: literal,
                rx: source.0 as u16,
                ry: source.1 as u16,
            },
        );
        sim.session.install_waypoints(&waypoints);
        sim.trigger_runtime.definitions = definitions;
        sim.scenario_rng = SimRng::new(31);
        sim.main_rng = SimRng::new(31).into();
        sim.mapgen_rng = SimRng::new(31);
        let hashes = |sim: &Simulation| {
            json!({
                "scenario": native_rng_hash(&sim.scenario_rng.native_state_hex()),
                "main": native_rng_hash(&sim.main_rng.native_state_hex()),
                "mapgen": native_rng_hash(&sim.mapgen_rng.native_state_hex()),
            })
        };
        assert_eq!(
            hashes(&sim),
            row["rng_before"],
            "{name}: original seeded priors"
        );
        let frame = sim.session.binary_frame();
        let mut effects = Vec::new();
        let (returned, draws) = crate::sim::rng::trace_draws(|| {
            sim.apply_live_trigger_action(
                &action,
                sim.trigger_action_context(typ, None, TagEventInput::default(), &rules),
                &mut effects,
                &rules,
                None,
                FrameEffects::default(),
            )
        });
        assert_eq!(
            i32::from(returned),
            integer(&row["returned_al"]),
            "{name}: native AL"
        );
        assert!(
            draws.is_empty() && effects.is_empty(),
            "{name}: no RNG/presentation/timer events"
        );
        assert_eq!(
            hashes(&sim),
            row["rng_after"],
            "{name}: all retained RNG words"
        );
        assert_eq!(
            sim.session.binary_frame(),
            frame,
            "{name}: no frame advancement"
        );
        assert_eq!(
            sim.fog.whole_map_revealed_for_owner(player),
            row["latch_after"].as_u64().unwrap() != 0
        );
        let expected: BTreeSet<_> = row["final_cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|cell| {
                (
                    integer(&cell["cell"][0]) as u16,
                    integer(&cell["cell"][1]) as u16,
                )
            })
            .collect();
        let revealed = |fog: &FogState| {
            (0..128u16)
                .flat_map(|y| (0..128u16).map(move |x| (x, y)))
                .filter(|&(x, y)| fog.is_ground_unshrouded(player, x, y))
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(
            revealed(&sim.fog),
            expected,
            "{name}: native whole footprint, no extra cells"
        );
        assert!(
            (0..128u16).all(|y| (0..128u16).all(|x| !sim.fog.is_ground_unshrouded(other, x, y))),
            "{name}: trigger House is not PlayerPtr"
        );
        let compare = |fog: &FogState, cells: &Value| {
            for cell in cells.as_array().unwrap() {
                let x = integer(&cell["cell"][0]) as u16;
                let y = integer(&cell["cell"][1]) as u16;
                let vis = &fog.by_owner()[&player];
                let state = vis.shroud_knowledge_raw()[usize::from(y) * 128 + usize::from(x)];
                assert_eq!(
                    state.counter(),
                    integer(&cell["counter"]),
                    "{name}: ({x},{y}) counter"
                );
                assert_eq!(
                    state.gap_counter(),
                    integer(&cell["gap"]),
                    "{name}: ({x},{y}) gap"
                );
                assert_eq!(
                    state.pending(),
                    cell["flags"].as_u64().unwrap() & 0x20 != 0,
                    "{name}: pending"
                );
                assert_eq!(
                    fog.is_ground_unshrouded(player, x, y),
                    cell["alt"].as_u64().unwrap() & 8 != 0
                );
                assert_eq!(
                    fog.is_ground_open(player, x, y),
                    cell["alt"].as_u64().unwrap() & 16 != 0
                );
                assert_eq!(
                    state.local_sources() + state.allied_sources(),
                    0,
                    "{name}: no permanent source"
                );
            }
        };
        compare(&sim.fog, &row["final_cells"]);
        let config = sim.sight_reveal_config(&rules);
        let heights = sim.sight_height_grid(&config);
        let mut probe = Simulation::with_seed(31);
        probe.fog = FogState::new(128, 128);
        probe.session.set_current_house_for_test(Some(player));
        probe.install_resolved_terrain_for_new_map(sim.resolved_terrain.as_ref().unwrap().clone());
        probe
            .fog
            .by_owner_mut_for_test()
            .insert(player, OwnerVisibility::new(128, 128));
        for pass in row["passes"].as_array().unwrap() {
            let coord = &pass["coord"];
            assert_eq!(pass["house"], "player");
            assert_eq!(pass["by_height"], 1);
            assert_eq!(
                [&pass["outline"], &pass["unreveal"], &pass["arg6"]],
                [&json!(0), &json!(0), &json!(0)]
            );
            let _ = probe.reveal_area(
                &[player],
                (
                    (integer(&coord[0]) / 256) as u16,
                    (integer(&coord[1]) / 256) as u16,
                ),
                integer(&coord[2]),
                radius.clamp(0, i32::from(u16::MAX)) as u16,
                Some(&heights),
                integer(&pass["final"]) != 0,
                &config,
                crate::sim::world::UninitContext::with_rules(&rules),
                crate::sim::vision::MapRevealProjection::Transient,
                probe.session.current_house(),
            );
            compare(&probe.fog, &pass["states"]);
        }
        if input["missing"] == true {
            if let Some(last) = row["calls"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find(|call| call["call"] == "packed_lookup")
            {
                let dummy = sim.effective_shared_cell_dummy().snapshot();
                assert_eq!(
                    json!([dummy.coord.0, dummy.coord.1]),
                    last["cell"],
                    "{name}: shared Dummy ordering"
                );
                assert_eq!(dummy.level, level);
            }
        }
        for vis in sim.fog.by_owner_mut_for_test().values_mut() {
            vis.clear_all_visible();
        }
        assert_eq!(
            revealed(&sim.fog),
            expected,
            "{name}: knowledge survives sight cleanup"
        );
        let restored: FogState =
            save_codec::deserialize(&save_codec::serialize(&sim.fog).unwrap()).unwrap();
        assert_eq!(
            revealed(&restored),
            expected,
            "{name}: knowledge survives save/load"
        );
        compare(&restored, &row["final_cells"]);
        let query = NativeCellQuery::isolated(sim.resolved_terrain.as_ref().unwrap());
        for cell in row["final_cells"].as_array().unwrap() {
            assert_eq!(
                crate::sim::vision::cell_is_shrouded(
                    &restored,
                    &query,
                    player,
                    (
                        integer(&cell["cell"][0]) as u16,
                        integer(&cell["cell"][1]) as u16
                    )
                ),
                cell["shrouded_query_al"] == true,
                "{name}: original IsShrouded consumer"
            );
        }
    }
}

fn rows() -> Vec<Value> {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/tag_lifecycle.json",
    ))
    .unwrap()
}

#[test]
fn native_trigger_reveal_retail_definitions_use_retained_radius_and_literal_waypoints() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    let Some(raw_rules) = crate::rules::retail_ini_fixture::retail_ini_bytes("rulesmd.ini") else {
        return;
    };
    let Some(raw_art) = crate::rules::retail_ini_fixture::retail_ini_bytes("artmd.ini") else {
        return;
    };
    let Some(raw_map) = crate::rules::retail_ini_fixture::retail_archive_bytes("SOV01UMD.MAP")
    else {
        return;
    };
    let map = crate::map::map_file::MapFile::from_bytes(&raw_map).unwrap();
    let corpus: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_action_values.json",
    ))
    .unwrap();
    let native = corpus
        .iter()
        .find(|row| row["case_kind"] == "trigger_reveal")
        .unwrap();
    let definitions = &native["retail_definitions"];
    for (name, raw) in [("rulesmd.ini", &raw_rules), ("sov01umd.map", &raw_map)] {
        assert_eq!(
            crate::util::sha256::sha256_hex(raw),
            definitions["sources"][name]["sha256"]
        );
        assert_eq!(json!(raw.len()), definitions["sources"][name]["bytes"]);
    }
    let mut layers = RulesLayerStack::new(IniFile::from_bytes(&raw_rules).unwrap());
    let art = IniFile::from_bytes(&raw_art).unwrap();
    let rule_radius = |layers: &RulesLayerStack| {
        crate::rules::ruleset::RuleSet::from_processed_rules(
            &layers.process_with_fixed_art(&art).unwrap(),
        )
        .unwrap()
        .reveal_trigger_radius()
    };
    assert_eq!(
        json!(rule_radius(&layers)),
        native["radius_reader"]["rows"][0]["value"]
    );
    layers.push(RulesLayerKind::Scenario, map.ini.clone());
    assert_eq!(
        json!(rule_radius(&layers)),
        native["radius_reader"]["rows"][1]["value"]
    );
    let triggers = definitions["triggers"].as_array().unwrap();
    assert_eq!(triggers.len(), 4);
    let mut count = 0;
    for trigger in triggers {
        let id = trigger["id"].as_str().unwrap();
        let authored = &map.actions[id];
        assert_eq!(
            authored.entries().len(),
            trigger["actions"].as_array().unwrap().len(),
            "{id}"
        );
        for (entry, expected) in authored
            .entries()
            .iter()
            .zip(trigger["actions"].as_array().unwrap())
        {
            assert_eq!(json!(entry.kind), expected["kind"], "{id}");
            if entry.kind == 17 {
                assert_eq!(json!(entry.literal_value()), expected["value"], "{id}: +90");
                assert_eq!(
                    json!(entry.waypoint_index),
                    expected["waypoint_44"],
                    "{id}: +44"
                );
                count += 1;
            }
        }
    }
    assert_eq!(json!(count), definitions["action17_count"]);
    assert_eq!(
        json!(
            map.actions
                .values()
                .flat_map(|actions| actions.entries())
                .filter(|entry| entry.kind == 16)
                .count()
        ),
        definitions["action16_count_all_definitions"]
    );
    for (index, expected) in definitions["waypoints"].as_object().unwrap() {
        let waypoint = &map.waypoints[&index.parse::<u32>().unwrap()];
        assert_eq!(
            json!([waypoint.rx, waypoint.ry]),
            *expected,
            "native waypoint{index}"
        );
    }
}

#[test]
fn action_11_inline_labels_and_ordered_effects_match_original_controls() {
    let corpus: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_action_values.json",
    ))
    .unwrap();
    let message = corpus
        .iter()
        .find(|row| row["case_kind"] == "localized_message")
        .unwrap();
    let mut compared = 0;
    for control in message["label_controls"].as_array().unwrap() {
        let entry = if let Some(raw) = control["read"]["read"].as_str() {
            let ini =
                crate::rules::ini_parser::IniFile::from_str(&format!("[Actions]\nLABEL={raw}\n"));
            crate::map::actions::parse_actions(&ini)
                .remove("LABEL")
                .unwrap()
                .entries()[0]
                .clone()
        } else {
            // Original constructor-only control retains an empty inline label.
            ActionEntry {
                kind: 11,
                params: Vec::new(),
                waypoint_index: Some(0),
            }
        };
        assert_eq!(entry.csf_label(), control["label"].as_str().unwrap());
        let mut sim = Simulation::new();
        let mut definitions = RuntimeDefinitions::default();
        let typ = definitions.trigger_type("LABEL").unwrap();
        let action = definitions.resolve_action(entry, None, None);
        sim.trigger_runtime.definitions = definitions;
        let preceding = TriggerEffect::InputLockChanged {
            locked: true,
            was_locked: false,
        };
        let mut effects = vec![preceding.clone()];
        let result = sim.apply_live_trigger_action(
            &action,
            sim.trigger_action_context(
                typ,
                None,
                TagEventInput::default(),
                crate::rules::ruleset::RuleSet::empty_for_test(),
            ),
            &mut effects,
            crate::rules::ruleset::RuleSet::empty_for_test(),
            None,
            FrameEffects::default(),
        );
        assert_eq!(json!(i32::from(result)), control["returned"]);
        let mut expected = vec![preceding];
        let label = control["label"].as_str().unwrap();
        if !label.is_empty() {
            expected.push(TriggerEffect::LocalizedMessage {
                csf_label: label.to_string(),
            });
        }
        assert_eq!(
            effects, expected,
            "{:?}: native posting admission and action order",
            control["input"]
        );
        compared += 1;
    }
    assert_eq!(compared, 10);
}

#[test]
fn restored_live_owner_rejects_dangling_definitions_instances_and_attachments() {
    let corpus = rows();
    let input = &corpus
        .iter()
        .find(|row| row["input"]["name"] == "reverse_chain")
        .unwrap()["input"];
    let mut sim = Simulation::with_seed(31);
    sim.trigger_runtime.definitions = supplied_definitions(input);
    sim.trigger_runtime.definitions_prepared = true;
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(
        64,
        64,
        crate::map::resolved_terrain::test_clear_cell,
    ));
    let house = supplied_house(&mut sim, input);
    let tag = sim.shared_trigger_tag(0);
    let object = sim.substrate.allocate_stable_id();
    let type_id = sim.interner.intern("E1");
    let mut entity = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
        object,
        0,
        0,
        0,
        0,
        house,
        crate::sim::combat::Health::new(100),
        type_id,
        crate::map::entities::EntityCategory::Unit,
        0,
        5,
        false,
    );
    entity.lifecycle.set_in_limbo_for_test(true);
    sim.substrate.entities.insert(entity);
    sim.trigger_runtime
        .replace_trigger_tag_attachment(TagAttachment::Object(object), Some(tag));
    let cell =
        crate::sim::world::Simulation::trigger_cell_identity(&sim.resolved_terrain, (50, 60));
    sim.trigger_runtime
        .replace_cell_trigger_tag(cell, Some(tag));
    sim.trigger_runtime.logic_tags.push(tag);
    sim.validate_restored_trigger_runtime().unwrap();
    let owner = sim.trigger_runtime.clone();
    let corruptions: [(&str, fn(&mut Simulation)); 9] = [
        ("definition link", |sim| {
            sim.trigger_runtime.definitions.trigger_types[0].linked = Some(usize::MAX)
        }),
        ("team reference count", |sim| {
            sim.trigger_runtime.definitions.team_tags.push(None)
        }),
        ("team retained tag", |sim| {
            let index = sim.trigger_runtime.definitions.team_type("TT").unwrap();
            sim.trigger_runtime.definitions.team_tags[index] = Some(usize::MAX);
        }),
        ("live type", |sim| {
            sim.trigger_runtime.tags[0].as_mut().unwrap().type_index = usize::MAX
        }),
        ("live link", |sim| {
            sim.trigger_runtime.tags[0].as_mut().unwrap().triggers[0] = usize::MAX
        }),
        ("registry duplicate", |sim| {
            sim.trigger_runtime.tag_order.push(0)
        }),
        ("object attachment", |sim| {
            sim.trigger_runtime
                .attachments
                .insert(TagAttachment::Object(u64::MAX), 0);
        }),
        ("team attachment", |sim| {
            sim.trigger_runtime
                .attachments
                .insert(TagAttachment::Team(u64::MAX), 0);
        }),
        ("cell attachment", |sim| {
            sim.trigger_runtime
                .cell_tags
                .insert(NativeCellIdentity::Real(usize::MAX), 0);
        }),
    ];
    for (name, corrupt) in corruptions {
        sim.trigger_runtime = owner.clone();
        corrupt(&mut sim);
        assert!(sim.validate_restored_trigger_runtime().is_err(), "{name}");
    }
    sim.trigger_runtime = owner;
    crate::sim::world::Simulation::mark_live_tag_for_delete(
        &mut sim.substrate,
        &mut sim.trigger_runtime,
        tag,
    );
    sim.validate_restored_trigger_runtime().unwrap();
    // The Tag deleting destructor leaves its Triggers in the common queue.
    // This valid saved intermediate state must not reconstruct or reject them.
    sim.substrate
        .pending_delete
        .remove_ticket_for_test(crate::sim::world::DeferredDeleteTicket::Tag(tag));
    sim.delete_live_tag(
        tag,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        0,
        false,
    );
    sim.validate_restored_trigger_runtime().unwrap();
}

#[test]
fn physical_team_tag_references_match_original_retention_and_long_id_controls() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::team_ai_ini::TeamAiIniRegistry;

    let corpus = rows();
    let reference = corpus
        .iter()
        .find(|row| row["input"]["reference_controls"] == true)
        .unwrap();
    let controls = reference["team_tag_controls"].as_array().unwrap();
    assert_eq!(controls.len(), 12);
    let team_source = |value: &Value| {
        format!(
            // The native field controls supply a constructed TT directly.
            // Keep its section admitted by the full physical reader even
            // when Tag is absent/empty; entryless sections are discarded by
            // LoadFromStraw525A60 and are outside that isolated field prior.
            "[TeamTypes]\n0=TT\n[TT]\nName=TT\n{}",
            value
                .as_str()
                .map_or(String::new(), |value| format!("Tag={value}\n"))
        )
    };
    const MAP_PREFIX: &str = "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\n\
        LocalSize=0,0,2,1\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n";
    for control in controls {
        let fixed = IniFile::from_str(&team_source(&control["values"][0]));
        for (step, expected) in control["history"].as_array().unwrap().iter().enumerate() {
            let scenario = if step == 0 {
                MAP_PREFIX.to_string()
            } else {
                format!("{MAP_PREFIX}{}", team_source(&control["values"][1]))
            };
            let map = crate::map::map_file::MapFile::from_bytes(scenario.as_bytes()).unwrap();
            let teams = TeamAiIniRegistry::from_sources(&fixed, &map.ini, false);
            let definitions = RuntimeDefinitions::read_map(&map, &teams, None, None);
            assert_eq!(definitions.team_types, ["TT"]);
            assert_eq!(
                json!(
                    definitions
                        .tag_types
                        .iter()
                        .map(|typ| typ.id.as_str())
                        .collect::<Vec<_>>()
                ),
                expected["after"],
                "{} read {step}: factory order",
                control["name"]
            );
            let retained = definitions.team_tags[0];
            let actual = retained.map_or(
                Value::Null,
                |index| json!({"index":index,"id":definitions.tag_types[index].id}),
            );
            assert_eq!(
                actual, expected["result"],
                "{} read {step}",
                control["name"]
            );

            // Native Team6E8D76 consumes its stored Type+D0 pointer. The
            // constructor must allocate a fresh live Tag from that ordinal,
            // without resolving the raw name again or consuming an RNG draw.
            let mut sim = Simulation::new();
            sim.trigger_runtime.definitions = definitions;
            let typ = sim.interner.intern("TT");
            let before = sim.trigger_runtime.definitions.clone();
            let (_, draws) = crate::sim::rng::trace_draws(|| sim.attach_team_trigger_tag(7, typ));
            assert!(draws.is_empty());
            assert_eq!(sim.trigger_runtime.definitions, before);
            let attachment = sim.trigger_runtime.attachments.get(&TagAttachment::Team(7));
            assert_eq!(attachment.is_some(), retained.is_some());
            if let Some(&tag) = attachment {
                assert_eq!(
                    sim.trigger_runtime.tag(tag).unwrap().type_index,
                    retained.unwrap()
                );
                assert_eq!(sim.trigger_runtime.tag(tag).unwrap().refs, 0);
            }
            let bytes = save_codec::serialize(&sim.trigger_runtime).unwrap();
            sim.trigger_runtime = save_codec::deserialize(&bytes).unwrap();
            sim.attach_team_trigger_tag(8, typ);
            assert_eq!(sim.trigger_runtime.definitions, before);
            if let Some(retained) = retained {
                let first = sim.trigger_runtime.attachments[&TagAttachment::Team(7)];
                let second = sim.trigger_runtime.attachments[&TagAttachment::Team(8)];
                assert_ne!(first, second);
                assert_eq!(
                    sim.trigger_runtime.tag(second).unwrap().type_index,
                    retained
                );
            }
        }
        assert_eq!(control["rng_before"], control["rng_after"]);
    }
}

#[test]
fn team_member_tag_admission_and_removal_match_original_controls() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    use crate::sim::house_state::HouseState;
    use crate::sim::rng::SimRng;
    use crate::sim::team_script_vm::TeamScriptVm;

    let corpus: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/team_creation.json",
    ))
    .unwrap();
    let controls: Vec<_> = corpus
        .iter()
        .filter(|row| row["case_kind"] == "member_tag")
        .collect();
    assert_eq!(controls.len(), 8);
    let Some((retail, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let mut actual = Vec::new();
    let mut expected = Vec::new();
    for control in controls {
        let input = &control["input"];
        let mut layers = RulesLayerStack::new(retail.clone());
        // The original matrix explicitly supplies Type+5E0=0/1 on a genuine
        // E1. This is an admission control, not a retail CARRIER cargo proof.
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(&format!(
                "[E1]\nPassengers={}\n",
                integer(&input["passengers"])
            )),
        );
        let processed = layers.process_with_fixed_art(&art).unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.install_art_data(crate::rules::retail_ini_fixture::art_registry(&art));
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        sim.houses
            .register(HouseState::new(owner, 0, Some(owner), false, 0, 10));
        let definitions = IniFile::from_str(&format!(
            "[TeamTypes]\n0=TT\n[TT]\nHouse=Americans\nScript=S\nTaskForce=F\nOnTransOnly={}\n\
             [ScriptTypes]\n0=S\n[S]\n0=5,0\n[TaskForces]\n0=F\n[F]\n0=1,E1\n",
            if input["on_trans_only"].as_bool().unwrap() {
                "yes"
            } else {
                "no"
            }
        ));
        let registry = TeamAiIniRegistry::from_sources(&definitions, &IniFile::from_str(""), false);
        sim.install_team_ai_registry(&registry, &rules).unwrap();
        let author_type = sim.trigger_runtime.definitions.tag_type("author").unwrap();
        let team_type = sim.trigger_runtime.definitions.tag_type("team").unwrap();
        sim.trigger_runtime.definitions.team_types = vec!["TT".into()];
        sim.trigger_runtime.definitions.team_tags = vec![
            input["nonnull_team_tag"]
                .as_bool()
                .unwrap()
                .then_some(team_type),
        ];
        let typ = sim.interner.get("TT").unwrap();
        let team = sim.create_team(typ, Some(owner), &rules).unwrap();
        let member = sim
            .with_object_placement_scope(|sim| {
                sim.construct_object_limbo_at_height(
                    "E1",
                    "Americans",
                    0,
                    0,
                    0,
                    0,
                    &rules,
                    crate::sim::world::FrameEffects::default(),
                )
            })
            .unwrap();
        let author_tag = sim.construct_trigger_tag(author_type);
        // Supplied anchor attachments retain refs2/1 throughout the original
        // control. Zero-ref destruction and deferred drain are excluded here.
        sim.trigger_runtime
            .replace_trigger_tag_attachment(TagAttachment::Object(u64::MAX), Some(author_tag));
        sim.trigger_runtime
            .replace_trigger_tag_attachment(TagAttachment::Object(member), Some(author_tag));
        let team_tag = sim
            .trigger_runtime
            .attachments
            .get(&TagAttachment::Team(team))
            .copied();
        if let Some(tag) = team_tag {
            sim.trigger_runtime
                .replace_trigger_tag_attachment(TagAttachment::Object(u64::MAX - 1), Some(tag));
        }
        let state = |sim: &Simulation| {
            let attached = sim
                .trigger_runtime
                .attachments
                .get(&TagAttachment::Object(member))
                .copied();
            json!({
                "object_tag": attached.map(|tag| if tag == author_tag { "author" } else if Some(tag) == team_tag { "team" } else { "other" }),
                "author_refs": sim.trigger_runtime.tag(author_tag).unwrap().refs,
                "team_refs": team_tag.map(|tag| sim.trigger_runtime.tag(tag).unwrap().refs),
                "has_team": sim.team_script_vm.team_for_member(member).is_some(),
                "team_count": sim.team_script_vm.team(team).unwrap().taskforce_counts_for_test()[0],
            })
        };
        let native_state = |value: &Value| {
            json!({
                "object_tag": value["object_tag"], "author_refs": value["author_refs"],
                "team_refs": value["team_refs"], "has_team": value["foot_team"] != "0x0",
                "team_count": value["team_count"],
            })
        };
        let prior = state(&sim);
        // Import the native mutation boundary after genuine constructors;
        // concrete constructor RNG is separately covered by reinforcement rows.
        let rng_before = &control["member_rng_before"];
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(rng_before["scenario"].as_str().unwrap());
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(rng_before["main"].as_str().unwrap()).into();
        sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(rng_before["mapgen"].as_str().unwrap());
        let (added, add_draws) = crate::sim::rng::trace_draws(|| {
            sim.with_object_placement_scope(|sim| {
                sim.team_add_member(team, member, false, &rules, None, FrameEffects::default())
            })
        });
        assert_eq!(i32::from(added), integer(&control["added_al"]), "{input}");
        let after_add = state(&sim);
        sim.trigger_runtime =
            save_codec::deserialize(&save_codec::serialize(&sim.trigger_runtime).unwrap()).unwrap();
        sim.team_script_vm = save_codec::deserialize::<TeamScriptVm>(
            &save_codec::serialize(&sim.team_script_vm).unwrap(),
        )
        .unwrap();
        assert_eq!(
            state(&sim),
            after_add,
            "{input}: retained attachments/membership through save"
        );
        let (_, remove_draws) = crate::sim::rng::trace_draws(|| {
            sim.team_remove_member(team, member, false, &rules, FrameEffects::default());
        });
        assert!(add_draws.is_empty() && remove_draws.is_empty(), "{input}");
        let rng_after = &control["member_rng_after"];
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            rng_after["scenario"].as_str().unwrap(),
            "{input}"
        );
        assert_eq!(
            sim.main_rng.native_state_hex(),
            rng_after["main"].as_str().unwrap(),
            "{input}"
        );
        assert_eq!(
            sim.mapgen_rng.native_state_hex(),
            rng_after["mapgen"].as_str().unwrap(),
            "{input}"
        );
        actual.push(json!({"name":input["name"], "prior":prior, "after_add":after_add, "after_remove":state(&sim)}));
        expected.push(json!({"name":input["name"], "prior":native_state(&control["prior"]), "after_add":native_state(&control["after_add"]), "after_remove":native_state(&control["after_remove"])}));
        assert!(
            sim.substrate.pending_delete.is_empty(),
            "{input}: anchors prevent destructor admission"
        );
    }
    assert_eq!(
        actual, expected,
        "original AddMember6EA500 / RemoveMember6EA870 Tag histories"
    );
}

#[test]
fn definition_factories_match_all_original_action_reference_controls() {
    let corpus = rows();
    let controls = corpus
        .iter()
        .find(|row| row["input"]["reference_controls"] == true)
        .unwrap();
    // The original controls are a continuous reader history: parameter types
    // 1 and5 share the TeamType array, including two distinct identities whose
    // stored24-byte IDs are equal. Replaying each truncated `before` name
    // through FindOrAllocate would collapse that supplied native prior.
    let mut definitions = RuntimeDefinitions::default();
    for name in controls["initial_ids"].as_array().unwrap() {
        let name = name.as_str().unwrap();
        definitions.team_type(name);
        definitions.trigger_type(name);
        definitions.tag_type(name);
    }
    let mut compared = 0;
    for row in controls["rows"].as_array().unwrap() {
        let parameter_type = integer(&row["parameter_type"]);
        let before: Vec<_> = match parameter_type {
            1 | 5 => definitions.team_types.iter().map(String::as_str).collect(),
            2 => definitions
                .trigger_types
                .iter()
                .map(|typ| typ.id.as_str())
                .collect(),
            3 => definitions
                .tag_types
                .iter()
                .map(|typ| typ.id.as_str())
                .collect(),
            _ => unreachable!(),
        };
        assert_eq!(
            json!(before),
            row["before"],
            "{}: original retained prior",
            row["raw"]
        );
        let raw = row["raw"].as_str().unwrap();
        let ini =
            crate::rules::ini_parser::IniFile::from_str(&format!("[Actions]\nREFERENCE={raw}\n"));
        let record = crate::map::actions::parse_actions(&ini)
            .remove("REFERENCE")
            .unwrap();
        let resolved = definitions.resolve_action(record.entries()[0].clone(), None, None);
        let names: Vec<_> = match parameter_type {
            1 | 5 => definitions.team_types.iter().map(String::as_str).collect(),
            2 => definitions
                .trigger_types
                .iter()
                .map(|typ| typ.id.as_str())
                .collect(),
            3 => definitions
                .tag_types
                .iter()
                .map(|typ| typ.id.as_str())
                .collect(),
            _ => unreachable!(),
        };
        assert_eq!(json!(names), row["after"], "{raw}: retained type order");
        let result = match resolved.reference {
            None => Value::Null,
            Some(DefinitionReference::Team(index)) => json!({
                "index": index, "id": definitions.team_types[index],
                "name": definitions.team_types[index],
            }),
            Some(DefinitionReference::Trigger(index)) => {
                let typ = &definitions.trigger_types[index];
                json!({ "index": index, "id": typ.id, "name": typ.id,
                    "flags": [i32::from(typ.difficulty[0]), i32::from(typ.difficulty[1]),
                        i32::from(typ.difficulty[2]), i32::from(typ.enabled), 0],
                    "linked": typ.linked, "event_count": typ.events.len(),
                    "action_count": typ.actions.len() })
            }
            Some(DefinitionReference::Tag(index)) => {
                let typ = &definitions.tag_types[index];
                json!({ "index": index, "id": typ.id, "name": typ.name,
                    "repeat": typ.repeat, "trigger": typ.trigger })
            }
        };
        let mut expected = row["result"].clone();
        if let Some(object) = expected.as_object_mut() {
            object.remove("id_hex");
        }
        assert_eq!(result, expected, "{raw}: original factory return/defaults");
        compared += 1;
    }
    assert_eq!(compared, 44);
}

fn stock_state(sim: &Simulation, draws: usize) -> Value {
    let runtime = &sim.trigger_runtime;
    let tag_name = |id: usize| {
        runtime.definitions.tag_types[runtime.tag(id).unwrap().type_index]
            .id
            .clone()
    };
    let live_tags: Vec<_> = runtime
        .tag_order
        .iter()
        .map(|&id| {
            let tag = runtime.tag(id).unwrap();
            let triggers: Vec<_> = tag
                .triggers
                .iter()
                .map(|&id| {
                    let live = runtime.trigger(id).unwrap();
                    json!({ "type": runtime.definitions.trigger_types[live.type_index].id,
                "enabled": i32::from(live.enabled), "pending": i32::from(live.pending),
                "start": live.timer.start_frame(), "duration": live.timer.duration(),
                "completed": live.completed })
                })
                .collect();
            json!({ "type": tag_name(id), "refs": tag.refs, "pending": i32::from(tag.pending),
            "guard": i32::from(tag.dispatch_guard), "triggers": triggers })
        })
        .collect();
    let houses: serde_json::Map<_, _> = sim
        .houses
        .order()
        .iter()
        .map(|house| {
            let tags: Vec<_> = runtime
                .house_tags
                .get(house)
                .into_iter()
                .flatten()
                .map(|&tag| tag_name(tag))
                .collect();
            (sim.interner.resolve(*house).to_string(), json!(tags))
        })
        .collect();
    json!({ "frame": sim.session.binary_frame(), "live_tags": live_tags,
        "logic_tags": runtime.logic_tags.iter().map(|&tag| tag_name(tag)).collect::<Vec<_>>(),
        "map_tags": runtime.map_tags.iter().map(|&tag| tag_name(tag)).collect::<Vec<_>>(),
        "house_tags": houses, "rng_draws": draws, "logic_cursor": runtime.logic_cursor,
        "dispatches": runtime.dispatch_trace.iter().map(|&(tag, kind)| (tag_name(tag), kind)).collect::<Vec<_>>() })
}

fn compare_stock_state(sim: &Simulation, draws: usize, expected: &Value) {
    let actual = stock_state(sim, draws);
    for (key, value) in actual.as_object().unwrap() {
        assert_eq!(value, &expected[key], "{}: {key}", expected["phase"]);
    }
}

#[test]
fn physical_retail_sov01_registration_and_enable_timers_match_original_at_all_difficulties() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    let Some(raw_ai) = crate::rules::retail_ini_fixture::retail_ini_bytes("aimd.ini") else {
        return;
    };
    let Some(raw_eva) = crate::rules::retail_ini_fixture::retail_ini_bytes("evamd.ini") else {
        return;
    };
    let Some(raw_rules) = crate::rules::retail_ini_fixture::retail_ini_bytes("rulesmd.ini") else {
        return;
    };
    let Some(raw_art) = crate::rules::retail_ini_fixture::retail_ini_bytes("artmd.ini") else {
        return;
    };
    let Some(raw_map) = crate::rules::retail_ini_fixture::retail_archive_bytes("SOV01UMD.MAP")
    else {
        return;
    };
    let map = crate::map::map_file::MapFile::from_bytes(&raw_map).unwrap();
    let teams = crate::rules::team_ai_ini::TeamAiIniRegistry::from_sources(
        &IniFile::from_bytes(&raw_ai).unwrap(),
        &map.ini,
        false,
    );
    let eva =
        crate::rules::sound_ini::EvaRegistry::from_ini(&IniFile::from_bytes(&raw_eva).unwrap());
    let mut layers = RulesLayerStack::new(IniFile::from_bytes(&raw_rules).unwrap());
    layers.push(RulesLayerKind::Scenario, map.ini.clone());
    let processed = layers
        .process_with_fixed_art(&IniFile::from_bytes(&raw_art).unwrap())
        .unwrap();
    let rules = crate::rules::ruleset::RuleSet::from_processed_rules(&processed).unwrap();
    let roster = crate::map::houses::parse_house_roster(&map.ini, &[], Some(&rules));
    let mut compared = 0;
    for row in rows()
        .into_iter()
        .filter(|row| row["input"]["stock_prefix"] == true)
    {
        for (name, raw) in [
            ("sov01umd.map", &raw_map),
            ("aimd.ini", &raw_ai),
            ("evamd.ini", &raw_eva),
            ("rulesmd.ini", &raw_rules),
        ] {
            assert_eq!(json!(raw.len()), row["sources"][name]["bytes"]);
            assert_eq!(
                crate::util::sha256::sha256_hex(raw),
                row["sources"][name]["sha256"]
            );
        }
        let mut sim = Simulation::with_seed(31);
        sim.scenario_rng = crate::sim::rng::SimRng::from_native_state_hex_for_test(
            row["rng_before"]["scenario"].as_str().unwrap(),
        );
        sim.session.set_binary_frame_for_test(0);
        // The original comparison supplies sparse initialized Cells and
        // minimal House/Country bindings. Full terrain and House construction
        // RNG are explicitly outside this native prefix's coverage.
        sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(
            160,
            128,
            crate::map::resolved_terrain::test_clear_cell,
        ));
        for authored in &roster.houses {
            let house = sim.interner.intern(&authored.name);
            let country = authored
                .country
                .as_deref()
                .map(|name| sim.interner.intern(name));
            sim.houses
                .register(crate::sim::house_state::HouseState::new(
                    house, 0, country, false, 0, 10,
                ));
        }
        sim.initialize_trigger_definitions(&map, &teams, Some(&eva), None);
        sim.trigger_runtime.fixture_difficulty = Some(integer(&row["input"]["difficulty"]));
        let runtime = &sim.trigger_runtime;
        let final_order = row["type_orders"].as_array().unwrap().last().unwrap();
        assert_eq!(
            json!(runtime.definitions.team_types),
            final_order["registries"]["team_type"]
        );
        assert_eq!(
            json!(
                runtime
                    .definitions
                    .trigger_types
                    .iter()
                    .map(|typ| &typ.id)
                    .collect::<Vec<_>>()
            ),
            final_order["registries"]["trigger_type"]
        );
        assert_eq!(
            json!(
                runtime
                    .definitions
                    .tag_types
                    .iter()
                    .map(|typ| &typ.id)
                    .collect::<Vec<_>>()
            ),
            final_order["registries"]["tag_type"]
        );
        let tag_definitions: Vec<_> = runtime
            .definitions
            .tag_types
            .iter()
            .enumerate()
            .map(|(index, typ)| {
                json!({ "id": typ.id, "repeat": typ.repeat,
                "trigger": typ.trigger.map(|id| &runtime.definitions.trigger_types[id].id),
                "flags": runtime.definitions.classification(index) })
            })
            .collect();
        assert_eq!(json!(tag_definitions), row["definitions"]);
        for selected in row["selected_triggers"].as_array().unwrap() {
            let index = runtime
                .definitions
                .trigger_types
                .iter()
                .position(|typ| typ.id == selected["id"].as_str().unwrap())
                .unwrap();
            let typ = &runtime.definitions.trigger_types[index];
            assert_eq!(json!(index), selected["index"]);
            assert_eq!(json!(typ.owner), selected["owner"]);
            assert_eq!(
                json!(
                    typ.events
                        .iter()
                        .map(|event| json!({"kind": event.kind, "value":event.value}))
                        .collect::<Vec<_>>()
                ),
                selected["events"]
            );
            let actions: Vec<_> = typ.actions.iter().map(|action| {
                let (mut team, mut trigger, mut tag) = (Value::Null, Value::Null, Value::Null);
                match action.reference {
                    Some(DefinitionReference::Team(index)) => team = json!(runtime.definitions.team_types[index]),
                    Some(DefinitionReference::Trigger(index)) => trigger = json!(runtime.definitions.trigger_types[index].id),
                    Some(DefinitionReference::Tag(index)) => tag = json!(runtime.definitions.tag_types[index].id),
                    None => {},
                }
                json!({ "kind": action.entry.kind, "value": action.value,
                    "parameters": action.entry.params.iter().skip(2).take(4).map(|value| crt_atoi(value)).collect::<Vec<_>>(),
                    "team": team, "trigger": trigger, "tag": tag })
            }).collect();
            assert_eq!(json!(actions), selected["actions"]);
        }
        let ((), draws) =
            crate::sim::rng::trace_draws(|| sim.install_authored_cell_trigger_tags(&map));
        let mut draw_count = draws.len();
        compare_stock_state(&sim, draw_count, &row["history"][0]);
        let ((), draws) = crate::sim::rng::trace_draws(|| sim.classify_map_triggers(&rules));
        draw_count += draws.len();
        compare_stock_state(&sim, draw_count, &row["history"][1]);
        sim.trigger_runtime.stop_before_action = Some(integer(&row["first_poll"]["action_kind"]));
        let boundary = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            sim.advance_triggers(&rules, None, FrameEffects::default())
        }))
        .expect_err("same original first gameplay-action boundary must stop the prefix");
        assert_eq!(*boundary.downcast::<i32>().unwrap(), 46);
        sim.trigger_runtime.stop_before_action = None;
        compare_stock_state(&sim, draw_count, &row["history"][2]);
        let intro_type = sim
            .trigger_runtime
            .definitions
            .trigger_types
            .iter()
            .position(|typ| typ.id == "0DD8E18C")
            .unwrap();
        let intro = *sim
            .trigger_runtime
            .trigger_order
            .iter()
            .find(|&&id| sim.trigger_runtime.trigger(id).unwrap().type_index == intro_type)
            .unwrap();
        let setup_type = sim
            .trigger_runtime
            .definitions
            .trigger_types
            .iter()
            .position(|typ| typ.id == "0B76DF3C")
            .unwrap();
        let enable = sim.trigger_runtime.definitions.trigger_types[setup_type]
            .actions
            .iter()
            .find(|action| action.entry.kind == 53)
            .unwrap()
            .clone();
        for (step, observation) in row["observations"].as_array().unwrap().iter().enumerate() {
            sim.session
                .set_binary_frame_for_test(observation["frame"].as_u64().unwrap() as u32);
            let (returned, draws) = crate::sim::rng::trace_draws(|| {
                if observation["phase"] == "resolved53_enable" {
                    sim.apply_live_trigger_action(
                        &enable,
                        sim.trigger_action_context(
                            setup_type,
                            None,
                            TagEventInput::default(),
                            &rules,
                        ),
                        &mut Vec::new(),
                        &rules,
                        None,
                        FrameEffects::default(),
                    )
                } else {
                    sim.register_live_trigger_event(
                        intro,
                        TagEventInput {
                            kind: 13,
                            actor: None,
                            source: None,
                            cell: (0, 0),
                            force: false,
                        },
                        false,
                        crate::rules::ruleset::RuleSet::empty_for_test(),
                    )
                }
            });
            draw_count += draws.len();
            assert_eq!(json!(i32::from(returned)), observation["returned"]);
            compare_stock_state(&sim, draw_count, &row["history"][step + 3]);
        }
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"]["scenario"]
        );
        compared += 1;
    }
    assert_eq!(compared, 3);
}

fn supplied_definitions(input: &Value) -> RuntimeDefinitions {
    let default = json!([{ "events": [[47,0]], "actions": [[28,7]] }]);
    let triggers = input
        .get("triggers")
        .unwrap_or(&default)
        .as_array()
        .unwrap();
    let mut definitions = RuntimeDefinitions::default();
    for (index, trigger) in triggers.iter().enumerate() {
        let flags = trigger.get("flags").cloned().unwrap_or(json!([1, 1, 1, 1]));
        let events = trigger["events"]
            .as_array()
            .map_or_else(Vec::new, |events| {
                events
                    .iter()
                    .map(|event| EventCondition {
                        kind: integer(&event[0]),
                        value: integer(&event[1]),
                        ..EventCondition::default()
                    })
                    .collect()
            });
        let actions = trigger["actions"]
            .as_array()
            .map_or_else(Vec::new, |actions| {
                actions
                    .iter()
                    .map(|action| RuntimeAction {
                        entry: ActionEntry {
                            kind: integer(&action[0]),
                            params: Vec::new(),
                            waypoint_index: Some(0),
                        },
                        value: Some(integer(&action[1])),
                        reference: action
                            .get(2)
                            .map(|target| DefinitionReference::Trigger(integer(target) as usize)),
                        sound: None,
                    })
                    .collect()
            });
        definitions.trigger_types.push(TriggerType {
            id: format!("TRIGGER{index}"),
            owner: Some("Americans".into()),
            linked: (index + 1 < triggers.len()).then_some(index + 1),
            enabled: integer(&flags[3]) != 0,
            difficulty: [
                integer(&flags[0]) != 0,
                integer(&flags[1]) != 0,
                integer(&flags[2]) != 0,
            ],
            events: events.into_iter().map(RuntimeEvent::from).collect(),
            transfer_on_capture: flags.get(4).is_some_and(|value| integer(value) != 0),
            actions,
        });
    }
    for index in 0..8 {
        definitions.tag_types.push(TagType {
            id: format!("TAG{index}"),
            name: format!("TAG{index}"),
            repeat: input.get("repeat").map_or(2, integer),
            trigger: (!triggers.is_empty()).then_some(0),
        });
    }
    definitions
}

fn supplied_house(sim: &mut Simulation, input: &Value) -> crate::sim::intern::InternedId {
    let owner = sim.interner.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, Some(owner), false, 0, 10);
    if input["active_cleanup"].as_bool().unwrap_or(false) {
        house.supply_allow_win_tag_state(
            input.get("allow_win_count").map_or(3, integer),
            CdTimer::from_raw(-1, 99),
        );
    }
    sim.houses.register(house);
    owner
}

fn supplied_rules() -> crate::rules::ruleset::RuleSet {
    crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[Countries]\n0=Americans\n[Americans]\nName=United States\n",
    ))
    .unwrap()
}

fn history_state(
    sim: &Simulation,
    input: &Value,
    identities: &[usize],
    objects: &[u64],
    returned: Option<i32>,
    draws: usize,
    house: crate::sim::intern::InternedId,
) -> Value {
    let runtime = &sim.trigger_runtime;
    let mixed = input["mixed_deferred_queue"].as_bool().unwrap_or(false);
    let ticket_value = |ticket: DeferredDeleteTicket| match ticket {
        DeferredDeleteTicket::Object(id) => json!([
            "object",
            objects.iter().position(|&object| object == id).unwrap()
        ]),
        DeferredDeleteTicket::Tag(id) => {
            json!(["tag", identities.iter().position(|&tag| tag == id).unwrap()])
        }
        DeferredDeleteTicket::Trigger(id) => json!(["trigger", id]),
        DeferredDeleteTicket::Overlay(_) | DeferredDeleteTicket::DiskLaser(_) => {
            panic!("mixed Object/Tag/Trigger controls contain no Overlay/DiskLaser")
        }
    };
    let destructs: Vec<_> = sim
        .lifecycle_test_events_for_test()
        .iter()
        .filter_map(|event| match event {
            crate::sim::world::LifecycleTestEvent::DeferredDeleteSelected { ticket } => {
                Some(*ticket)
            }
            _ => None,
        })
        .collect();
    let tags: Vec<_> = identities
        .iter()
        .map(|&index| {
            let Some(tag) = runtime.tag(index) else {
                return Value::Null;
            };
            let triggers: Vec<_> = tag
                .triggers
                .iter()
                .map(|&index| {
                    let trigger = runtime.trigger(index).unwrap();
                    json!({ "type": trigger.type_index, "enabled": i32::from(trigger.enabled),
                "pending": i32::from(trigger.pending), "start": trigger.timer.start_frame(),
                "duration": trigger.timer.duration(), "completed": trigger.completed })
                })
                .collect();
            json!({ "refs": tag.refs, "pending": i32::from(tag.pending), "triggers": triggers })
        })
        .collect();
    let pending_tags: Vec<_> = sim
        .substrate
        .pending_delete
        .retained_tickets()
        .filter_map(|pending| match pending {
            DeferredDeleteTicket::Tag(index) => {
                Some(identities.iter().position(|id| *id == index).unwrap())
            }
            _ => None,
        })
        .collect();
    let mut state = json!({ "returned": returned, "tags": tags, "actions": runtime.action_trace,
        "rng_draws": draws, "pending_count": sim.substrate.pending_delete.len(), "pending_tags": pending_tags,
        "globals": runtime.globals_set, "locals": runtime.locals_set, "changed": i32::from(runtime.dirty[0]) });
    if input["lifecycle"].as_bool().unwrap_or(false) {
        let tags = runtime.tag_order.len();
        let triggers = runtime.trigger_order.len();
        state["frees"] = if input["operations"].as_array().is_some_and(|operations| {
            operations
                .iter()
                .any(|operation| operation[0] == "dispose_object")
        }) {
            // The direct-scalar control has no pending-selection visit.
            // Observe the same physical finalizer rather than fabricating
            // a DeferredDeleteSelected ticket for that entry route.
            assert!(runtime.freed_trace.is_empty());
            json!(
                sim.lifecycle_test_events_for_test()
                    .iter()
                    .filter_map(|event| match event {
                        crate::sim::world::LifecycleTestEvent::FinalizedCommon { stable_id }
                            if objects.contains(stable_id) =>
                            Some("object"),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            )
        } else if mixed {
            json!(
                destructs
                    .iter()
                    .map(|ticket| match ticket {
                        DeferredDeleteTicket::Object(_) => "object",
                        DeferredDeleteTicket::Tag(_) => "tag",
                        DeferredDeleteTicket::Trigger(_) => "trigger",
                        DeferredDeleteTicket::Overlay(_) | DeferredDeleteTicket::DiskLaser(_) =>
                            unreachable!(),
                    })
                    .collect::<Vec<_>>()
            )
        } else {
            json!(runtime.freed_trace)
        };
        // The corpus supplies initialized empty Objects to exercise attachment
        // teardown. These counts describe those supplied identities only.
        let retained_objects = if mixed {
            objects
                .iter()
                .filter(|&&id| sim.classify_object(id).is_some())
                .count()
        } else {
            objects.len()
        };
        state["registry_counts"] = json!({ "abstract": tags + triggers + retained_objects,
            "tag_trigger_base": tags + triggers, "tag": tags, "trigger": triggers,
            "pending": sim.substrate.pending_delete.len(), "cell_tags": runtime.cell_tags.len(),
            "logic_tags": runtime.logic_tags.len(), "map_tags": runtime.map_tags.len(),
            "object": retained_objects, "object_base": retained_objects, "object_base_b": retained_objects });
        state["object_tags"] = json!(
            objects
                .iter()
                .map(|id| runtime
                    .attachments
                    .get(&TagAttachment::Object(*id))
                    .map(|tag| identities
                        .iter()
                        .position(|identity| identity == tag)
                        .unwrap()))
                .collect::<Vec<_>>()
        );
    }
    if mixed {
        state["pending_tickets"] = json!(
            sim.substrate
                .pending_delete
                .retained_tickets()
                .map(ticket_value)
                .collect::<Vec<_>>()
        );
        state["destruct_order"] =
            json!(destructs.into_iter().map(ticket_value).collect::<Vec<_>>());
        state["object_alive"] = json!(
            objects
                .iter()
                .map(|&id| sim
                    .substrate
                    .entities
                    .get(id)
                    .map(|entity| i32::from(entity.lifecycle.object_alive())))
                .collect::<Vec<_>>()
        );
    }
    if input["polling"].as_bool().unwrap_or(false) {
        state["dispatches"] = json!(
            runtime
                .dispatch_trace
                .iter()
                .map(|(tag, kind)| (
                    identities
                        .iter()
                        .position(|identity| identity == tag)
                        .unwrap(),
                    *kind
                ))
                .collect::<Vec<_>>()
        );
        state["logic_tags"] = json!(
            runtime
                .logic_tags
                .iter()
                .map(|tag| identities
                    .iter()
                    .position(|identity| identity == tag)
                    .unwrap())
                .collect::<Vec<_>>()
        );
        state["scenario_flags"] = json!(runtime.dirty.map(i32::from));
    }
    if input["active_cleanup"].as_bool().unwrap_or(false) {
        let (count, timer) = sim.houses[&house].allow_win_tag_state();
        state["logic_cursor"] = json!(runtime.logic_cursor);
        state["map_cursor"] = json!(runtime.map_cursor);
        state["allow_win_count"] = json!(count);
        state["allow_win_timer"] = json!([timer.start_frame(), timer.duration()]);
    }
    state
}

#[test]
fn live_instances_match_all_native_supplied_histories_and_serialization() {
    let damage: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_damage_events.json",
    ))
    .unwrap();
    let destruction = damage["object_destruction_controls"].as_array().unwrap();
    assert_eq!(destruction.len(), 3);
    assert_eq!(
        replay_native_histories(rows().into_iter().chain(destruction.iter().cloned())),
        65
    );
}

/// One history driver for the original lifecycle corpus and bounded consumers.
/// Operations continue through the same production owners and saved authority.
pub(super) fn replay_native_histories(rows: impl Iterator<Item = Value>) -> usize {
    use crate::sim::rng::SimRng;

    let mut compared = 0;
    for row in rows.filter(|row| {
        !row["input"]["stock_prefix"].as_bool().unwrap_or(false)
            && !row["input"]["reference_controls"]
                .as_bool()
                .unwrap_or(false)
    }) {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let mut sim = Simulation::with_seed(input["seed"].as_u64().unwrap_or(31));
        if let Some(states) = row.get("full_rng_before") {
            // Newer controls supply all three process/scenario priors. In
            // particular, their native MapGen Seed(31) differs from a fresh
            // fixed-map Simulation's process default Seed(0).
            sim.scenario_rng =
                SimRng::from_native_state_hex_for_test(states["scenario"].as_str().unwrap());
            sim.main_rng =
                SimRng::from_native_state_hex_for_test(states["main"].as_str().unwrap()).into();
            sim.mapgen_rng =
                SimRng::from_native_state_hex_for_test(states["mapgen"].as_str().unwrap());
        }
        // The original small-world fixture supplies initialized CellClass
        // allocations at(50,60)/(51,60). This is a storage prerequisite,
        // not a claim about the production retail terrain-construction pass.
        sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(
            64,
            64,
            crate::map::resolved_terrain::test_clear_cell,
        ));
        sim.session
            .set_binary_frame_for_test(input["frame"].as_u64().unwrap_or(123) as u32);
        sim.trigger_runtime.definitions = supplied_definitions(input);
        sim.trigger_runtime.fixture_difficulty = Some(input.get("difficulty").map_or(1, integer));
        sim.trigger_runtime.logic_cursor = input.get("logic_cursor").map_or(0, integer);
        sim.trigger_runtime.map_cursor = input.get("map_cursor").map_or(0, integer);
        let house = supplied_house(&mut sim, input);
        let rules = supplied_rules();
        let flags = input.get("live_flags").cloned().unwrap_or(json!([0, 0]));
        let default_operations = json!([["shared"], ["dispatch", 0, 13], ["dispatch", 0, 13]]);
        let operations = input
            .get("operations")
            .unwrap_or(&default_operations)
            .as_array()
            .unwrap();
        let mut identities = Vec::new();
        let mut objects = Vec::new();
        let mut draws = 0;
        for (step, operation) in operations.iter().enumerate() {
            let args = operation.as_array().unwrap();
            let ((returned, effects), trace) = crate::sim::rng::trace_draws(|| {
                let mut effects = Vec::new();
                let mut returned = None;
                match args[0].as_str().unwrap() {
                    "shared" | "fresh" => {
                        let typ = args.get(1).map_or(0, integer) as usize;
                        let index = if args[0] == "shared" {
                            sim.shared_trigger_tag(typ)
                        } else {
                            sim.construct_trigger_tag(typ)
                        };
                        if !identities.contains(&index) {
                            identities.push(index);
                        }
                        returned =
                            Some(identities.iter().position(|id| *id == index).unwrap() as i32);
                    }
                    "frame" => sim
                        .session
                        .set_binary_frame_for_test(args[1].as_u64().unwrap() as u32),
                    "attach_cell" => {
                        let cell = crate::sim::world::Simulation::trigger_cell_identity(
                            &sim.resolved_terrain,
                            (integer(&args[2]) as i16, integer(&args[3]) as i16),
                        );
                        sim.trigger_runtime.replace_cell_trigger_tag(
                            cell,
                            Some(identities[integer(&args[1]) as usize]),
                        );
                    }
                    "object" => {
                        if input["mixed_deferred_queue"] == true {
                            // This control supplies a plain, unplaced Object
                            // lifetime. The shared drain's family admission,
                            // ordering and physical removal are compared; no
                            // concrete Unit destructor parity is claimed.
                            let id = sim.substrate.allocate_stable_id();
                            let type_id = sim.interner.intern("E1");
                            let mut object =
                                crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
                                    id,
                                    0,
                                    0,
                                    0,
                                    0,
                                    house,
                                    crate::sim::combat::Health::new(100),
                                    type_id,
                                    crate::map::entities::EntityCategory::Unit,
                                    0,
                                    5,
                                    false,
                                );
                            object.lifecycle.set_in_limbo_for_test(true);
                            object.in_logic_vector.set_for_test(false);
                            sim.substrate.entities.insert(object);
                            objects.push(id);
                        } else {
                            objects.push(objects.len() as u64);
                        }
                    }
                    "object_alive" => {
                        sim.substrate
                            .entities
                            .get_mut(objects[integer(&args[1]) as usize])
                            .unwrap()
                            .lifecycle
                            .set_object_alive_for_test(integer(&args[2]) != 0);
                    }
                    "dispose_object" => {
                        // The existing constructor-complete scalar owner
                        // shares physical finalization with deferred drain.
                        // This supplied Object control does not certify the
                        // stand-in Unit's concrete destructor or tracking.
                        assert!(sim.discard_constructed_limbo(
                            objects[integer(&args[1]) as usize],
                            &rules,
                            FrameEffects::default(),
                        ));
                    }
                    "supply_pending" => {
                        sim.substrate.pending_delete.clear_for_test();
                        for ticket in args[1].as_array().unwrap() {
                            let index = integer(&ticket[1]) as usize;
                            match ticket[0].as_str().unwrap() {
                                "object" => {
                                    sim.substrate.pending_delete.queue_object(objects[index])
                                }
                                "tag" => sim.substrate.pending_delete.queue_tag(identities[index]),
                                "trigger" => sim.substrate.pending_delete.queue_trigger(index),
                                family => panic!("unexpected supplied family {family}"),
                            }
                        }
                    }
                    "attach_object" => {
                        let tag = (integer(&args[2]) >= 0)
                            .then(|| identities[integer(&args[2]) as usize]);
                        returned = Some(i32::from(
                            sim.trigger_runtime.replace_trigger_tag_attachment(
                                TagAttachment::Object(objects[integer(&args[1]) as usize]),
                                tag,
                            ),
                        ));
                    }
                    "dispatch" => {
                        let cell = args.get(3).map_or((0, 0), |cell| {
                            (integer(&cell[0]) as i16, integer(&cell[1]) as i16)
                        });
                        let force = args.get(4).is_some_and(|value| integer(value) != 0);
                        let object = args.get(5).map(|index| objects[integer(index) as usize]);
                        returned = Some(i32::from(sim.dispatch_live_tag_event(
                            identities[integer(&args[1]) as usize],
                            TagEventInput {
                                kind: integer(&args[2]),
                                actor: object,
                                source: None,
                                cell,
                                force,
                            },
                            &mut effects,
                            &rules,
                            None,
                            FrameEffects::default(),
                        )));
                    }
                    "global" | "local" => {
                        returned = Some(i32::from(
                            crate::sim::world::Simulation::set_trigger_variable(
                                &sim.session,
                                &mut sim.scenario_rng,
                                &mut sim.trigger_runtime,
                                args[0] == "global",
                                integer(&args[1]),
                                integer(&args[2]) != 0,
                            ),
                        ))
                    }
                    "action" => {
                        let action = RuntimeAction {
                            entry: ActionEntry {
                                kind: integer(&args[1]),
                                params: Vec::new(),
                                waypoint_index: Some(0),
                            },
                            reference: Some(DefinitionReference::Trigger(
                                integer(&args[2]) as usize
                            )),
                            value: Some(0),
                            sound: None,
                        };
                        returned = Some(i32::from(sim.apply_live_trigger_action(
                            &action,
                            sim.trigger_action_context(0, None, TagEventInput::default(), &rules),
                            &mut effects,
                            &rules,
                            None,
                            FrameEffects::default(),
                        )));
                    }
                    "mark_tag" => crate::sim::world::Simulation::mark_live_tag_for_delete(
                        &mut sim.substrate,
                        &mut sim.trigger_runtime,
                        identities[integer(&args[1]) as usize],
                    ),
                    "mark_trigger" => {
                        let trigger = sim
                            .trigger_runtime
                            .tag(identities[integer(&args[1]) as usize])
                            .unwrap()
                            .triggers[integer(&args[2]) as usize];
                        crate::sim::world::Simulation::mark_live_trigger_for_delete(
                            &mut sim.substrate,
                            &mut sim.trigger_runtime,
                            trigger,
                        );
                    }
                    "drain" => sim.process_pending_delete_with(
                        &rules,
                        None,
                        FrameEffects::default(),
                        integer(&flags[0]),
                        integer(&flags[1]) != 0,
                    ),
                    "register_logic" => {
                        sim.trigger_runtime.logic_tags = args[1..]
                            .iter()
                            .map(|i| identities[integer(i) as usize])
                            .collect()
                    }
                    "register_map" => {
                        sim.trigger_runtime.map_tags = args[1..]
                            .iter()
                            .map(|i| identities[integer(i) as usize])
                            .collect()
                    }
                    "scenario_flags" => {
                        sim.trigger_runtime.dirty = [
                            integer(&args[1]) != 0,
                            integer(&args[2]) != 0,
                            integer(&args[3]) != 0,
                            integer(&args[4]) != 0,
                        ]
                    }
                    "poll" => {
                        effects = sim.advance_triggers(&rules, None, FrameEffects::default());
                    }
                    kind => panic!("unhandled native operation {name}/{step}/{kind}"),
                }
                (returned, effects)
            });
            assert!(
                effects.is_empty(),
                "supplied native histories do not contain presentation actions"
            );
            draws += trace.len();
            sim.substrate.pending_delete.assert_consistent_for_test();
            assert_eq!(
                history_state(&sim, input, &identities, &objects, returned, draws, house),
                row["history"][step],
                "{name}/{step}: {operation}"
            );
            let bytes =
                save_codec::serialize(&(&sim.trigger_runtime, &sim.substrate.pending_delete))
                    .unwrap();
            let (mut restored, restored_queue): (TriggerRuntime, DeferredDeleteQueue) =
                save_codec::deserialize(&bytes).unwrap();
            assert_eq!(restored, sim.trigger_runtime, "{name}/{step} saved owner");
            assert_eq!(
                restored_queue, sim.substrate.pending_delete,
                "{name}/{step} saved queue"
            );
            restored_queue.assert_consistent_for_test();
            let mut original_hash = std::collections::hash_map::DefaultHasher::new();
            let mut restored_hash = std::collections::hash_map::DefaultHasher::new();
            sim.trigger_runtime.hash_state(&mut original_hash);
            restored.hash_state(&mut restored_hash);
            sim.substrate.pending_delete.hash_state(&mut original_hash);
            restored_queue.hash_state(&mut restored_hash);
            assert_eq!(
                original_hash.finish(),
                restored_hash.finish(),
                "{name}/{step} saved hash"
            );
            // Continue through the restored authority at every boundary. Only
            // cumulative test observers are transferred outside serialization.
            restored.action_trace = std::mem::take(&mut sim.trigger_runtime.action_trace);
            restored.dispatch_trace = std::mem::take(&mut sim.trigger_runtime.dispatch_trace);
            restored.freed_trace = std::mem::take(&mut sim.trigger_runtime.freed_trace);
            sim.trigger_runtime = restored;
            sim.substrate.pending_delete = restored_queue;
            if row.get("object_tag_raw_history").is_some() {
                // Original destruction keeps Tag+2C after the last object
                // is gone. Preserve it across restore without retaining a
                // Rust attachment to vanished object storage.
                sim.validate_restored_trigger_runtime().unwrap();
            }
        }
        if let Some(states) = row.get("full_rng_after_history") {
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                states["scenario"],
                "{name}: full Scenario"
            );
            assert_eq!(
                sim.main_rng.native_state_hex(),
                states["main"],
                "{name}: full Main"
            );
            assert_eq!(
                sim.mapgen_rng.native_state_hex(),
                states["mapgen"],
                "{name}: full MapGen"
            );
        }
        assert_eq!(
            sim.scenario_rng.next_u32(),
            row["next_random"].as_u64().unwrap() as u32,
            "{name} final RNG continuation"
        );
        compared += 1;
    }
    compared
}

#[test]
fn native_trigger_reveal_initial_event48_tags_receive_neutral_discovery_dispatch() {
    use crate::sim::game_entity::GameEntity;
    use crate::sim::house_state::HouseState;
    use crate::sim::world::UninitContext;
    let Some(raw_map) = crate::rules::retail_ini_fixture::retail_archive_bytes("SOV01UMD.MAP")
    else {
        return;
    };
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    let map = crate::map::map_file::MapFile::from_bytes(&raw_map).unwrap();
    let corpus = crate::sim::vision::discovery_corpus();
    let footprint = &corpus["full_startup_footprint"];
    assert_eq!(
        crate::util::sha256::sha256_hex(&raw_map),
        footprint["sources"]["sov01umd.map"]["sha256"]
    );
    assert!(
        footprint["event4_lead_intersection"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut compared = 0;
    for lead in footprint["admitted_structure_anchor_leads"]
        .as_array()
        .unwrap()
    {
        if lead["type"] != "GTGCAN" {
            continue;
        }
        let mut sim = Simulation::with_seed(31);
        let player = sim.interner.intern("Player");
        let owner = sim.interner.intern(lead["owner"].as_str().unwrap());
        sim.session.set_current_house_for_test(Some(player));
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false),
        );
        sim.houses
            .register(HouseState::new(player, 0, None, true, 0, 10));
        sim.houses
            .register(HouseState::new(owner, 0, None, false, 0, 10));
        sim.initialize_trigger_definitions(
            &map,
            &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
            None,
            None,
        );
        let typ = sim.interner.intern("GTGCAN");
        let id = sim.substrate.allocate_stable_id();
        let mut entity = GameEntity::new_at_frame_for_test(
            id,
            integer(&lead["cell"][0]) as u16,
            integer(&lead["cell"][1]) as u16,
            4,
            0,
            owner,
            crate::sim::combat::Health::new(100),
            typ,
            crate::map::entities::EntityCategory::Structure,
            0,
            0,
            false,
            0,
        );
        entity.building_art.set_actually_placed_for_test(true);
        sim.substrate.entities.insert(entity);
        sim.attach_object_trigger_tag(id, Some(lead["tag"].as_str().unwrap()));
        let tag = sim.trigger_runtime.attachments[&TagAttachment::Object(id)];
        let chain = sim.trigger_runtime.tag(tag).unwrap().triggers.clone();
        assert_eq!(chain.len(), 1);
        let trigger = sim.trigger_runtime.trigger(chain[0]).unwrap();
        let events = &sim.trigger_runtime.definitions.trigger_types[trigger.type_index].events;
        assert_eq!(events.len(), 1);
        assert_eq!(
            json!(events[0].kind),
            lead["native_trigger"]["events"][0]["kind"]
        );
        assert_eq!(events[0].kind, 48);
        let before = sim.trigger_runtime.clone();
        let (_, draws) = crate::sim::rng::trace_draws(|| {
            sim.with_object_placement_scope(|sim| {
                assert!(sim.discover_techno(id, owner, UninitContext::with_rules(&rules)));
            });
            assert!(sim.trigger_runtime.dispatch_trace.is_empty());
            assert!(sim.discover_techno(id, player, UninitContext::with_rules(&rules)));
        });
        assert!(draws.is_empty());
        assert_eq!(sim.trigger_runtime.dispatch_trace, [(tag, 4)]);
        assert_eq!(sim.trigger_runtime, before);
        assert!(sim.drain_trigger_effects().is_empty());
        assert!(!sim.discover_techno(id, player, UninitContext::with_rules(&rules)));
        assert_eq!(sim.trigger_runtime.dispatch_trace, [(tag, 4)]);
        compared += 1;
    }
    assert_eq!(compared, 2);
}
