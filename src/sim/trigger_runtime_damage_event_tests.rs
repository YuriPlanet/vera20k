//! Bounded original HasOccurred71E940/StateA71F950/StateB71F9C0 and
//! Register7264C0 comparisons. Object damage emission is tested by its owner.

use super::tests::{event_ingress_fixture, install_event_ingress_map, spawn_event_ingress_unit};
use super::*;
use crate::map::map_file::MapFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::rng::SimRng;
use crate::util::save_codec;
use serde_json::{Value, json};

fn corpus() -> &'static Value {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/trigger_damage_events.json");
    assert_eq!(corpus["code_unchanged"], true);
    corpus
}

fn integer(value: &Value) -> i32 {
    value.as_i64().unwrap().try_into().unwrap()
}

fn timer(value: &Value) -> CdTimer {
    // The middle native dword is zero in these controls. The shared Rust
    // owner retains the signed anchor/duration at +0/+8.
    assert_eq!(value[1], 0);
    CdTimer::from_raw(integer(&value[0]), integer(&value[2]))
}

fn install_rng_boundary(sim: &mut Simulation, states: &Value) {
    sim.scenario_rng = SimRng::from_native_state_hex_for_test(states["scenario"].as_str().unwrap());
    sim.main_rng = SimRng::from_native_state_hex_for_test(states["main"].as_str().unwrap()).into();
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(states["mapgen"].as_str().unwrap());
}

fn rng_states(sim: &Simulation) -> Value {
    json!({
        "scenario": sim.scenario_rng.native_state_hex(),
        "main": sim.main_rng.native_state_hex(),
        "mapgen": sim.mapgen_rng.native_state_hex(),
    })
}

#[test]
fn explosion_damage_tag_chain_respects_depth_and_recovers_on_a_small_stack() {
    // Fix the stack size so a larger RUST_MIN_STACK cannot hide recursive
    // Execute -> AreaDamage -> ObjectHealth -> Tag -> Spring frames.
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let (mut sim, rules, overlays, houses) = super::tests::event_ingress_fixture_with_rules(
                // This fixture appends rules inside TAGUNIT's section. Root
                // the weapon before the production WeaponType reading pass.
                "Primary=ChainBlast\n\
             [ChainBlast]\nDamage=1\nROF=1\nRange=1\nProjectile=ChainInviso\nWarhead=ChainWH\n\
             [ChainInviso]\nInviso=yes\n\
             [ChainWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            );
            assert_eq!(rules.weapon_by_array_index(0).unwrap().id, "ChainBlast");
            assert_eq!(rules.weapon_by_array_index(0).unwrap().warhead.as_deref(), Some("ChainWH"));
            let count = MAX_TRIGGER_SPRING_DEPTH + 1;
            let (mut triggers, mut tags, mut events, mut actions, mut waypoints) = (
                String::from("[Triggers]\n"),
                String::from("[Tags]\n"),
                String::from("[Events]\n"),
                String::from("[Actions]\n"),
                String::from("[Waypoints]\n"),
            );
            let mut targets = Vec::new();
            for index in 0..=count {
                let cell = (10 + 2 * (index % 6) as u16, 10 + 2 * (index / 6) as u16);
                targets.push(spawn_event_ingress_unit(&mut sim, &rules, houses[0], cell));
                triggers.push_str(&format!("TR{index:02}=Country0,<none>,Chain,0,1,1,1,0\n"));
                // One-shot Tags avoid latched callbacks springing again on the
                // same receiver's later events while the first chain unwinds.
                tags.push_str(&format!("TAG{index:02}=0,Chain,TR{index:02}\n"));
                events.push_str(&format!("TR{index:02}=1,41,0,0\n"));
                waypoints.push_str(&format!(
                    "{}={}\n",
                    26 + index,
                    u32::from(cell.1) * 1000 + u32::from(cell.0)
                ));
                if index + 1 < count {
                    // Two-letter tokens AA.. begin at waypoint 26.
                    let next = (index + 1) as u8;
                    let token = format!(
                        "{}{}",
                        char::from(b'A' + next / 26),
                        char::from(b'A' + next % 26)
                    );
                    actions.push_str(&format!("TR{index:02}=1,42,0,0,0,0,0,0,{token}\n"));
                } else {
                    let marker = if index == count { 10 } else { 9 };
                    actions.push_str(&format!("TR{index:02}=1,28,0,{marker},0,0,0,0,A\n"));
                }
            }
            let map = install_event_ingress_map(
                &mut sim,
                &rules,
                &format!("{triggers}{tags}{events}{actions}{waypoints}"),
            );
            sim.session.install_waypoints(&map.waypoints);
            for (index, &target) in targets.iter().enumerate() {
                sim.attach_object_trigger_tag(target, Some(&format!("TAG{index:02}")));
            }
            super::tests::event_ingress_damage(
                &mut sim,
                &rules,
                &overlays,
                targets[0],
                crate::sim::combat::RAD_NO_ATTACKER,
                1,
                FrameEffects::default(),
            );
            assert_eq!(
                sim.trigger_runtime
                    .action_trace
                    .iter()
                    .filter(|row| row.0 == 42)
                    .count(),
                MAX_TRIGGER_SPRING_DEPTH,
            );
            assert_eq!(
                sim.entities()
                    .get(targets[count - 1])
                    .unwrap()
                    .health
                    .current(),
                99,
                "real damage reaches the refused Tag"
            );
            assert!(
                !sim.global_variables()[9],
                "the next nested Spring is refused"
            );
            sim.trigger_runtime.action_trace.clear();
            super::tests::event_ingress_damage(
                &mut sim,
                &rules,
                &overlays,
                targets[count],
                crate::sim::combat::RAD_NO_ATTACKER,
                1,
                FrameEffects::default(),
            );
            assert!(
                sim.global_variables()[10],
                "the next production damage dispatch recovers"
            );
            assert_eq!(sim.trigger_runtime.action_trace, [(28, 10)]);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn retail_no_dock_crash_springs_41_before_record_kill() {
    use crate::sim::world::LifecycleTestEvent;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let object = rules.object("ORCA").unwrap();
    assert!(object.airport_bound && !object.dock.is_empty());
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 5);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    let aircraft = sim
        .spawn_object("ORCA", "Americans", 15, 15, 0, &rules)
        .unwrap();
    let owner = sim.entities().get(aircraft).unwrap().owner();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_country_for_test(Some(owner));
    // Supplied airborne idle-entry boundary; the separate game replay uses
    // ordinary Move from the ground to reach this no-dock caller naturally.
    sim.set_object_height(
        aircraft,
        object.flight_level(rules.general.flight_level),
        &rules,
        None,
        FrameEffects::default(),
    );
    assert_eq!(
        sim.entities().get(aircraft).unwrap().health.current(),
        object.strength
    );
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nFIRST=Americans,DEATH,Crash,0,1,1,1,0\n\
         DEATH=Americans,<none>,Kill,0,1,1,1,0\n\
         [Tags]\nTAG=1,Crash,FIRST\n\
         [Events]\nFIRST=1,41,0,0\nDEATH=1,29,0,0\n\
         [Actions]\nFIRST=1,28,0,41,0,0,0,0,A\n\
         DEATH=1,28,0,29,0,0,0,0,A\n",
    );
    assert_eq!(
        resolve_trigger_house(&sim, Some("Americans"), &rules),
        Some(owner)
    );
    sim.attach_object_trigger_tag(aircraft, Some("TAG"));
    let tag = sim.trigger_runtime.attachments[&TagAttachment::Object(aircraft)];
    sim.trigger_runtime.dispatch_trace.clear();
    sim.clear_lifecycle_test_events_for_test();
    sim.scenario_rng = SimRng::new(5);
    let (_, draws) = crate::sim::rng::trace_draws(|| {
        crate::sim::aircraft::enter_idle_mode_for(
            &mut sim,
            aircraft,
            &rules,
            None,
            FrameEffects::default(),
        );
    });
    // Crash4DEC0F raises41 before RecordKill4DEC51 can raise29. The sole
    // Repeat1 attachment must detach on41, excluding the linked29 action.
    assert!(sim.global_variables()[41]);
    assert!(!sim.global_variables()[29]);
    assert_eq!(sim.trigger_runtime.dispatch_trace, [(tag, 41)]);
    assert!(!sim.object_has_trigger_tag(aircraft));
    assert_eq!(sim.houses[&owner].stats().units_lost(), 1);
    assert_eq!(sim.lifecycle_test_events_for_test().iter().filter(|event| {
        matches!(event, LifecycleTestEvent::RecordKillBookkeeping { stable_id } if *stable_id == aircraft)
    }).count(), 1);
    let plane = sim.entities().get(aircraft).unwrap();
    assert_eq!(plane.health.current(), 0);
    assert!(plane.lifecycle.object_alive() && plane.crash.is_crashing());
    assert_eq!(
        draws.len(),
        3,
        "the seed5 spin boundary adds no Trigger RNG draws"
    );
}

#[test]
fn live_crash_with_attacker_keeps_both_41_visits() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    let attacker = spawn_event_ingress_unit(&mut sim, &rules, houses[1], (17, 15));
    sim.set_object_height(
        victim,
        512,
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    );
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nHIT=Country0,FIRST,Hit,0,1,1,1,0\n\
         FIRST=Country0,<none>,First,0,1,1,1,0\n\
         [Events]\nHIT=1,38,0,0\nFIRST=1,41,0,0\n\
         [Actions]\nHIT=1,28,0,38,0,0,0,0,A\nFIRST=1,28,0,41,0,0,0,0,A\n\
         [Tags]\nTAG=2,Crash,HIT\n",
    );
    sim.attach_object_trigger_tag(victim, Some("TAG"));
    sim.trigger_runtime.dispatch_trace.clear();
    assert!(sim.foot_crash(
        victim,
        Some(attacker),
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    ));
    // Crash4DEBEC/4DEC0F/4DEC36: the persistent Tag survives all three
    // visits, including the second41 carrying the retained attacker.
    assert_eq!(
        sim.trigger_runtime
            .dispatch_trace
            .iter()
            .take(3)
            .map(|row| row.1)
            .collect::<Vec<_>>(),
        [38, 41, 41],
    );
    // ResetTimers726400 clears only13/51 completion bits. These discrete
    // nodes remain latched and may spring again during later RecordKill visits.
    assert!(sim.global_variables()[38] && sim.global_variables()[41]);
    assert!(sim.object_has_trigger_tag(victim));
}

#[test]
fn crash_stops_when_a_real_tag_action_uninits_before_or_during_record_kill() {
    for kind in [41, 29] {
        let (mut sim, rules, overlays, houses) = event_ingress_fixture();
        let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
        assert!(!rules.object("TAGUNIT").unwrap().crashable);
        // Supplied shared-Foot boundary: C4 on this non-Crashable Unit
        // reaches UnInit, rather than an Aircraft's nested dead-HP Crash.
        sim.set_object_height(
            victim,
            512,
            &rules,
            Some(&overlays),
            FrameEffects::default(),
        );
        install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nKILL=Country0,<none>,Kill,0,1,1,1,0\n\
                 [Events]\nKILL=1,{kind},0,0\n\
                 [Actions]\nKILL=1,32,0,0,0,0,0,0,A\n[Tags]\nTAG=2,Kill,KILL\n",
            ),
        );
        sim.attach_object_trigger_tag(victim, Some("TAG"));
        let (accepted, draws) = crate::sim::rng::trace_draws(|| {
            sim.foot_crash(
                victim,
                None,
                &rules,
                Some(&overlays),
                FrameEffects::default(),
            )
        });
        assert!(
            accepted,
            "event{kind}: a callback removal still accepts Crash"
        );
        let entity = sim.entities().get(victim).unwrap();
        assert!(!entity.is_object_alive(), "event{kind}: real C4/UnInit");
        assert_eq!(entity.health.current(), 0);
        assert!(
            !entity.crash.is_crashing(),
            "event{kind}: no outer crash tail"
        );
        assert!(entity.rocking.is_none());
        if kind == 41 {
            assert_eq!(
                sim.houses[&houses[0]].stats().units_lost(),
                1,
                "the prefix callback's kill is not booked again by outer Crash"
            );
        }
        assert!(
            draws.is_empty(),
            "event{kind}: this minimal death adds no spin draws"
        );
    }
}

#[test]
fn damage_event_predicates_match_original_discrete_controls() {
    let corpus = corpus();
    let cases = corpus["controls"].as_array().unwrap();
    assert_eq!(cases.len(), 30);
    assert_eq!(corpus["predicate_rng"]["unchanged"], true);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let (mut sim, rules, _, houses) = event_ingress_fixture();
        sim.session.set_binary_frame_for_test(123);
        let actor = case["actor_owner"].as_u64().map(|index| {
            spawn_event_ingress_unit(&mut sim, &rules, houses[index as usize], (15, 15))
        });
        let source = case["source_owner"].as_u64().map(|index| {
            spawn_event_ingress_unit(&mut sim, &rules, houses[index as usize], (17, 15))
        });
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
        let events = &sim.trigger_runtime.definitions.trigger_types[trigger_type].events;
        assert_eq!(events.len(), 1, "{name}: counted event reader");
        assert_eq!(events[0].kind, integer(&case["read"]["kind"]), "{name}");
        assert_eq!(events[0].value, integer(&case["read"]["value"]), "{name}");
        assert_eq!(
            events[0].type_name.as_deref().unwrap_or(""),
            case["read"]["name"].as_str().unwrap(),
            "{name}: counted event reader"
        );
        assert_eq!(
            events[0].team_name.is_some(),
            case["read"]["team_present"].as_bool().unwrap(),
            "{name}: counted event reader"
        );
        // Represent the supplied native sentinel by a distinct retained Rust
        // identity. Neither representation may be replaced by actor/House.
        sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0].retained_house =
            Some(houses[2]);
        sim.trigger_runtime.triggers[live].as_mut().unwrap().timer = timer(&case["timer_before"]);
        install_rng_boundary(&mut sim, &corpus["predicate_rng"]["full_seed31"]);
        let mut persistence = integer(&case["initial_persistence"]) != 0;
        let mut predicate = |sim: &mut Simulation| {
            sim.has_occurred(
                0,
                TagEventInput {
                    kind: integer(&case["raised"]),
                    actor,
                    source,
                    ..Default::default()
                },
                case["incoming_house"]
                    .as_u64()
                    .map(|index| houses[index as usize]),
                &mut persistence,
                &rules,
                live,
            )
        };
        let (result, draws) = crate::sim::rng::trace_draws(|| {
            if case["scenario_init"].as_bool().unwrap() {
                sim.with_buildup_bypass(predicate)
            } else {
                predicate(&mut sim)
            }
        });
        assert_eq!(result, integer(&case["result"]) != 0, "{name}: predicate");
        assert_eq!(persistence, integer(&case["persistence"]) != 0, "{name}");
        assert_eq!(
            sim.trigger_runtime.trigger(live).unwrap().timer,
            timer(&case["timer_after"]),
            "{name}: timer"
        );
        assert_eq!(case["event54"], case["event54_before"], "{name}");
        assert_eq!(
            sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0].retained_house,
            Some(houses[2]),
            "{name}: retained House"
        );
        assert!(draws.is_empty(), "{name}: RNG calls");
        assert_eq!(
            rng_states(&sim),
            corpus["predicate_rng"]["full_after"],
            "{name}: all three complete native RNG states"
        );
    }
    let classification = corpus["classification"].as_array().unwrap();
    assert_eq!(classification.len(), 6);
    for row in classification {
        assert_eq!(row["state_a"], 1);
        assert_eq!(row["state_b"], 1);
        assert_eq!(event_classification(integer(&row["kind"])), 2);
    }
}

#[test]
fn damage_event_single_register_history_matches_original() {
    let corpus = corpus();
    let (mut sim, rules, _, _) = event_ingress_fixture();
    sim.session.set_binary_frame_for_test(123);
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nDAMAGE=Country0,<none>,Damage,0,1,1,1,0\n\
         [Tags]\nTAG=2,Damage,DAMAGE\n[Events]\nDAMAGE=1,38,0,0\n",
    );
    let typ = sim.trigger_runtime.definitions.tag_type("TAG").unwrap();
    let tag = sim.shared_trigger_tag(typ);
    let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
    let trigger_type = sim.trigger_runtime.trigger(live).unwrap().type_index;
    let history = corpus["register_history"].as_array().unwrap();
    assert_eq!(history.len(), 3);
    sim.trigger_runtime.triggers[live].as_mut().unwrap().timer =
        timer(&history[0]["prior"]["timer"]);
    install_rng_boundary(&mut sim, &corpus["predicate_rng"]["full_seed31"]);
    // This is one original single-event history: matching/wrong/matching.
    // It establishes no multi-condition completion-latch behavior.
    for row in history {
        let name = row["name"].as_str().unwrap();
        let before = sim.trigger_runtime.trigger(live).unwrap();
        assert_eq!(before.timer, timer(&row["prior"]["timer"]), "{name}");
        assert_eq!(before.completed, integer(&row["prior"]["completed"]) as u32);
        assert_eq!(row["prior"]["house"], 0);
        assert_eq!(row["prior"]["event_house"], 0);
        assert!(before.retained_house.is_none());
        assert!(
            sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0]
                .retained_house
                .is_none()
        );
        assert!(row["actor_owner"].is_null());
        let (result, draws) = crate::sim::rng::trace_draws(|| {
            sim.register_live_trigger_event(
                live,
                TagEventInput {
                    kind: integer(&row["raised"]),
                    ..Default::default()
                },
                row["repeat"].as_bool().unwrap(),
                &rules,
            )
        });
        assert_eq!(result, integer(&row["result"]) != 0, "{name}: Register");
        let after = sim.trigger_runtime.trigger(live).unwrap();
        assert_eq!(after.timer, timer(&row["timer"]), "{name}: timer");
        assert_eq!(after.completed, integer(&row["completed"]) as u32, "{name}");
        assert_eq!(row["house"], 0);
        assert!(after.retained_house.is_none(), "{name}: live House");
        assert_eq!(row["event_house"], 0);
        assert!(
            sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0]
                .retained_house
                .is_none(),
            "{name}: shared event House"
        );
        assert!(draws.is_empty(), "{name}: RNG calls");
        assert_eq!(
            rng_states(&sim),
            corpus["predicate_rng"]["full_after"],
            "{name}: all three complete native RNG states"
        );
    }
}

struct RetailDamageScene {
    sim: Simulation,
    rules: RuleSet,
    overlays: OverlayTypeRegistry,
    map: MapFile,
}

fn retail_damage_scene(route: &Value) -> Option<RetailDamageScene> {
    use crate::rules::{
        ini_parser::IniFile, process_owner::NativeScenarioRulesPrefix, sound_ini::EvaRegistry,
        team_ai_ini::TeamAiIniRegistry,
    };

    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let map_name = route["map"].as_str().unwrap();
    let raw = assets.get_ref(map_name).unwrap();
    assert_eq!(crate::util::sha256::sha256_hex(raw), route["map_sha256"]);
    let map = MapFile::from_bytes(raw).unwrap();
    let named_name = map_name.replace(".MAP", ".INI");
    let named = assets.resolve_ref(&named_name).map(|_| {
        crate::rules::retail_sources::select_ini(&assets, &named_name)
            .unwrap()
            .ini
    });
    let mut rules_owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (mut rules, processed, art, _) = rules_owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    let overlays = OverlayTypeRegistry::from_ini(&processed, Some(&art));
    let fixed_ai = crate::rules::retail_sources::select_ini(&assets, "AIMD.INI")
        .unwrap()
        .ini;
    let teams = TeamAiIniRegistry::from_sources(&fixed_ai, &map.ini, false);
    if let Some(team_id) = route["team_id"].as_str() {
        let team = teams
            .team_types
            .iter()
            .find(|team| team.id == team_id)
            .unwrap();
        for (key, value) in route["team"].as_object().unwrap() {
            assert_eq!(team.section().read_string(key, "", 512), *value);
        }
        for (section, id, expected) in [
            ("TaskForces", "task_force_id", "task_force"),
            ("ScriptTypes", "script_id", "script"),
        ] {
            let name = route[id].as_str().unwrap();
            assert!(
                map.ini
                    .section(section)
                    .unwrap()
                    .raw_entries()
                    .any(|(_, value)| value == name)
            );
            for (key, value) in route[expected].as_object().unwrap() {
                assert_eq!(
                    map.ini.section(name).unwrap().read_string(key, "", 512),
                    *value
                );
            }
        }
        let script = teams
            .scripts
            .iter()
            .find(|script| script.id == route["script_id"].as_str().unwrap())
            .unwrap();
        for (index, action) in script.actions.iter().enumerate() {
            assert_eq!(
                format!("{},{}", action.action_id, action.argument),
                route["script"][index.to_string()]
            );
        }
    }
    let mut records = String::new();
    for section in ["Triggers", "Events", "Actions", "Tags"] {
        records.push_str(&format!("[{section}]\n"));
        for (key, value) in map.ini.section(section).unwrap().raw_entries() {
            let expected = if section == "Tags" {
                route["tags"].get(key)
            } else {
                route["linked_triggers"].get(key).map(|record| {
                    &record[match section {
                        "Triggers" => "trigger",
                        "Events" => "raw_event",
                        "Actions" => "actions",
                        _ => unreachable!(),
                    }]
                })
            };
            if let Some(expected) = expected {
                assert_eq!(value, expected.as_str().unwrap());
                records.push_str(&format!("{key}={value}\n"));
            }
        }
    }
    for (key, value) in route["structures"].as_object().unwrap() {
        assert_eq!(
            map.ini
                .section("Structures")
                .unwrap()
                .read_string(key, "", 512),
            *value
        );
    }
    let excerpt = MapFile::from_bytes(
        format!(
            "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
             [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n{records}"
        )
        .as_bytes(),
    )
    .unwrap();
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
        &rules,
        Some(&map.ini),
    );
    sim.install_team_ai_registry(&teams, &rules).unwrap();
    let eva =
        EvaRegistry::from_ini(&IniFile::from_bytes(assets.get_ref("EVAMD.INI").unwrap()).unwrap());
    sim.initialize_trigger_definitions(&excerpt, &teams, Some(&eva), None);
    sim.classify_map_triggers(&rules);
    // Declared clear geometry isolates the selected received-damage route.
    // Retail rules/roster/definitions remain intact; whole campaign timing,
    // terrain, animation asset binding and world RNG parity are not claimed.
    let terrain_rules = crate::rules::terrain_rules::TerrainRules::from_ini(&processed);
    let clear = terrain_rules.semantics_for_land_type(0).unwrap();
    let terrain = crate::map::resolved_terrain::test_grid(64, 64, |x, y| {
        let mut cell = crate::map::resolved_terrain::test_clear_cell(x, y);
        cell.speed_costs = clear.speed_costs;
        cell.base_speed_costs = clear.speed_costs;
        cell
    });
    let bounds =
        crate::map::playfield::PlayfieldBounds::from_normalized_local_size(32, 0, 0, 32, 32);
    let path = crate::sim::pathfinding::PathGrid::from_resolved_terrain(&terrain);
    sim.zone_grid = Some(
        crate::sim::pathfinding::zone_map::ZoneGrid::build_with_native_map_context(
            &path,
            &terrain,
            &[],
            Some((32, 32)),
            Some(bounds),
        ),
    );
    sim.install_fixture_path_grid(Some(&path));
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.set_playfield_bounds_for_test(Some(bounds));
    sim.set_playfield_size_height_for_test(Some(32));
    sim.session.set_map_width_for_test(64);
    sim.session.set_map_height_for_test(64);
    Some(RetailDamageScene {
        sim,
        rules,
        overlays,
        map,
    })
}

#[test]
fn retail_all05_first_damage_creates_defense_team_from_existing_units() {
    use super::tests::event_ingress_damage;

    let corpus = corpus();
    let route = &corpus["retail_routes"][0];
    assert_eq!(route["map"], "ALL05UMD.MAP");
    let Some(RetailDamageScene {
        mut sim,
        rules,
        overlays,
        map,
    }) = retail_damage_scene(route)
    else {
        return;
    };
    let mut record = map
        .entities
        .iter()
        .find(|entity| {
            entity.type_id == "YAPOWR" && entity.trigger_tag.as_deref() == Some("07787C6C")
        })
        .unwrap()
        .clone();
    record.cell_x = 32;
    record.cell_y = 32;
    let owner = sim.interner.get(&record.owner).unwrap();
    let terrain = sim.resolved_terrain.as_ref().unwrap().clone();
    assert_eq!(
        sim.spawn_from_map_with_resolved(&[record], Some(&rules), Some(&terrain)),
        1
    );
    let victim = sim
        .entities()
        .iter_sorted()
        .find(|(_, entity)| sim.interner.resolve(entity.type_ref()) == "YAPOWR")
        .unwrap()
        .0;
    let tag = sim.trigger_runtime.attachments[&TagAttachment::Object(victim)];
    let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
    let definition = &sim.trigger_runtime.definitions.trigger_types
        [sim.trigger_runtime.trigger(live).unwrap().type_index];
    assert_eq!(definition.id, route["trigger_id"]);
    assert_eq!(
        definition.events[0].kind,
        integer(&route["native_event"]["conditions"][0]["kind"])
    );
    assert_eq!(
        resolve_trigger_house(&sim, definition.owner.as_deref(), &rules),
        Some(owner)
    );
    let team_type = sim
        .interner
        .get(route["team_id"].as_str().unwrap())
        .unwrap();
    let team_definition = *sim.team_script_vm.team_type_definition(team_type).unwrap();
    assert_eq!(
        sim.interner.resolve(team_definition.script_id),
        route["script_id"]
    );
    let task_force = sim.team_script_vm.task_force_of(team_type).unwrap();
    assert_eq!(sim.interner.resolve(task_force.id), route["task_force_id"]);
    let requirements = task_force.entries.clone();
    let owner_name = sim.interner.resolve(owner).to_owned();
    let mut supplied = Vec::new();
    for requirement in requirements {
        let type_name = sim.interner.resolve(requirement.member_type.id).to_owned();
        for _ in 0..requirement.count {
            let x = 26 + supplied.len() as u16;
            supplied.push(
                sim.spawn_object_at_height(&type_name, &owner_name, x, 36, 0, 0, &rules)
                    .unwrap(),
            );
        }
    }
    assert_eq!(supplied.len(), 5);
    let count = sim.entities().len();
    let initial_health = sim.entities().get(victim).unwrap().health.current();
    sim.trigger_runtime.action_trace.clear();
    // A real source also reaches the original duplicate41 visit. The first
    // one-shot spring must detach before Object rereads its Tag for that visit.
    event_ingress_damage(
        &mut sim,
        &rules,
        &overlays,
        victim,
        supplied[0],
        1,
        FrameEffects::default(),
    );
    assert_eq!(
        sim.entities().get(victim).unwrap().health.current(),
        initial_health - 1
    );
    assert_eq!(
        sim.entities().len(),
        count,
        "CreateTeam constructs no fresh Foot objects"
    );
    let teams: Vec<_> = sim
        .team_script_vm
        .teams_in_order()
        .filter(|team| team.team_type_id() == Some(team_type))
        .collect();
    assert_eq!(teams.len(), 1);
    assert_eq!(teams[0].owner(), owner);
    assert_eq!(teams[0].script_id(), team_definition.script_id);
    assert_eq!(
        teams[0].member_count(),
        0,
        "normal TeamAI owns later recruitment"
    );
    let team = teams[0].id();
    assert!(!sim.object_placement_scope_active());
    assert!(
        !sim.trigger_runtime
            .attachments
            .contains_key(&TagAttachment::Object(victim))
    );
    assert_eq!(
        sim.trigger_runtime
            .action_trace
            .iter()
            .map(|row| row.0)
            .collect::<Vec<_>>(),
        [4]
    );
    sim.flush_pending_delete();
    assert!(sim.trigger_runtime.tag(tag).is_none());
    assert!(sim.trigger_runtime.trigger(live).is_none());
    for _ in 0..3 {
        sim.run_team_ai_pass(&rules, Some(&overlays), FrameEffects::default());
    }
    let state = sim.team_script_vm.team(team).unwrap();
    let mut members: Vec<_> = state.members().collect();
    members.sort_unstable();
    supplied.sort_unstable();
    assert_eq!(
        members, supplied,
        "the retail TaskForce recruits the supplied existing objects"
    );
    assert_eq!(state.refusal(), None);
    event_ingress_damage(
        &mut sim,
        &rules,
        &overlays,
        victim,
        supplied[0],
        1,
        FrameEffects::default(),
    );
    sim.flush_pending_delete();
    assert_eq!(sim.team_script_vm.teams_in_order().count(), 1);
    assert_eq!(sim.entities().len(), count);
    assert_eq!(
        sim.trigger_runtime
            .action_trace
            .iter()
            .map(|row| row.0)
            .collect::<Vec<_>>(),
        [4]
    );
}

#[test]
fn retail_all06_red_damage_runs_parliament_destroy_attached_synchronously() {
    use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
    use crate::sim::world::LifecycleTestEvent;

    let corpus = corpus();
    let route = &corpus["retail_routes"][1];
    assert_eq!(route["map"], "ALL06UMD.MAP");
    let Some(RetailDamageScene {
        mut sim,
        rules,
        overlays,
        map,
    }) = retail_damage_scene(route)
    else {
        return;
    };
    let mut record = map
        .entities
        .iter()
        .find(|entity| {
            entity.type_id == "CALOND04" && entity.trigger_tag.as_deref() == Some("048BC38C")
        })
        .unwrap()
        .clone();
    record.cell_x = 32;
    record.cell_y = 32;
    let owner = sim.interner.get(&record.owner).unwrap();
    let terrain = sim.resolved_terrain.as_ref().unwrap().clone();
    assert_eq!(
        sim.spawn_from_map_with_resolved(&[record], Some(&rules), Some(&terrain)),
        1
    );
    let victim = sim
        .entities()
        .iter_sorted()
        .find(|(_, entity)| sim.interner.resolve(entity.type_ref()) == "CALOND04")
        .unwrap()
        .0;
    let tag = sim.trigger_runtime.attachments[&TagAttachment::Object(victim)];
    let native_cleanup = corpus["object_destruction_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "base_object_last_tag_reference")
        .unwrap();
    let native_tag = &native_cleanup["history"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["tags"][0];
    let retained_refs = integer(&native_tag["refs"]);
    assert_eq!(sim.trigger_runtime.tag(tag).unwrap().refs, retained_refs);
    let find_live = |sim: &Simulation, id: &str| {
        *sim.trigger_runtime
            .tag(tag)
            .unwrap()
            .triggers
            .iter()
            .find(|&&live| {
                sim.trigger_runtime.definitions.trigger_types
                    [sim.trigger_runtime.trigger(live).unwrap().type_index]
                    .id
                    == id
            })
            .unwrap()
    };
    let quarter = find_live(&sim, route["trigger_id"].as_str().unwrap());
    let next = find_live(&sim, "048BB6BC");
    assert!(sim.trigger_runtime.trigger(quarter).unwrap().enabled);
    assert!(!sim.trigger_runtime.trigger(next).unwrap().enabled);
    sim.session.set_binary_frame_for_test(123);
    // The original action retains encoded waypoint X (23); its coordinate
    // is supplied inside the declared clear arena, rather than the full map.
    sim.session
        .install_waypoints(&std::collections::HashMap::from([(
            23,
            crate::map::waypoints::Waypoint {
                index: 23,
                rx: 32,
                ry: 32,
            },
        )]));
    let initial_health = sim.entities().get(victim).unwrap().health.current();
    assert_eq!(initial_health, rules.object("CALOND04").unwrap().strength);
    assert!(initial_health > 1);
    let warhead = sim.interner.intern("SA");
    let hit = EntityDamageEvent::direct_receiver(
        victim,
        initial_health - 1,
        0,
        RAD_NO_ATTACKER,
        None,
        warhead,
        ReceiverCallFlags {
            ignore_defenses: true,
            arg6: false,
        },
    );
    sim.trigger_runtime.action_trace.clear();
    sim.trigger_runtime.dispatch_trace.clear();
    sim.clear_lifecycle_test_events_for_test();
    let _ = sim.drain_trigger_effects();
    sim.commit_direct_damage_receiver(&rules, Some(&overlays), hit, FrameEffects::default());
    assert_eq!(
        sim.trigger_runtime
            .action_trace
            .iter()
            .map(|row| row.0)
            .collect::<Vec<_>>(),
        [54, 32, 53, 112]
    );
    assert!(
        !sim.trigger_runtime.trigger(quarter).unwrap().enabled,
        "self-disable precedes the nested C4 receiver"
    );
    let enabled = sim.trigger_runtime.trigger(next).unwrap();
    assert!(enabled.enabled);
    assert_eq!(
        enabled.timer.start_frame(),
        123,
        "enable restarts the existing timer owner"
    );
    assert!(enabled.timer.duration() > 0);
    assert!(
        sim.entities()
            .get(victim)
            .is_none_or(|entity| entity.health.current() == 0 && !entity.lifecycle.object_alive()),
        "DestroyAttached finishes before the outer receiver returns"
    );
    let losses_after_kill = sim.houses[&owner].stats().buildings_lost();
    let raised: Vec<_> = sim
        .trigger_runtime
        .dispatch_trace
        .iter()
        .map(|row| row.1)
        .collect();
    assert_eq!(
        raised,
        [43, 48, 29],
        "nested kill visits retain the Tag dispatch guard; the outer receiver must reread Alive before first-damage events"
    );
    assert!(matches!(
        sim.drain_trigger_effects().as_slice(),
        [TriggerEffect::CameraAtCoord { velocity: None, .. }]
    ));
    sim.flush_pending_delete();
    assert!(sim.entities().get(victim).is_none());
    assert!(
        !sim.trigger_runtime
            .attachments
            .contains_key(&TagAttachment::Object(victim))
    );
    // Original scalar deletion loses the Object's storage, retaining its
    // Tag counter and live linked program. It does not call SetTag(NULL).
    // The production owner drops only Rust's derived attachment index.
    let retained = sim.trigger_runtime.tag(tag).unwrap();
    assert_eq!(retained.refs, retained_refs);
    assert_eq!(retained.pending, integer(&native_tag["pending"]) != 0);
    assert!(sim.trigger_runtime.logic_tags.contains(&tag));
    sim.validate_restored_trigger_runtime().unwrap();
    let bytes = save_codec::serialize(&sim.trigger_runtime).unwrap();
    let mut restored: TriggerRuntime = save_codec::deserialize(&bytes).unwrap();
    assert_eq!(restored, sim.trigger_runtime);
    restored.action_trace = std::mem::take(&mut sim.trigger_runtime.action_trace);
    restored.dispatch_trace = std::mem::take(&mut sim.trigger_runtime.dispatch_trace);
    sim.trigger_runtime = restored;
    sim.validate_restored_trigger_runtime().unwrap();
    // A repeated direct hit or deferred retirement must not book another
    // death after the nested receiver already completed it.
    let hit = EntityDamageEvent::direct_receiver(
        victim,
        1,
        0,
        RAD_NO_ATTACKER,
        None,
        warhead,
        ReceiverCallFlags {
            ignore_defenses: true,
            arg6: false,
        },
    );
    sim.commit_direct_damage_receiver(&rules, Some(&overlays), hit, FrameEffects::default());
    sim.flush_pending_delete();
    assert_eq!(
        sim.houses[&owner].stats().buildings_lost(),
        losses_after_kill
    );
    assert_eq!(
        sim.lifecycle_test_events_for_test().iter().filter(|event| {
            matches!(event, LifecycleTestEvent::RecordKillBookkeeping { stable_id } if *stable_id == victim)
        }).count(),
        1,
        "the nested receiver owns the single actual kill callback"
    );
    assert_eq!(
        sim.trigger_runtime
            .action_trace
            .iter()
            .map(|row| row.0)
            .collect::<Vec<_>>(),
        [54, 32, 53, 112]
    );

    // The same retail Tag must survive its building: the newly enabled13
    // program disables this chain and announces the loss after its timer.
    // This bounded Logic poll stops before the later campaign failure route.
    let timer = sim.trigger_runtime.trigger(next).unwrap().timer;
    sim.session
        .set_binary_frame_for_test(timer.start_frame().wrapping_add(timer.duration()) as u32);
    let previous_actions = sim.trigger_runtime.action_trace.len();
    let effects = sim.advance_triggers(&rules, Some(&overlays), FrameEffects::default());
    assert!(!sim.trigger_runtime.trigger(next).unwrap().enabled);
    assert_eq!(
        sim.trigger_runtime.action_trace[previous_actions..]
            .iter()
            .map(|row| row.0)
            .collect::<Vec<_>>(),
        map.actions["048BB6BC"]
            .entries()
            .iter()
            .map(|action| action.kind)
            .collect::<Vec<_>>()
    );
    assert!(effects.iter().any(|effect| {
        matches!(effect, TriggerEffect::LocalizedMessage { csf_label } if csf_label == "MISSION:LostStct")
    }));
    assert_eq!(
        sim.sound_events
            .iter()
            .filter(|event| { matches!(event, SimSoundEvent::TriggerEva { .. }) })
            .count(),
        1,
        "the retained Event13 route reaches its actual EVAMD announcement"
    );
    assert_eq!(sim.trigger_runtime.tag(tag).unwrap().refs, retained_refs);
}
