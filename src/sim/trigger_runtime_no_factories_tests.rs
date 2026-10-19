//! Original HasOccurred17/Execute9 controls and their ordinary production chain.
use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::{
    combat::Health, game_entity::GameEntity, house_state::HouseState, intern::InternedId,
};
use serde_json::{Value, json};

fn corpus() -> &'static Value {
    let value = crate::test_fixture::json("tools/spatial_oracle/trigger_no_factories_events.json");
    assert_eq!(
        value["native_sha256"],
        "3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600"
    );
    value
}

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn fixture(countries: &[i32]) -> (Simulation, RuleSet, [InternedId; 3]) {
    let mut text = String::from("[Countries]\n");
    for index in 0..20 {
        text.push_str(&format!("{index}=Country{index}\n"));
    }
    for index in 0..20 {
        text.push_str(&format!("[Country{index}]\n"));
    }
    text.push_str("[BuildingTypes]\n");
    for (index, rtti) in [0, 1, 15, 16].into_iter().enumerate() {
        text.push_str(&format!("{index}=F{rtti}\n"));
    }
    text.push_str("[F0]\n[F1]\nFactory=Unit\n[F15]\nFactory=Infantry\n[F16]\nFactory=InfantryType\n[VehicleTypes]\n0=ACTOR\n[ACTOR]\nStrength=100\n");
    let rules = RuleSet::from_ini(&IniFile::from_str(&text)).unwrap();
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let houses = std::array::from_fn(|index| {
        let owner = sim.interner.intern(&format!("House{index}"));
        let country = sim.interner.intern(&format!("Country{}", countries[index]));
        sim.houses.insert(
            owner,
            HouseState::new(owner, index as u8, Some(country), false, 0, 10),
        );
        sim.session.house_order.push(owner);
        owner
    });
    (sim, rules, houses)
}

fn seed_native(sim: &mut Simulation, state: &Value) {
    use crate::sim::rng::SimRng;
    let restore = |stream: &str| {
        let record = state[stream].as_str().unwrap();
        if record.len() == 64 {
            // Predicate/Action9 record SHA256 of the seed31 state; the
            // Strategy controls preserve the full native bytes instead.
            SimRng::new(31)
        } else {
            SimRng::from_native_state_hex_for_test(record)
        }
    };
    sim.scenario_rng = restore("scenario");
    sim.main_rng = restore("main").into();
    sim.mapgen_rng = restore("mapgen");
    assert_rng_state(sim, state);
}

fn rng_state(sim: &Simulation) -> Value {
    json!({"scenario":sim.scenario_rng.native_state_hex(),
        "main":sim.main_rng.native_state_hex(),"mapgen":sim.mapgen_rng.native_state_hex()})
}

fn assert_rng_state(sim: &Simulation, expected: &Value) {
    let actual = rng_state(sim);
    for stream in ["scenario", "main", "mapgen"] {
        let hex = actual[stream].as_str().unwrap();
        if expected[stream].as_str().unwrap().len() == 64 {
            let bytes: Vec<_> = (0..hex.len())
                .step_by(2)
                .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16).unwrap())
                .collect();
            assert_eq!(crate::util::sha256::sha256_hex(&bytes), expected[stream]);
        } else {
            assert_eq!(actual[stream], expected[stream]);
        }
    }
}

#[test]
fn no_factories_event_matches_original_incoming_house_list_and_raw_class() {
    let corpus = corpus();
    let native = &corpus["predicates"];
    let controls = native["controls"].as_array().unwrap();
    let mut compared = 0;
    for case in controls {
        // A negative Factory RTTI is an injected native storage control, not
        // producible by ReadFactory's complete table. Do not alias it to a
        // different Rust input and claim a golden comparison.
        if case["house_priors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|house| {
                house["buildings"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|building| !building.is_null() && int(&building["factory_rtti"]) < 0)
            })
        {
            continue;
        }
        let (mut sim, rules, houses) = fixture(&[0, 7, 0]);
        for (index, house) in case["house_priors"].as_array().unwrap().iter().enumerate() {
            // Safe vectors store neither null entries nor negative counts.
            // Their admitted list is the same ordered nonnull native prefix.
            let count = int(&house["list_count"]).max(0) as usize;
            for building in house["buildings"].as_array().unwrap().iter().take(count) {
                if building.is_null() {
                    continue;
                }
                let id = sim.substrate.next_stable_object_id;
                sim.substrate.next_stable_object_id += 1;
                let typ = sim
                    .interner
                    .get(&format!("F{}", int(&building["factory_rtti"])))
                    .unwrap();
                let mut entity = GameEntity::new_at_frame_zero_for_test(
                    id,
                    10,
                    10,
                    0,
                    0,
                    houses[index],
                    Health::new(int(&building["health"])),
                    typ,
                    EntityCategory::Structure,
                    0,
                    0,
                    false,
                );
                entity
                    .lifecycle
                    .set_in_limbo_for_test(building["limbo"].as_bool().unwrap());
                entity
                    .lifecycle
                    .set_object_alive_for_test(building["ai_alive"].as_bool().unwrap());
                sim.substrate.entities.insert(entity);
                sim.append_house_base_building_for_test(id);
            }
            sim.houses
                .get_mut(&houses[index])
                .unwrap()
                .tracking
                .set_buildings_for_test(int(&house["tracked_buildings"]));
        }
        sim.session.house_order = case["house_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|slot| houses[slot.as_u64().unwrap() as usize])
            .collect();
        super::tests::install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nCHECK=Country0,<none>,Check,0,1,1,1,0\n[Events]\nCHECK=1,17,0,{}\n[Tags]\nTAG=2,Check,CHECK\n",
                int(&case["value"])
            ),
        );
        let live = sim.trigger_runtime.trigger_order[0];
        let typ = sim.trigger_runtime.trigger(live).unwrap().type_index;
        sim.trigger_runtime.definitions.trigger_types[typ].events[0].retained_house =
            Some(houses[2]);
        let timer = CdTimer::from_raw(int(&case["timer_before"][0]), int(&case["timer_before"][2]));
        sim.trigger_runtime.triggers[live].as_mut().unwrap().timer = timer;
        let actor = case["actor_owner"].as_u64().map(|slot| {
            let id = sim.substrate.next_stable_object_id;
            sim.substrate.next_stable_object_id += 1;
            let typ = sim.interner.get("ACTOR").unwrap();
            sim.substrate
                .entities
                .insert(GameEntity::new_at_frame_zero_for_test(
                    id,
                    10,
                    10,
                    0,
                    0,
                    houses[slot as usize],
                    Health::new(100),
                    typ,
                    EntityCategory::Unit,
                    0,
                    0,
                    false,
                ));
            id
        });
        seed_native(&mut sim, &native["rng_before"]);
        let mut persistence = int(&case["initial_persistence"]) != 0;
        let mut predicate = |sim: &mut Simulation| {
            sim.has_occurred(
                0,
                TagEventInput {
                    kind: int(&case["raised"]),
                    actor,
                    ..Default::default()
                },
                case["incoming_house"]
                    .as_u64()
                    .map(|slot| houses[slot as usize]),
                &mut persistence,
                Some(&rules),
                live,
            )
        };
        let actual = if case["scenario_init"].as_bool().unwrap() {
            sim.with_buildup_bypass(predicate)
        } else {
            predicate(&mut sim)
        };
        assert_eq!(actual, int(&case["result"]) != 0, "{case}");
        assert_eq!(persistence, int(&case["persistence"]) != 0, "{case}");
        assert_eq!(sim.trigger_runtime.trigger(live).unwrap().timer, timer);
        assert_eq!(
            sim.trigger_runtime.definitions.trigger_types[typ].events[0].retained_house,
            Some(houses[2])
        );
        assert_rng_state(&sim, &native["rng_after"]);
        compared += 1;
    }
    assert_eq!(controls.len(), 19);
    assert_eq!(compared, 18);
    assert_eq!(native["classification"]["state_a"], 0);
    assert_eq!(native["classification"]["state_b"], 1);
    assert_eq!(event_classification(17), 8);
}

#[test]
fn fire_sale_action_matches_original_selector_state_and_unchanged_timer() {
    let corpus = corpus();
    let controls = corpus["action9"]["controls"].as_array().unwrap();
    assert_eq!(controls.len(), 12);
    for case in controls {
        let (mut sim, rules, houses) = fixture(&[19, 7, 19]);
        sim.session.house_order = case["house_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|slot| houses[slot.as_u64().unwrap() as usize])
            .collect();
        for (slot, house) in case["player_at_slots"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            if let Some(index) = house.as_u64() {
                sim.session
                    .start_slot_houses
                    .insert(slot as u32, houses[index as usize]);
            }
        }
        super::tests::install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nCHECK=Country19,<none>,Check,0,1,1,1,0\n[Events]\nCHECK=1,8,0,0\n[Actions]\nCHECK={}\n[Tags]\nTAG=2,Check,CHECK\n",
                case["native_read"]["read"].as_str().unwrap()
            ),
        );
        let live = sim.trigger_runtime.trigger_order[0];
        let typ = sim.trigger_runtime.trigger(live).unwrap().type_index;
        sim.trigger_runtime.triggers[live]
            .as_mut()
            .unwrap()
            .retained_house = case["trigger_owner"]
            .as_u64()
            .map(|slot| houses[slot as usize]);
        for (index, owner) in houses.iter().enumerate() {
            let house = sim.houses.get_mut(owner).unwrap();
            house.strategy_emergency.mode = int(&case["modes_before"][index]);
            house.strategy_timer = CdTimer::from_raw(
                int(&case["strategy_timers_before"][index][0]),
                int(&case["strategy_timers_before"][index][2]),
            );
        }
        let before: Vec<_> = houses
            .iter()
            .map(|owner| serde_json::to_value(&sim.houses[owner]).unwrap())
            .collect();
        seed_native(&mut sim, &case["rng_before"]);
        let action = sim.trigger_runtime.definitions.trigger_types[typ].actions[0].clone();
        let actual = sim.apply_live_trigger_action(
            &action,
            TriggerActionContext {
                live_index: case["live_trigger"].as_bool().unwrap().then_some(live),
                incoming_house: case["incoming_house"]
                    .as_u64()
                    .map(|slot| houses[slot as usize]),
                input: TagEventInput::default(),
            },
            &mut Vec::new(),
            Some(&rules),
            None,
            FrameEffects::default(),
        );
        assert_eq!(actual, int(&case["result"]) != 0, "{case}");
        for (index, owner) in houses.iter().enumerate() {
            let mut expected = before[index].clone();
            expected["strategy_emergency"]["mode"] = case["modes_after"][index].clone();
            assert_eq!(
                serde_json::to_value(&sim.houses[owner]).unwrap(),
                expected,
                "{case}"
            );
        }
        assert_rng_state(&sim, &case["rng_after"]);
    }
}

#[test]
fn no_factories_retail_sov02_delay_reaches_existing_sale_and_hunt_after_restore() {
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    use crate::sim::{snapshot::GameSnapshot, world::UninitContext};
    let corpus = corpus();
    let retail = &corpus["retail"];
    let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let bytes = assets.get_ref(retail["map"].as_str().unwrap()).unwrap();
    assert_eq!(crate::util::sha256::sha256_hex(bytes), retail["map_sha256"]);
    let map = crate::map::map_file::MapFile::from_bytes(bytes).unwrap();
    let named = assets.resolve_ref("SOV02SMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "SOV02SMD.INI")
            .unwrap()
            .ini
    });
    let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (mut rules, _, fixed_art, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    crate::map::theater::load_theater(&mut assets, &map.header.theater);
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&fixed_art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&fixed_art),
    );
    rules.bind_building_buildup_assets(&assets, &map.header.theater);
    assert!(rules.has_buildup("GAPILL"));
    assert_eq!(rules.object("GAPILE").unwrap().factory_rtti(), 16);
    assert_eq!(rules.object("GAPILL").unwrap().factory_rtti(), 0);
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let (mut sim, _, _) = crate::sim::world::entry_test_fixture::fixture();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    sim.session.set_started(true);
    crate::sim::scenario_bootstrap::initialize_map_roster_houses(
        &mut sim,
        &roster,
        Some(&rules),
        Some(&map.ini),
    );
    let house_name = retail["house_name"].as_str().unwrap();
    let house = sim.interner.get(house_name).unwrap();
    assert_eq!(
        sim.house_by_country_index(crate::rules::ruleset::CountryIdx(19), &rules),
        Some(house)
    );
    let other = sim
        .session
        .house_order
        .iter()
        .copied()
        .find(|&id| id != house)
        .unwrap();
    // Crop only the two exact retail trigger definitions into the normal
    // Tag owner. The flat playfield and relocated ordinary objects make this
    // an integration regression, not a played or complete SOV02 comparison.
    let mut body = String::new();
    for section in ["Triggers", "Events", "Actions", "Tags"] {
        body.push_str(&format!("[{section}]\n"));
        for route in retail["routes"].as_array().unwrap() {
            let id = route["trigger_id"].as_str().unwrap();
            let value = match section {
                "Triggers" => route["trigger"].as_str().unwrap(),
                "Events" => route["raw_event"].as_str().unwrap(),
                "Actions" => route["raw_actions"].as_str().unwrap(),
                "Tags" => {
                    for (tag, raw) in route["tags"].as_object().unwrap() {
                        assert_eq!(
                            map.ini.section("Tags").unwrap().read_string(tag, "", 512),
                            raw.as_str().unwrap()
                        );
                        body.push_str(&format!("{tag}={}\n", raw.as_str().unwrap()));
                    }
                    continue;
                }
                _ => unreachable!(),
            };
            assert_eq!(
                map.ini.section(section).unwrap().read_string(id, "", 512),
                value
            );
            body.push_str(&format!("{id}={value}\n"));
        }
    }
    super::tests::install_event_ingress_map(&mut sim, &rules, &body);
    let factory = sim
        .spawn_object_at_height("GAPILE", house_name, 15, 15, 0, 0, &rules)
        .unwrap();
    let plain: Vec<_> = [(18, 15), (15, 18)]
        .into_iter()
        .map(|(x, y)| {
            sim.spawn_object_at_height("GAPILL", house_name, x, y, 0, 0, &rules)
                .unwrap()
        })
        .collect();
    let foot = sim
        .spawn_object_at_height("E1", house_name, 20, 20, 0, 0, &rules)
        .unwrap();
    let delayed_type = sim
        .trigger_runtime
        .definitions
        .trigger_type("043A9BBC")
        .unwrap();
    let delayed = *sim
        .trigger_runtime
        .trigger_order
        .iter()
        .find(|&&live| sim.trigger_runtime.trigger(live).unwrap().type_index == delayed_type)
        .unwrap();
    assert!(!sim.trigger_runtime.trigger(delayed).unwrap().enabled);
    sim.session.binary_frame = 8;
    sim.raise_house_tags(house, Some(&rules), None, FrameEffects::default());
    assert!(!sim.trigger_runtime.trigger(delayed).unwrap().enabled);
    // Both list and accounting move through the real ChangeOwner owner.
    sim.change_owner_with_rules(factory, other, &rules, None, FrameEffects::default());
    assert!(
        !sim.houses[&house]
            .base_projection
            .buildings()
            .contains(&factory)
    );
    sim.change_owner_with_rules(factory, house, &rules, None, FrameEffects::default());
    assert!(
        sim.houses[&house]
            .base_projection
            .buildings()
            .contains(&factory)
    );
    sim.session.binary_frame = 16;
    sim.raise_house_tags(house, Some(&rules), None, FrameEffects::default());
    assert!(!sim.trigger_runtime.trigger(delayed).unwrap().enabled);
    sim.techno_limbo_with_context(factory, UninitContext::new(Some(&rules), None));
    assert_eq!(sim.houses[&house].tracking.buildings(), 3);
    sim.session.binary_frame = 24;
    sim.raise_house_tags(house, Some(&rules), None, FrameEffects::default());
    assert!(sim.trigger_runtime.trigger(delayed).unwrap().enabled);
    assert_eq!(sim.houses[&house].strategy_emergency.mode(), 0);
    sim.houses.get_mut(&house).unwrap().strategy_timer = CdTimer::started(1000, 109);
    let saved = GameSnapshot::save(&sim, 0, 0, "SOV02SMD.MAP", 0);
    let mut sim = GameSnapshot::load(&saved).unwrap().sim;
    sim.restore_after_snapshot_load().unwrap();
    sim.resolve_type_handles(&rules);
    let timer = sim.houses[&house].strategy_timer;
    for (frame, mode) in [(698, 0), (700, 4)] {
        sim.session.binary_frame = frame;
        sim.advance_triggers(Some(&rules), None, FrameEffects::default());
        assert_eq!(sim.houses[&house].strategy_emergency.mode(), mode);
        assert_eq!(sim.houses[&house].strategy_timer, timer);
        assert!(
            plain
                .iter()
                .all(|&id| !sim.substrate.entities.get(id).unwrap().building_down())
        );
    }
    let native = &corpus["strategy"]["genuine_rng_strategy"];
    seed_native(&mut sim, &native["native_rng"]["before"]);
    sim.session.binary_frame = 1108;
    crate::sim::house_strategy::update_strategy(
        &mut sim,
        &rules,
        house,
        None,
        FrameEffects::default(),
    );
    assert_rng_state(&sim, &native["native_rng"]["before"]);
    assert!(!sim.houses[&house].strategy_emergency.all_to_hunt_bias());
    sim.session.binary_frame = 1109;
    let (_, draws) = crate::sim::rng::trace_draws(|| {
        crate::sim::house_strategy::update_strategy(
            &mut sim,
            &rules,
            house,
            None,
            FrameEffects::default(),
        )
    });
    assert_eq!(
        draws.len(),
        native["native_rng"]["raw_draw_count"].as_u64().unwrap() as usize
    );
    assert_rng_state(&sim, &native["native_rng"]["after"]);
    assert_eq!(
        sim.houses[&house].strategy_timer,
        CdTimer::started(1109, int(&native["delay"]))
    );
    assert!(sim.houses[&house].strategy_emergency.all_to_hunt_bias());
    assert!(
        plain
            .iter()
            .all(|&id| sim.substrate.entities.get(id).unwrap().building_down())
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(foot)
            .unwrap()
            .mission
            .queued()
            .known(),
        Some(crate::sim::mission::MissionType::Hunt)
    );
}
