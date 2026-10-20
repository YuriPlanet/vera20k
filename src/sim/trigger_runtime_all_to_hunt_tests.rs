//! Action6's original selector/ordinary Foot controls. Native Team/queue
//! callbacks are bounded leaves; the connected retail test exercises their
//! existing Rust owners, not a complete native ALL06 mission.
use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::{combat::Health, game_entity::GameEntity, house_state::HouseState};
use serde_json::Value;

fn corpus() -> &'static Value {
    let result = crate::test_fixture::json("tools/spatial_oracle/trigger_all_to_hunt.json");
    assert_eq!(
        result["native_sha256"],
        "3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600"
    );
    result
}

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap().try_into().unwrap()
}

#[test]
fn all_to_hunt_action_matches_original_selector_mode_timer_and_ordinary_foot() {
    use crate::sim::{rng::SimRng, timer::CdTimer};
    let corpus = corpus();
    let native = &corpus["action6"];
    let mut ini = String::from("[Countries]\n");
    for index in 0..8 {
        ini.push_str(&format!("{index}=C{index}\n"));
    }
    for index in 0..8 {
        ini.push_str(&format!("[C{index}]\n"));
    }
    ini.push_str(
        "[VehicleTypes]\n0=U\n[BuildingTypes]\n0=B\n[U]\nStrength=100\n[B]\nStrength=100\n",
    );
    let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
    let controls = native["controls"].as_array().unwrap();
    assert_eq!(controls.len(), 8);
    for control in controls {
        let input = &control["input"];
        let mut sim = Simulation::with_seed(31);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let houses: Vec<_> = native["country_priors"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(slot, country)| {
                let owner = sim.interner.intern(&format!("H{slot}"));
                let country = sim.interner.intern(&format!("C{}", int(country)));
                let mut house = HouseState::new(owner, slot as u8, Some(country), false, 0, 10);
                house
                    .strategy_emergency_mut()
                    .set_mode_for_test(int(&control["modes_before"][slot]));
                house.set_strategy_timer_for_test(CdTimer::from_raw(
                    int(&control["strategy_timers_before"][slot][0]),
                    int(&control["strategy_timers_before"][slot][2]),
                ));
                sim.houses.register(house);
                owner
            })
            .collect();
        sim.houses
            .reorder_for_test(&input["order"].as_array().map_or_else(
                || houses.clone(),
                |order| {
                    order
                        .iter()
                        .map(|slot| houses[slot.as_u64().unwrap() as usize])
                        .collect()
                },
            ));
        super::tests::install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nCHECK=C0,<none>,Check,0,1,1,1,0\n[Events]\nCHECK=1,8,0,0\n[Actions]\nCHECK={}\n[Tags]\nTAG=2,Check,CHECK\n",
                control["native_read"]["read"].as_str().unwrap()
            ),
        );
        let live = sim.trigger_runtime.trigger_order[0];
        let typ = sim.trigger_runtime.trigger(live).unwrap().type_index;
        // Defaults are the actual ActionFixture priors in the corpus generator:
        // retained House2 and incoming House1; explicit null controls override.
        sim.trigger_runtime.triggers[live]
            .as_mut()
            .unwrap()
            .retained_house = if input.get("trigger_owner").is_some_and(Value::is_null) {
            None
        } else {
            Some(houses[2])
        };
        let actors: Vec<_> = native["actors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|actor| {
                let id = sim.allocate_stable_id();
                let owner = houses[actor["owner"].as_u64().unwrap() as usize];
                let foot = actor["foot"].as_bool().unwrap();
                let ty = sim.interner.get(if foot { "U" } else { "B" }).unwrap();
                let mut entity = GameEntity::new_at_frame_zero_for_test(
                    id,
                    10,
                    10,
                    0,
                    0,
                    owner,
                    Health::new(100),
                    ty,
                    if foot {
                        EntityCategory::Unit
                    } else {
                        EntityCategory::Structure
                    },
                    0,
                    5,
                    false,
                );
                entity
                    .lifecycle
                    .set_cell_marked(actor["down"].as_bool().unwrap());
                entity
                    .lifecycle
                    .set_in_limbo_for_test(actor["limbo"].as_bool().unwrap());
                sim.substrate.entities.insert(entity);
                if actor["team"].as_bool().unwrap() {
                    let script = sim.interner.intern("SUPPLIED_TEAM");
                    sim.team_script_vm.register_script(
                        crate::sim::team_script_vm::TeamScriptDefinition {
                            id: script,
                            source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
                            actions: vec![],
                        },
                    );
                    sim.team_script_vm.create_team(owner, script, vec![id], 0);
                }
                id
            })
            .collect();
        let rng = &native["rng_full_seed31"];
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(rng["scenario"].as_str().unwrap());
        sim.main_rng = SimRng::from_native_state_hex_for_test(rng["main"].as_str().unwrap()).into();
        sim.mapgen_rng = SimRng::from_native_state_hex_for_test(rng["mapgen"].as_str().unwrap());
        let action = sim.trigger_runtime.definitions.trigger_types[typ].actions[0].clone();
        let actual = sim.apply_live_trigger_action(
            &action,
            TriggerActionContext {
                live_index: (!input["null_trigger"].as_bool().unwrap_or(false)).then_some(live),
                incoming_house: if input.get("incoming").is_some_and(Value::is_null) {
                    None
                } else {
                    Some(houses[1])
                },
                input: TagEventInput::default(),
            },
            &mut Vec::new(),
            Some(&rules),
            None,
            FrameEffects::default(),
        );
        assert_eq!(actual, int(&control["result"]) != 0, "{input}");
        for (slot, house) in houses.iter().enumerate() {
            assert_eq!(
                sim.houses[house].strategy_emergency().mode(),
                int(&control["modes_after"][slot]),
                "{input}"
            );
            assert_eq!(
                sim.houses[house].strategy_emergency().all_to_hunt_bias(),
                int(&control["latch_after"][slot]) != 0,
                "{input}"
            );
            assert_eq!(
                *sim.houses[house].strategy_timer(),
                CdTimer::from_raw(
                    int(&control["strategy_timers_after"][slot][0]),
                    int(&control["strategy_timers_after"][slot][2])
                ),
                "{input}"
            );
        }
        for (slot, id) in actors.iter().enumerate() {
            let queued =
                control["events"].as_array().unwrap().iter().any(|e| {
                    e["call"] == "queue_mission" && e["actor"].as_u64() == Some(slot as u64)
                });
            assert_eq!(
                sim.substrate
                    .entities
                    .get(*id)
                    .unwrap()
                    .mission
                    .queued()
                    .known()
                    == Some(crate::sim::mission::MissionType::Hunt),
                queued,
                "{input} actor{slot}"
            );
            if queued {
                assert!(
                    sim.team_script_vm.team_for_member(*id).is_none(),
                    "{input} actor{slot}"
                );
            }
        }
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            rng["scenario"].as_str().unwrap(),
            "{input}"
        );
        assert_eq!(
            sim.main_rng.native_state_hex(),
            rng["main"].as_str().unwrap(),
            "{input}"
        );
        assert_eq!(
            sim.mapgen_rng.native_state_hex(),
            rng["mapgen"].as_str().unwrap(),
            "{input}"
        );
    }
}

#[test]
fn all_to_hunt_retail_all06_elapsed_enable_chain_releases_existing_gattling_team() {
    use crate::rules::{process_owner::NativeScenarioRulesPrefix, team_ai_ini::TeamAiIniRegistry};
    use crate::sim::mission::MissionType;
    let corpus = corpus();
    let native = &corpus["retail"];
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let raw_map = assets.get_ref("ALL06UMD.MAP").unwrap();
    assert_eq!(
        crate::util::sha256::sha256_hex(raw_map),
        native["map_sha256"]
    );
    assert_eq!(
        crate::util::sha256::sha256_hex(assets.get_ref("RULESMD.INI").unwrap()),
        native["rules_sha256"]
    );
    let map = crate::map::map_file::MapFile::from_bytes(raw_map).unwrap();
    let named = assets.resolve_ref("ALL06UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL06UMD.INI")
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
    let ai = crate::rules::retail_sources::select_ini(&assets, "AIMD.INI")
        .unwrap()
        .ini;
    let teams = TeamAiIniRegistry::from_sources(&ai, &map.ini, false);
    for country in native["country_registration"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["entries"]
        .as_array()
        .unwrap()
    {
        assert_eq!(
            rules.country_name(crate::rules::ruleset::CountryIdx(
                int(&country["index"]) as u16
            )),
            country["name"].as_str()
        );
    }
    let chain = ["05C6450C", "05C6397C"];
    let mut body = String::new();
    for section in ["Triggers", "Events", "Actions", "Tags"] {
        body.push_str(&format!("[{section}]\n"));
        for (key, value) in map.ini.section(section).unwrap().raw_entries() {
            if (section == "Tags"
                && map
                    .tags
                    .get(key)
                    .is_some_and(|tag| tag.trigger().is_some_and(|id| chain.contains(&id))))
                || (section != "Tags" && chain.contains(&key))
            {
                body.push_str(&format!("{key}={value}\n"));
            }
        }
    }
    for id in chain {
        for (section, field) in [
            ("Triggers", "trigger"),
            ("Events", "event"),
            ("Actions", "actions"),
        ] {
            assert_eq!(
                map.ini.section(section).unwrap().get_for_test(id),
                native["routes"][id][field].as_str()
            );
        }
    }
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let mut sim = Simulation::with_seed(31);
    crate::sim::scenario_bootstrap::initialize_map_roster_houses(
        &mut sim,
        &roster,
        Some(&rules),
        Some(&map.ini),
    );
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let owner = sim.interner.get("YuriCountry").unwrap();
    assert_eq!(
        sim.house_by_country_index(crate::rules::ruleset::CountryIdx(9), &rules),
        Some(owner)
    );
    assert!(
        map.entities
            .iter()
            .all(|entity| entity.owner != "YuriCountry"
                || !matches!(
                    entity.category,
                    EntityCategory::Unit | EntityCategory::Infantry
                ))
    );
    super::tests::install_event_ingress_map(&mut sim, &rules, &body);
    sim.install_team_ai_registry(&teams, &rules).unwrap();
    // Explicit prerequisite: Wave4 has already arrived. ALL06 starts with no
    // Yuri Foot; its whole reinforcement sequence and physical terrain are
    // outside this cropped integration. Use its real Team/TF/Script and the
    // existing constructor/membership owners for the selected three YTNK.
    let selected = &native["preferred_ordinary_team"];
    for (id, fields) in [
        (selected["id"].as_str().unwrap(), &selected["team"]),
        (
            selected["task_force_id"].as_str().unwrap(),
            &selected["task_force"],
        ),
        (selected["script_id"].as_str().unwrap(), &selected["script"]),
    ] {
        for (key, value) in fields.as_object().unwrap() {
            assert_eq!(
                map.ini.section(id).unwrap().get_for_test(key),
                value.as_str()
            );
        }
    }
    let typ = sim.interner.get(selected["id"].as_str().unwrap()).unwrap();
    let team = sim.create_team(typ, Some(owner), &rules).unwrap();
    let actors: Vec<_> = [(10, 10), (12, 10), (14, 10)]
        .into_iter()
        .map(|(x, y)| {
            let id = sim
                .spawn_object("YTNK", "YuriCountry", x, y, 0, &rules)
                .unwrap();
            assert!(sim.team_add_member(team, id, false, &rules, None, FrameEffects::default()));
            id
        })
        .collect();
    let enemy = sim
        .spawn_object("HTNK", "Americans", 20, 10, 0, &rules)
        .unwrap();
    let enabled = |sim: &Simulation, id: &str| {
        sim.trigger_runtime.trigger_order.iter().any(|&live| {
            let trigger = sim.trigger_runtime.trigger(live).unwrap();
            sim.trigger_runtime.definitions.trigger_types[trigger.type_index].id == id
                && trigger.enabled
        })
    };
    assert!(!enabled(&sim, chain[0]) && !enabled(&sim, chain[1]));
    // Execute the actual Wave4 predecessor's final Enable action, supplying
    // only that already-arrived-wave precondition, excluding its other arms.
    let entry = map.actions["095EB2AC"].entries().last().unwrap().clone();
    let action = sim
        .trigger_runtime
        .definitions
        .resolve_action(entry, None, None);
    assert!(sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: None,
            incoming_house: None,
            input: TagEventInput::default()
        },
        &mut Vec::new(),
        Some(&rules),
        None,
        FrameEffects::default()
    ));
    assert!(enabled(&sim, chain[0]));
    for frame in [0, 3599, 3600, 4049, 4050] {
        sim.session.set_binary_frame_for_test(frame);
        sim.advance_triggers(Some(&rules), None, FrameEffects::default());
        for &id in &actors {
            assert_eq!(
                sim.team_script_vm.team_for_member(id).is_none(),
                frame >= 4050,
                "frame{frame}"
            );
            if frame >= 4050 {
                assert_eq!(
                    sim.substrate
                        .entities
                        .get(id)
                        .unwrap()
                        .mission
                        .queued()
                        .known(),
                    Some(MissionType::Hunt)
                );
            }
        }
    }
    assert!(sim.houses[&owner].strategy_emergency().all_to_hunt_bias());
    // Continue through the existing mission consumer on the selected
    // ordinary armed route. This is Rust integration, not a native full map.
    for &id in &actors {
        assert!(
            sim.mission_commence_exact(id, sim.session.binary_frame())
                .unwrap()
        );
    }
    for frame in 4051..4091 {
        sim.session.set_binary_frame_for_test(frame);
        for &id in &actors {
            sim.object_ai_visit_one(id, Some(&rules), crate::sim::world::ObjectAiCtx::default());
        }
        if actors.iter().all(|id| {
            sim.substrate
                .entities
                .get(*id)
                .unwrap()
                .attack_target
                .target()
                == Some(crate::sim::combat::TargetKind::Entity(enemy))
        }) {
            break;
        }
    }
    assert!(actors.iter().all(|id| {
        sim.substrate
            .entities
            .get(*id)
            .unwrap()
            .attack_target
            .target()
            == Some(crate::sim::combat::TargetKind::Entity(enemy))
    }));
}
