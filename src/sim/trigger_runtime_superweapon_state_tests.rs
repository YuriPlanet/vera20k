//! Original Execute132/135/136 controls and the authored ALL03 nuke chain.
//! Goldens are emitted by trigger_action_values --superweapon-state;
//! child reset tests consume its --superweapon-reset history.

use super::*;
use crate::map::map_file::MapFile;
use crate::map::waypoints::Waypoint;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::rng::SimRng;
use serde_json::{Value, json};

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/trigger_superweapon_state.json")
}

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap().try_into().unwrap()
}

fn cell(value: &Value) -> (u16, u16) {
    (int(&value[0]) as u16, int(&value[1]) as u16)
}

fn all03_inputs() -> Option<(RuleSet, MapFile)> {
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let raw = assets.get_ref("ALL03UMD.MAP").expect("retail ALL03");
    let map = MapFile::from_bytes(raw).unwrap();
    let data = corpus();
    assert_eq!(
        data["retail_inputs"]["native_sha256"],
        "3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600"
    );
    for source in data["retail_inputs"]["sources"].as_array().unwrap() {
        let name = source["file"].as_str().unwrap();
        assert_eq!(
            crate::util::sha256::sha256_hex(assets.get_ref(name).unwrap()),
            source["sha256"],
            "retail {name} identity"
        );
    }
    let named = assets.resolve_ref("ALL03UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL03UMD.INI")
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
    assert_eq!(
        json!(rules.super_weapon_order),
        data["retail_inputs"]["registry"]
    );
    let native_type = &data["retail_inputs"]["layers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["super_state"];
    assert_eq!(
        rules.super_weapon_order[int(&native_type["registry_index"]) as usize],
        "NukeSpecial"
    );
    assert_eq!(
        rules
            .super_weapon("NukeSpecial")
            .unwrap()
            .recharge_time_frames,
        int(&native_type["recharge_frames"])
    );
    Some((rules, map))
}

fn seeded_rngs(sim: &mut Simulation, data: &Value) {
    let seeds = &data["rng_seed_states"];
    sim.main_rng = SimRng::from_native_state_hex_for_test(seeds["main"].as_str().unwrap()).into();
    sim.scenario_rng = SimRng::from_native_state_hex_for_test(seeds["scenario"].as_str().unwrap());
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(seeds["mapgen"].as_str().unwrap());
}

fn rng_hashes(sim: &Simulation) -> Value {
    let hash = |hex: String| {
        let bytes: Vec<u8> = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        crate::util::sha256::sha256_hex(&bytes)
    };
    json!({"main": hash(sim.main_rng.native_state_hex()),
        "scenario": hash(sim.scenario_rng.native_state_hex()),
        "mapgen": hash(sim.mapgen_rng.native_state_hex())})
}

fn state(sim: &Simulation, owner: InternedId, id: InternedId) -> Value {
    let instance = &sim.super_weapons[&owner][&id];
    // Inspect the production serialized private field without exposing a raw
    // mutable slot to another simulation owner.
    let saved = serde_json::to_value(instance).unwrap();
    let preferred = sim.houses[&owner].preferred_super_weapon_target();
    json!({"start": instance.charge_start_tick(), "duration": instance.charge_duration(),
        "granted": instance.is_active(), "ready": instance.is_ready(),
        "held": instance.is_suspended(), "custom_recharge": saved["custom_recharge_time"],
        "preferred": [preferred.0 as i16, preferred.1 as i16]})
}

#[test]
fn all03_authored_nuke_order_preserves_current_timer_and_recharges_at_override() {
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    use crate::sim::superweapon::{self, ai_fire};
    let Some((rules, map)) = all03_inputs() else {
        return;
    };
    let data = corpus();
    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    let fixed_ai = crate::rules::retail_sources::select_ini(&assets, "AIMD.INI")
        .unwrap()
        .ini;
    let teams = TeamAiIniRegistry::from_sources(&fixed_ai, &map.ini, false);
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let route = data["retail_inputs"]["routes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|route| route["id"] == "0AFF8D8C")
        .unwrap();
    assert_eq!(
        map.ini
            .section("Actions")
            .unwrap()
            .read_string("0AFF8D8C", "", 512),
        route["actions"]
    );
    let tag_name = route["tags"].as_object().unwrap().keys().next().unwrap();

    for sequence in data["sequence_controls"].as_array().unwrap() {
        if sequence["name"] != "all03_authored_nuke" {
            continue;
        }
        let mut sim = Simulation::with_seed(0);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.session
            .set_binary_frame_for_test(int(&sequence["initial_frame"]) as u32);
        crate::sim::scenario_bootstrap::initialize_map_roster_houses(
            &mut sim,
            &roster,
            Some(&rules),
            Some(&map.ini),
        );
        sim.session.install_waypoints(&map.waypoints);
        sim.install_team_ai_registry(&teams, &rules).unwrap();
        sim.initialize_trigger_definitions(&map, &teams, None, None);
        let owner = sim.interner.get("YuriCountry").unwrap();
        let enemy = sim.interner.get("Americans").unwrap();
        let id = sim.interner.get("NukeSpecial").unwrap();
        let sw = rules.super_weapon("NukeSpecial").unwrap();
        // The native control supplies these already-admitted AI/power priors;
        // it does not execute the whole ALL03 power puzzle/House scheduler.
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_enemy_house_for_test(Some(enemy));
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_player_control_for_test(false);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false),
        );
        let silo = sim
            .spawn_object("NAMISL", "YuriCountry", 48, 89, 0, &rules)
            .unwrap();
        let tag_type = sim.trigger_runtime.definitions.tag_type(tag_name).unwrap();
        let tag = sim.shared_trigger_tag(tag_type);
        let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
        let typ = sim.trigger_runtime.trigger(live).unwrap().type_index;
        seeded_rngs(&mut sim, data);
        assert_eq!(rng_hashes(&sim), sequence["rng_before"]);
        let initial = state(&sim, owner, id);
        for (key, expected) in sequence["initial_state"].as_object().unwrap() {
            assert_eq!(&initial[key], expected, "initial {key}");
        }
        let mut ordinal = 0;
        for row in sequence["rows"].as_array().unwrap() {
            let stage = row["stage"].as_str().unwrap();
            sim.session
                .set_binary_frame_for_test(int(&row["frame"]) as u32);
            match stage {
                "grant" => superweapon::grant_provided_supers(&mut sim, &rules, owner),
                "before_elapsed_ai" | "elapsed_ai" => {
                    superweapon::tick_house_superweapon_instances(&mut sim, &rules, owner)
                }
                "npc_ai_fire" => {
                    ai_fire::AI_FIRE_LOG.set(Some(Vec::new()));
                    ai_fire::try_fire(&mut sim, &rules, owner, None, FrameEffects::default());
                    let log = ai_fire::AI_FIRE_LOG.take().unwrap();
                    let calls: Vec<Value> = log.into_iter().filter_map(|event| match event {
                        ai_fire::AiFireEvent::Fire(fired, target) => Some(json!({
                            "stage": stage, "call": "fire", "index": rules.super_weapon_index(sim.interner.resolve(fired)).unwrap(),
                            "cell": target
                        })),
                        ai_fire::AiFireEvent::BestRallyTarget => panic!("scripted nuke must bypass best-rally picker"),
                        _ => None,
                    }).collect();
                    assert_eq!(json!(calls), row["fire"]);
                    assert_eq!(json!(sim.houses[&owner].nuke_target()), row["nuke_target"]);
                    assert_eq!(
                        sim.entities()
                            .get(silo)
                            .unwrap()
                            .mission_leaf
                            .as_building()
                            .unwrap()
                            .firing_super_weapon(),
                        int(&row["firing_type"])
                    );
                }
                "reset_running" => superweapon::reset_super(&mut sim, &rules, owner, "NukeSpecial"),
                "enemy_selection_prior" => {
                    // Full House enemy selection/cadence is outside the
                    // original control, which supplies the chosen enemy here.
                    assert_eq!(row["enemy_index"], 0);
                    sim.houses
                        .get_mut(&owner)
                        .unwrap()
                        .set_enemy_house_for_test(Some(enemy));
                }
                _ if stage.starts_with("execute") => {
                    let action =
                        sim.trigger_runtime.definitions.trigger_types[typ].actions[ordinal].clone();
                    let parsed = crate::map::actions::parse_actions(
                        &crate::rules::ini_parser::IniFile::from_str(&format!(
                            "[Actions]\nROW=1,{}\n",
                            row["raw_action"].as_str().unwrap()
                        )),
                    )
                    .remove("ROW")
                    .unwrap()
                    .entries()[0]
                        .clone();
                    assert_eq!(action.entry, parsed, "actual retail ordered action");
                    ordinal += 1;
                    let returned = sim.apply_live_trigger_action(
                        &action,
                        sim.trigger_action_context(
                            typ,
                            Some(live),
                            TagEventInput::default(),
                            Some(&rules),
                        ),
                        &mut Vec::new(),
                        Some(&rules),
                        None,
                        FrameEffects::default(),
                    );
                    assert_eq!(i32::from(returned), int(&row["returned"]), "{stage}");
                }
                _ => panic!("unhandled native stage {stage}"),
            }
            let observed = state(&sim, owner, id);
            for (key, expected) in row["state"].as_object().unwrap() {
                assert_eq!(&observed[key], expected, "{stage}: {key}");
            }
            assert_eq!(observed["preferred"], row["preferred"], "{stage}");
            assert_eq!(
                sim.super_weapons[&owner][&id].recharge_time(sw),
                int(&row["effective_recharge"]),
                "{stage}"
            );
        }
        assert_eq!(ordinal, route["native_actions"].as_array().unwrap().len());
        assert_eq!(rng_hashes(&sim), sequence["rng_after"]);
    }
}

#[test]
fn dispatch_matches_original_signed_override_waypoint_and_incoming_house_gates() {
    let Some((rules, _)) = all03_inputs() else {
        return;
    };
    let data = corpus();
    for row in data["dispatch_controls"].as_array().unwrap() {
        let input = &row["input"];
        let mut sim = Simulation::with_seed(0);
        sim.intern_rule_type_ids(&rules);
        let owner = sim.interner.intern("IncomingHouse");
        let other = sim.interner.intern("UnrelatedHouse");
        for house in [owner, other] {
            sim.houses
                .register(HouseState::new(house, 0, None, false, 0, 10));
            crate::sim::superweapon::construct_house_supers(&mut sim, &rules, house, false);
        }
        let id = sim.interner.get("NukeSpecial").unwrap();
        sim.super_weapons
            .get_mut(&other)
            .unwrap()
            .get_mut(&id)
            .unwrap()
            .set_recharge_time(int(&input["other_custom_recharge"]));
        sim.houses
            .get_mut(&other)
            .unwrap()
            .set_preferred_super_weapon_target(cell(&input["other_preferred"]));
        let instance = sim
            .super_weapons
            .get_mut(&owner)
            .unwrap()
            .get_mut(&id)
            .unwrap();
        instance.set_recharge_time(int(&input["custom_recharge"]));
        instance.set_charge_start_tick_for_test(int(&input["start"]));
        instance.set_charge_duration_for_test(int(&input["duration"]));
        instance.set_is_active_for_test(input["granted"].as_bool().unwrap());
        instance.set_is_ready_for_test(input["ready"].as_bool().unwrap());
        instance.set_is_suspended_for_test(input["held"].as_bool().unwrap());
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_preferred_super_weapon_target(cell(&input["preferred"]));
        let unrelated = state(&sim, other, id);
        let ini = crate::rules::ini_parser::IniFile::from_str(&format!(
            "[Actions]\nCASE=1,{}\n",
            row["read"]["raw"].as_str().unwrap()
        ));
        let entry = crate::map::actions::parse_actions(&ini)
            .remove("CASE")
            .unwrap()
            .entries()[0]
            .clone();
        assert_eq!(json!(entry.kind), row["read"]["kind"]);
        assert_eq!(json!(entry.literal_value()), row["read"]["value"]);
        assert_eq!(json!(entry.waypoint_index), row["read"]["waypoint"]);
        assert_eq!(json!(entry.auxiliary_value()), row["read"]["auxiliary"]);
        if let Some(index) = entry.waypoint_index {
            if index < 702 {
                let (rx, ry) = cell(&input["waypoint_cell"]);
                let waypoint = Waypoint { index, rx, ry };
                sim.session
                    .install_waypoints(&[(index, waypoint)].into_iter().collect());
            }
        }
        let action = RuntimeDefinitions::default().resolve_action(entry, None, None);
        let context = TriggerActionContext {
            live_index: None,
            incoming_house: input["owner_present"].as_bool().unwrap().then_some(owner),
            input: TagEventInput::default(),
        };
        seeded_rngs(&mut sim, data);
        assert_eq!(rng_hashes(&sim), row["rng_before"], "{}", row["name"]);
        let mut effects = Vec::new();
        let (returned, draws) = crate::sim::rng::trace_draws(|| {
            sim.apply_live_trigger_action(
                &action,
                context,
                &mut effects,
                Some(&rules),
                None,
                FrameEffects::default(),
            )
        });
        assert_eq!(
            i32::from(returned),
            int(&row["returned"]),
            "{}",
            row["name"]
        );
        let mut output = state(&sim, owner, id);
        let other_state = state(&sim, other, id);
        output["other_preferred"] = other_state["preferred"].clone();
        output["other_custom_recharge"] = other_state["custom_recharge"].clone();
        assert_eq!(output, row["output"], "{}", row["name"]);
        assert_eq!(state(&sim, other, id), unrelated, "other House isolated");
        assert_eq!(rng_hashes(&sim), row["rng_after"], "{}", row["name"]);
        assert!(draws.is_empty() && effects.is_empty());
        assert!(row["rng_calls"].as_array().unwrap().is_empty());
    }
}

#[test]
fn retained_recharge_and_target_survive_snapshot_and_affect_future_state_hash() {
    use crate::sim::snapshot::GameSnapshot;
    let Some((rules, _)) = all03_inputs() else {
        return;
    };
    let mut sim = Simulation::with_seed(0);
    sim.intern_rule_type_ids(&rules);
    let owner = sim.interner.intern("YuriCountry");
    sim.houses
        .register(HouseState::new(owner, 2, None, false, 0, 10));
    crate::sim::superweapon::construct_house_supers(&mut sim, &rules, owner, false);
    let id = sim.interner.get("NukeSpecial").unwrap();
    let original = sim.state_hash();
    sim.super_weapons
        .get_mut(&owner)
        .unwrap()
        .get_mut(&id)
        .unwrap()
        .set_recharge_time(4500);
    let overridden = sim.state_hash();
    assert_ne!(overridden, original);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_preferred_super_weapon_target((126, 140));
    let expected = sim.state_hash();
    assert_ne!(expected, overridden);
    let saved = state(&sim, owner, id);
    let bytes = GameSnapshot::save(&sim, 0, 0, "ALL03UMD.MAP", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.state_hash(), expected);
    assert_eq!(state(&restored, owner, id), saved);
    restored
        .houses
        .get_mut(&owner)
        .unwrap()
        .clear_preferred_super_weapon_target();
    assert_eq!(restored.state_hash(), overridden);
    restored
        .super_weapons
        .get_mut(&owner)
        .unwrap()
        .get_mut(&id)
        .unwrap()
        .set_recharge_time(-1);
    assert_eq!(restored.state_hash(), original);
}

#[path = "trigger_runtime_superweapon_reset_tests.rs"]
mod reset_tests;
