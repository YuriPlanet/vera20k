//! Original non-undeploy completion callbacks, through the live sale owner.
//!
//! The original corpus stops immediately before BuildingLimbo. Rust continues
//! through its normal Limbo/UnInit tail, so this comparison consumes the
//! callback/action history, retained receiver payload and House arithmetic.
//! Native Alive/Tag attachment/pending flags at that earlier boundary are not
//! claimed to describe Rust's later teardown state. The separate retail sale
//! regression owns the normal Selling frames and final removal.
//! The physical-map definition control stops both implementations at its
//! first action entry; it establishes dispatch, not a completed stock sale.

use super::*;
use crate::map::map_file::MapFile;
use crate::map::resolved_terrain::test_flat_ground_grid;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{KillCallback, KillSource};
use crate::sim::house_state::HouseState;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::UninitContext;
use serde_json::{Value, json};

fn definitions(case: &Value) -> MapFile {
    let mut text = String::from(
        "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
         [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
         [Basic]\nNewINIFormat=4\n",
    );
    if let Some(sections) = case["input"]["definitions"].as_object() {
        for (section, entries) in sections {
            text.push_str(&format!("[{section}]\n"));
            for (key, value) in entries.as_object().unwrap() {
                text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
            }
        }
    }
    MapFile::from_bytes(text.as_bytes()).unwrap()
}

fn scene(case: &Value, corpus: &Value) -> (Simulation, RuleSet, u64) {
    let inputs = &case["native_inputs"];
    let typ = &inputs["type"];
    let type_name = typ["name"].as_str().unwrap();
    let flag = |key: &str| {
        if case["input"][key].as_bool().unwrap_or(false) {
            "yes"
        } else {
            "no"
        }
    };
    let mut rule_text = format!(
        "[General]\nRefundPercent=50%\n[Countries]\n0=Americans\n1=SaleNewCountry\n\
             [Animations]\n0=GAPOWR_A\n1=GAPOWR_AD\n\
             [BuildingTypes]\n0={type_name}\n[{type_name}]\nStrength={}\nCost={}\n\
             DontScore={}\nInsignificant={}\n",
        typ["strength"].as_i64().unwrap(),
        typ["cost"].as_i64().unwrap(),
        flag("dont_score"),
        flag("insignificant"),
    );
    for (name, country) in [("OLD", "Americans"), ("NEW", "SaleNewCountry")] {
        if let Some(house) = inputs["houses"].get(name) {
            rule_text.push_str(&format!("[{country}]\n"));
            for (key, value) in house["country_keys"].as_object().unwrap() {
                rule_text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
            }
        }
    }
    let mut art_text = String::new();
    for (section, entries) in corpus["art_input"].as_object().unwrap() {
        art_text.push_str(&format!("[{section}]\n"));
        for (key, value) in entries.as_object().unwrap() {
            art_text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
    }
    let art = IniFile::from_str(&art_text);
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(&rule_text),
        &art,
    )
    .unwrap();
    // The fixed-ART rules reader supplies type keys; the normal loader also
    // installs the runtime animation registry before binding SHP bounds.
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    for animation in corpus["native_anim_inputs"].as_array().unwrap() {
        rules.bind_anim_frame_count_for_test(
            animation["name"].as_str().unwrap(),
            animation["raw_shp_frame_count"].as_i64().unwrap() as i32,
        );
    }
    let mut sim = Simulation::new();
    sim.install_resolved_terrain_for_new_map(test_flat_ground_grid(32));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    sim.session.set_game_mode(
        crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
            inputs["game_mode"].as_i64().unwrap() != 0,
        ),
    );
    for (name, country) in [("OLD", "Americans"), ("NEW", "SaleNewCountry")] {
        let Some(prior) = case["before"]["houses"].get(name) else {
            continue;
        };
        let owner = sim.intern(name);
        let house_inputs = &inputs["houses"][name];
        assert_eq!(
            u64::from(rules.country_index(country).unwrap().0),
            house_inputs["country_index"].as_u64().unwrap()
        );
        let country = sim.intern(country);
        let mut house = HouseState::new(
            owner,
            0,
            Some(country),
            house_inputs["human"].as_bool().unwrap(),
            prior["credits"].as_i64().unwrap() as i32,
            10,
        );
        house.set_player_control_for_test(house_inputs["player_control"].as_bool().unwrap());
        house.project_country_mults(&rules, &sim.interner);
        let bits =
            house.cost_factors().country[rules.object(type_name).unwrap().factor_slot()].bits();
        let raw = bits
            .to_le_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            raw,
            house_inputs["country_cost_buildings_bits"]
                .as_str()
                .unwrap()
        );
        sim.houses.register(house);
    }
    let id = sim
        .spawn_object(type_name, "OLD", 10, 10, 0, &rules)
        .unwrap();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    let mission = if case["input"]["ordinary_record_kill"]
        .as_bool()
        .unwrap_or(false)
    {
        MissionType::Guard
    } else {
        MissionType::Selling
    };
    entity
        .mission
        .set_current_for_test(MissionId::from_known(mission), 2);
    assert_eq!(
        i64::from(entity.building_loss_index()),
        case["constructor_loss_field"].as_i64().unwrap()
    );
    assert_eq!(
        i64::from(entity.health.current()),
        case["before"]["health"].as_i64().unwrap()
    );
    assert_eq!(
        entity.building_art.actually_placed(),
        case["before"]["actually_placed"].as_bool().unwrap()
    );
    assert_eq!(
        entity.building_art.damage_state_active(),
        case["before"]["damaged_mode"].as_bool().unwrap()
    );
    let slot3 = entity.building_art.anim_slots()[3];
    let slot3 = slot3.map(|id| {
        sim.interner
            .resolve(sim.substrate.anims.get(id).unwrap().type_id())
    });
    assert_eq!(json!(slot3), case["before"]["slot3"]);
    let map = definitions(case);
    sim.initialize_trigger_definitions(&map, &Default::default(), None, None);
    sim.classify_map_triggers(&rules);
    if let Some(tag) = case["input"]["definitions"]["Tags"]
        .as_object()
        .and_then(|tags| tags.keys().next())
    {
        sim.attach_object_trigger_tag(id, Some(tag));
    }
    (sim, rules, id)
}

fn stable_state(sim: &Simulation, id: u64, expected: &Value) -> Value {
    let entity = sim
        .entities()
        .get(id)
        .expect("UnInit retains the receiver until pending deletion drains");
    let houses = expected["houses"].as_object().unwrap().keys().map(|name| {
        let owner = sim.interner.get(name).unwrap();
        let house = &sim.houses[&owner];
        (name.clone(), json!({"credits":house.economy().credits(), "losses":house.stats().buildings_lost(), "score":house.economy().score()}))
    }).collect::<serde_json::Map<_, _>>();
    let triggers = sim
        .trigger_runtime
        .triggers
        .iter()
        .flatten()
        .map(|trigger| json!({"enabled":trigger.enabled, "completed":trigger.completed}))
        .collect::<Vec<_>>();
    json!({"owner":sim.interner.resolve(entity.owner()),
        "loss_field":entity.building_loss_index(), "health":entity.health.current(),
        "globals":sim.trigger_runtime.globals_set, "houses":houses, "triggers":triggers})
}

fn expected_stable_state(state: &Value) -> Value {
    let triggers = state["triggers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|trigger| json!({"enabled":trigger["enabled"], "completed":trigger["completed"]}))
        .collect::<Vec<_>>();
    json!({"owner":state["owner"], "loss_field":state["loss_field"], "health":state["health"],
        "globals":state["globals"], "houses":state["houses"], "triggers":triggers})
}

#[test]
fn sale_completion_tag_callbacks_match_original_histories() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/building_sale_triggers.json");
    assert_eq!(corpus["schema_version"], 1);
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 9);
    let mut compared = 0;
    let mut native_only = 0;
    for case in cases {
        let name = case["input"]["name"].as_str().unwrap();
        assert_eq!(case["unchanged_executable_sections"], true, "{name}");
        assert_eq!(case["unchanged_vtables"], true, "{name}");
        match case["input"]["rust_comparison"].as_str().unwrap() {
            "sale_callbacks" | "before_authored_action" => compared += 1,
            "native_only_selling_death" => {
                // The original action32 row executes the complete fatal C4
                // composition (including debris/animation randomness). This
                // consumer has no matching full fatal scene and makes no
                // production mismatch or parity claim for that separate row.
                native_only += 1;
                continue;
            }
            other => panic!("unclassified native comparison scope {other}"),
        }
        let (mut sim, rules, id) = scene(case, corpus);
        assert_eq!(
            stable_state(&sim, id, &case["before"]),
            expected_stable_state(&case["before"]),
            "{name}: supplied boundary"
        );
        sim.trigger_runtime.dispatch_trace.clear();
        sim.trigger_runtime.action_trace.clear();
        let rng = (
            sim.scenario_rng.native_state_hex(),
            sim.main_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        );
        let identity = sim.native_identity_cursor().unwrap();
        if let Some(action) = case["input"]["stop_before_action"].as_i64() {
            sim.trigger_runtime.stop_before_action = Some(action as i32);
            let stopped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                crate::sim::production::sell_complete(
                    &mut sim,
                    &rules,
                    None,
                    id,
                    FrameEffects::default(),
                )
            }))
            .expect_err("both implementations stop before the selected action body");
            assert_eq!(
                stopped.downcast_ref::<i32>(),
                Some(&(action as i32)),
                "{name}: stop reason"
            );
            sim.trigger_runtime.stop_before_action = None;
        } else if case["input"]["ordinary_record_kill"]
            .as_bool()
            .unwrap_or(false)
        {
            sim.record_the_kill(
                id,
                KillSource::Techno(None),
                KillCallback::Terminal,
                UninitContext::with_rules(&rules),
            );
        } else {
            assert!(!crate::sim::production::sell_complete(
                &mut sim,
                &rules,
                None,
                id,
                FrameEffects::default()
            ));
        }
        assert_eq!(
            stable_state(&sim, id, &case["after"]),
            expected_stable_state(&case["after"]),
            "{name}: callback/accounting result"
        );
        let calls = case["calls"].as_array().unwrap();
        let events = calls
            .iter()
            .filter(|call| call["kind"] == "tag_event")
            .map(|call| {
                assert_eq!(call["source_null"], true, "{name}: native Tag source");
                call["event"].as_i64().unwrap() as i32
            })
            .collect::<Vec<_>>();
        assert_eq!(
            sim.trigger_runtime
                .dispatch_trace
                .iter()
                .map(|(_, event)| *event)
                .collect::<Vec<_>>(),
            events,
            "{name}: callback order"
        );
        let actions = calls
            .iter()
            .filter(|call| call["kind"] == "action")
            .map(|call| {
                (
                    call["action"].as_i64().unwrap() as i32,
                    call["value"].as_i64().unwrap() as i32,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            sim.trigger_runtime.action_trace, actions,
            "{name}: action order"
        );
        assert_eq!(case["rng_unchanged"], true, "{name}");
        assert_eq!(
            (
                sim.scenario_rng.native_state_hex(),
                sim.main_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex()
            ),
            rng,
            "{name}: all three RNG states"
        );
        assert_eq!(
            u64::from(sim.native_identity_cursor().unwrap() - identity),
            case["identity_delta"].as_u64().unwrap(),
            "{name}: object identities"
        );
    }
    assert_eq!((compared, native_only), (8, 1));
}
