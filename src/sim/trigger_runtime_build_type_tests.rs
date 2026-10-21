//! Native built-type predicates and connected production consumers. Supplied
//! House/type fields are explicit oracle priors; whole factory worlds are not
//! certified by a leaf comparison.

use super::*;
use crate::rules::object_type::ObjectCategory;
use crate::sim::rng::SimRng;
use serde_json::Value;

fn corpus() -> &'static Value {
    let result = crate::test_fixture::json("tools/spatial_oracle/trigger_build_type_events.json");
    result
}

fn integer(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn categories() -> [(ObjectCategory, &'static str); 4] {
    [
        (ObjectCategory::Building, "building"),
        (ObjectCategory::Infantry, "infantry"),
        (ObjectCategory::Vehicle, "vehicle"),
        (ObjectCategory::Aircraft, "aircraft"),
    ]
}

fn supply_rng(sim: &mut Simulation, states: &Value) {
    sim.scenario_rng = SimRng::from_native_state_hex_for_test(states["scenario"].as_str().unwrap());
    sim.main_rng = SimRng::from_native_state_hex_for_test(states["main"].as_str().unwrap()).into();
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(states["mapgen"].as_str().unwrap());
}

fn assert_rng(sim: &Simulation, states: &Value) {
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        states["scenario"].as_str().unwrap()
    );
    assert_eq!(
        sim.main_rng.native_state_hex(),
        states["main"].as_str().unwrap()
    );
    assert_eq!(
        sim.mapgen_rng.native_state_hex(),
        states["mapgen"].as_str().unwrap()
    );
}

#[test]
fn build_type_events_match_native_house_selection_persistence_and_rng() {
    use super::tests::{
        event_ingress_fixture, install_event_ingress_map, spawn_event_ingress_unit,
    };
    let native = corpus();
    assert_eq!(native["country_indices"], serde_json::json!([0, 7, 0]));
    assert_eq!(native["controls"].as_array().unwrap().len(), 28);
    for case in native["controls"].as_array().unwrap() {
        let (mut sim, rules, _, houses) = event_ingress_fixture();
        for (house, fields) in houses.iter().zip(case["last_built"].as_array().unwrap()) {
            for (category, name) in categories() {
                sim.houses
                    .get_mut(house)
                    .unwrap()
                    .record_built_type(category, integer(&fields[name]));
            }
        }
        let actor = case["actor_owner"].as_u64().map(|index| {
            spawn_event_ingress_unit(&mut sim, &rules, houses[index as usize], (15, 15))
        });
        install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nBUILT=Country0,<none>,Built,0,1,1,1,0\n\
                 [Events]\nBUILT=1,{},0,{}\n[Tags]\nTAG=2,Built,BUILT\n",
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
        supply_rng(&mut sim, &native["predicate_rng"]["full_seed31"]);
        let mut persistence = integer(&case["initial_persistence"]) != 0;
        let mut evaluate = |sim: &mut Simulation| {
            sim.has_occurred(
                0,
                TagEventInput {
                    kind: integer(&case["raised"]),
                    actor,
                    ..Default::default()
                },
                case["incoming_house"].as_u64().map(|i| houses[i as usize]),
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
        assert_eq!(case["timer_after"], case["timer_before"]);
        assert_eq!(sim.trigger_runtime.trigger(live).unwrap().timer, timer);
        assert_eq!(
            sim.trigger_runtime.definitions.trigger_types[trigger_type].events[0].retained_house,
            Some(houses[2])
        );
        assert_rng(&sim, &native["predicate_rng"]["full_after"]);
    }
    for row in native["classification"].as_array().unwrap() {
        assert_eq!(row["state_a"], 1);
        assert_eq!(row["state_b"], 1);
        assert_eq!(event_classification(integer(&row["kind"])), 8);
    }
}

#[test]
fn build_type_state_uses_native_constructor_values_and_survives_snapshot_hash() {
    use crate::sim::house_state::HouseState;
    use crate::sim::snapshot::GameSnapshot;
    let native = corpus();
    let mut sim = Simulation::with_seed(0);
    let owner = sim.interner.intern("Owner");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    for (category, name) in categories() {
        let expected = integer(&native["constructor"]["last_built"][name]);
        assert_eq!(sim.houses[&owner].last_built_type_index(category), expected);
        assert_eq!(
            HouseState::default().last_built_type_index(category),
            expected
        );
    }
    let prior = sim.state_hash();
    let fields = &native["controls"][0]["last_built"][0];
    for (category, name) in categories() {
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .record_built_type(category, integer(&fields[name]));
    }
    let changed = sim.state_hash();
    assert_ne!(prior, changed);
    let saved = GameSnapshot::save(&sim, 0, 0, "built-types", 0);
    let mut restored = GameSnapshot::load(&saved).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.state_hash(), changed);
    for (category, name) in categories() {
        assert_eq!(
            restored.houses[&owner].last_built_type_index(category),
            integer(&fields[name])
        );
        restored
            .houses
            .get_mut(&owner)
            .unwrap()
            .record_built_type(category, -1);
        assert_ne!(
            restored.state_hash(),
            changed,
            "{category:?} must affect the hash"
        );
        restored
            .houses
            .get_mut(&owner)
            .unwrap()
            .record_built_type(category, integer(&fields[name]));
        assert_eq!(restored.state_hash(), changed);
    }
}

#[test]
fn build_type_event_latches_before_elapsed_time_as_in_native_register_history() {
    use super::tests::{event_ingress_fixture, install_event_ingress_map};
    let native = corpus();
    let (mut sim, rules, _, houses) = event_ingress_fixture();
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nBUILT=Country0,<none>,Built,0,1,1,1,0\n\
         [Events]\nBUILT=2,13,0,10,19,0,7\n[Tags]\nTAG=0,Built,BUILT\n",
    );
    let tag_type = sim.trigger_runtime.definitions.tag_type("TAG").unwrap();
    let tag = sim.shared_trigger_tag(tag_type);
    let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
    let typ = sim.trigger_runtime.trigger(live).unwrap().type_index;
    assert_eq!(
        sim.trigger_runtime.definitions.trigger_types[typ].events[0].kind,
        19
    );
    sim.trigger_runtime.definitions.trigger_types[typ].events[0].retained_house = Some(houses[0]);
    sim.trigger_runtime.triggers[live]
        .as_mut()
        .unwrap()
        .retained_house = Some(houses[0]);
    let histories = native["register_history"].as_array().unwrap();
    let prior = &histories[0]["prior"]["timer"];
    sim.trigger_runtime.triggers[live].as_mut().unwrap().timer =
        CdTimer::from_raw(integer(&prior[0]), integer(&prior[2]));
    supply_rng(&mut sim, &native["predicate_rng"]["full_seed31"]);
    for row in histories {
        sim.session
            .set_binary_frame_for_test(integer(&row["frame"]) as u32);
        sim.houses
            .get_mut(&houses[0])
            .unwrap()
            .record_built_type(ObjectCategory::Building, integer(&row["last_built_value"]));
        let result = sim.register_live_trigger_event(
            live,
            TagEventInput {
                kind: integer(&row["raised"]),
                ..Default::default()
            },
            row["repeat"].as_bool().unwrap(),
            &rules,
        );
        let state = sim.trigger_runtime.trigger(live).unwrap();
        assert_eq!(result, integer(&row["result"]) != 0, "{row}");
        assert_eq!(state.completed, integer(&row["completed"]) as u32);
        assert_eq!(
            state.retained_house,
            Some(houses[integer(&row["house"]) as usize])
        );
        assert_eq!(state.timer.start_frame(), integer(&row["timer"][0]));
        assert_eq!(state.timer.duration(), integer(&row["timer"][2]));
        assert_rng(&sim, &native["predicate_rng"]["full_after"]);
    }
}

#[test]
fn building_discovery_updates_last_type_only_after_admission_outside_init() {
    use crate::map::entities::EntityCategory;
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::{game_entity::GameEntity, house_state::HouseState, world::UninitContext};
    let native = corpus();
    let source = &native["discovery"];
    let index = integer(&source["input"]["type_df8"]);
    // Supplied registry/actor priors correspond to the oracle's Type+DF8=73.
    // This checks the additional store, not full Building construction.
    let mut body = String::from("[BuildingTypes]\n");
    for i in 0..=index {
        body.push_str(&format!("{i}=B{i}\n"));
    }
    let name = format!("B{index}");
    body.push_str(&format!("[{name}]\nStrength=100\nFoundation=1x1\n"));
    let rules = RuleSet::from_ini(&IniFile::from_str(&body)).unwrap();
    assert_eq!(rules.building_type_index(&name), Some(index));
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(&rules);
    let owner = sim.interner.intern("Owner");
    let player = sim.interner.intern("Player");
    sim.session.set_current_house_for_test(Some(player));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    sim.houses
        .register(HouseState::new(owner, 0, None, false, 0, 10));
    sim.houses
        .register(HouseState::new(player, 1, None, true, 0, 10));
    sim.houses.get_mut(&owner).unwrap().record_built_type(
        ObjectCategory::Building,
        integer(&source["input"]["initial_house26c"]),
    );
    let id = sim.substrate.allocate_stable_id();
    let mut entity =
        GameEntity::test_default_of_category(id, &name, "Owner", 10, 10, EntityCategory::Structure);
    entity.set_type_ref_for_test(sim.interner.intern(&name));
    entity.set_owner_for_test(owner);
    entity.building_art.set_actually_placed_for_test(true);
    sim.substrate.entities.insert(entity);
    sim.substrate
        .mission_assign_exact(
            id,
            crate::sim::mission::MissionId::from_raw(integer(&source["input"]["mission"])),
            0,
        )
        .unwrap();
    let built_before = sim.houses[&owner].stats().built();
    for row in source["histories"].as_array().unwrap() {
        let viewer = if row["viewer_current"].as_bool().unwrap() {
            player
        } else {
            owner
        };
        let context = UninitContext::new(&rules, None);
        let returned = if integer(&row["scenario_init"]) == 2 {
            sim.with_object_placement_scope(|sim| {
                sim.with_object_placement_scope(|sim| sim.discover_techno(id, viewer, context))
            })
        } else {
            sim.discover_techno(id, viewer, context)
        };
        assert_eq!(returned, integer(&row["returned"]) != 0, "{row}");
        assert_eq!(
            sim.houses[&owner].last_built_type_index(ObjectCategory::Building),
            integer(&row["owner"]["type_df8_copy"])
        );
        assert_eq!(
            sim.houses[&owner].stats().built(),
            built_before,
            "discovery never scores production"
        );
    }
}

#[test]
fn build_type_retail_all07_placement_reaches_house_poll_and_eva_once() {
    use crate::map::map_file::MapFile;
    use crate::rules::{
        ini_parser::IniFile, process_owner::NativeScenarioRulesPrefix, sound_ini::EvaRegistry,
    };
    use crate::sim::production::{self, ProductionPlacement};
    let native = corpus();
    let record = &native["retail"];
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let raw = assets.get_ref("ALL07SMD.MAP").unwrap();
    assert_eq!(crate::util::sha256::sha256_hex(raw), record["map_sha256"]);
    let map = MapFile::from_bytes(raw).unwrap();
    let named = assets.resolve_ref("ALL07SMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL07SMD.INI")
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
    let expected_index = integer(&record["selected_index7"]["stored_index"]);
    assert_eq!(rules.building_type_index("GAWEAP"), Some(expected_index));
    let trigger_id = record["trigger_id"].as_str().unwrap();
    let tag_id = record["tag_id"].as_str().unwrap();
    let mut selected = String::new();
    for (section, key, field) in [
        ("Triggers", trigger_id, "trigger"),
        ("Events", trigger_id, "raw_event"),
        ("Actions", trigger_id, "actions"),
        ("Tags", tag_id, "tag"),
    ] {
        let value = map.ini.section(section).unwrap().get_for_test(key).unwrap();
        assert_eq!(value, record[field].as_str().unwrap());
        selected.push_str(&format!("[{section}]\n{key}={value}\n"));
    }
    let mut sim = Simulation::new();
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    crate::sim::scenario_bootstrap::initialize_map_roster_houses(
        &mut sim,
        &roster,
        &rules,
        Some(&map.ini),
    );
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let house = sim.interner.get("Americans").unwrap();
    sim.session.set_current_house_for_test(Some(house));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    // The selected retail trigger/rules/roster are retained; geometry and a
    // preplaced yard plus completed factory product are explicit prerequisites.
    sim.with_object_placement_scope(|sim| {
        sim.spawn_object_at_height("GACNST", "Americans", 18, 18, 0, 0, &rules)
            .unwrap()
    });
    let eva =
        EvaRegistry::from_ini(&IniFile::from_bytes(assets.get_ref("EVAMD.INI").unwrap()).unwrap());
    let cue = eva.index_of("Mis_xA7_EvaUnholyAlliance").unwrap();
    let excerpt = MapFile::from_bytes(
        format!(
            "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
         [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n{selected}"
        )
        .as_bytes(),
    )
    .unwrap();
    // Bind the original EVA operand through the ordinary definition reader.
    sim.initialize_trigger_definitions(&excerpt, &Default::default(), Some(&eva), None);
    sim.classify_map_triggers(&rules);
    let tag_type = sim.trigger_runtime.definitions.tag_type(tag_id).unwrap();
    let tag = sim.shared_trigger_tag(tag_type);
    let previous = sim.houses[&house].last_built_type_index(ObjectCategory::Building);
    assert_ne!(previous, expected_index);
    sim.raise_house_tags(house, &rules, None, FrameEffects::default());
    let product = sim.interner.get("GAWEAP").unwrap();
    production::complete_held_building_for_test(&mut sim, &rules, house, product);
    assert_eq!(
        sim.houses[&house].last_built_type_index(ObjectCategory::Building),
        previous,
        "completion held in factory is not placement"
    );
    assert!(!production::place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAWEAP",
            cell: (18, 18)
        },
        None,
        FrameEffects::default()
    ));
    assert_eq!(
        sim.houses[&house].last_built_type_index(ObjectCategory::Building),
        previous,
        "refused placement must not record the type"
    );
    assert!(production::place_production_with_overlays(
        &mut sim,
        &rules,
        "Americans",
        ProductionPlacement::Building {
            type_id: "GAWEAP",
            cell: (23, 19)
        },
        None,
        FrameEffects::default()
    ));
    assert_eq!(
        sim.houses[&house].last_built_type_index(ObjectCategory::Building),
        expected_index
    );
    for (frame, heard) in [(8, false), (149, false), (152, true), (160, true)] {
        sim.session.set_binary_frame_for_test(frame);
        sim.raise_house_tags(house, &rules, None, FrameEffects::default());
        sim.flush_pending_delete();
        assert_eq!(
            sim.sound_events
                .iter()
                .filter(|e| matches!(e, SimSoundEvent::TriggerEva { index } if *index == cue))
                .count(),
            usize::from(heard),
            "frame{frame}"
        );
    }
    assert!(sim.trigger_runtime.tag(tag).is_none());
    assert_eq!(
        sim.trigger_runtime
            .action_trace
            .iter()
            .filter(|(kind, _)| *kind == 21)
            .count(),
        1
    );
}
