//! Original Building Update / Grand_Opening / AnimClass joins, bounded by the
//! supplied prior actors and House state in building_construction_joined.meta.
//! Types, buildup controls and Anim bounds enter through the retail readers.

use super::*;
use crate::map::resolved_terrain::test_flat_ground_grid;
use crate::rules::ini_parser::IniFile;
use crate::sim::components::BuildingUp;
use crate::sim::house_state::HouseState;
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::radio::{RadioMessage, RadioPayload, transmit, transmit_to_contact};
use crate::sim::rng::{SimRng, trace_draws};
use crate::sim::world::PlacementEvidence;
use serde_json::{Value, json};

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_construction_joined.json",
    ))
    .unwrap()
}

fn rng_digest(rng: &SimRng) -> String {
    let hex = rng.native_state_hex();
    let bytes: Vec<_> = (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).unwrap())
        .collect();
    crate::util::sha256::sha256_hex(&bytes)
}

fn prior_building(sim: &mut Simulation, rules: &RuleSet, name: &str, x: u16, y: u16) -> u64 {
    let id = sim
        .spawn_object_limbo_at_height(name, "Americans", x, y, 0, 0, rules)
        .unwrap();
    assert!(
        sim.reveal_constructed_object_at_height(
            id,
            x,
            y,
            0,
            0,
            PlacementEvidence::EvaluateMark,
            rules,
        )
        .is_some()
    );
    id
}

fn joined_fixture(rules: &RuleSet, row: &Value) -> (Simulation, u64) {
    let input = &row["input"];
    let name = input["name"].as_str().unwrap();
    let route = input["route"].as_str().unwrap();
    let mut sim = Simulation::new();
    // The joined original fixture stores3 at GameOptionsClass0xA8EB60.
    sim.session.game_options.game_speed = 3;
    sim.install_resolved_terrain_for_new_map(test_flat_ground_grid(24));
    let owner = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner, 0, Some(owner), route == "player", 5000, 10);
    house.current_iq = 0;
    sim.houses.insert(owner, house);
    let id = prior_building(&mut sim, rules, name, 6, 9);
    let yard = prior_building(&mut sim, rules, "GACNST", 12, 12);
    {
        let entity = sim.substrate.entities.get_mut(yard).unwrap();
        entity.finish_building_construction_for_test();
        entity.building_actually_placed = true;
        entity.building_last_operational = true;
    }
    // The original corpus supplies these already admitted actors, then starts
    // its Scenario ID cursor and streams before the entry under comparison.
    // Constructor/Unlimbo draws and full map admission are outside this join.
    sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(0));
    let seed = input["seed"].as_u64().unwrap();
    sim.main_rng = SimRng::new(seed);
    sim.scenario_rng = SimRng::new(seed);
    sim.set_logic_order_for_test(vec![id]);
    assert_eq!(sim.main_rng.native_state_hex(), row["rng_before"]["main"]);
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        row["rng_before"]["scenario"]
    );
    if let Some(health) = input["health"].as_i64() {
        sim.substrate.entities.get_mut(id).unwrap().health.current = health as i32;
    }
    let control = rules.buildup_control(name);
    let entry = match route {
        "player" => BuildingUp::placed_by_player(control, 0),
        "computer" => BuildingUp::placed_by_computer(control, 0),
        "deploy" => BuildingUp::deployed(control, 0),
        _ => panic!("unknown native entry {route}"),
    };
    if route == "player" {
        transmit(
            &mut sim,
            yard,
            id,
            RadioMessage::Hello,
            RadioPayload::default(),
            Some(rules),
        );
    }
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .install_building_up(entry, 0);
    if route == "player" {
        transmit(
            &mut sim,
            id,
            yard,
            RadioMessage::DockArrived,
            RadioPayload::default(),
            Some(rules),
        );
        transmit_to_contact(&mut sim, yard, RadioMessage::Break, Some(rules));
    }
    (sim, id)
}

fn assert_joined_frame(sim: &Simulation, id: u64, frame: &Value, context: &str) {
    let entity = sim.substrate.entities.get(id).unwrap();
    let stage = entity.native_stage();
    let building = json!({
        "bstate": entity.building_body_state().unwrap(),
        "done": entity.building_ready_latch(),
        "mission": entity.mission.current().raw(),
        "queued": entity.mission.queued().raw(),
        "rate": stage.rate(),
        "stage": stage.value(),
        "status": entity.mission.handler_state(),
        "timer": [stage.timer().start_frame(), stage.timer().duration()],
    });
    assert_eq!(building, frame["building"], "{context}: building");
    assert_eq!(
        json!([
            entity.body_facing.destination(),
            entity.body_facing.start_word(),
        ]),
        frame["body_facing"],
        "{context}: both native FacingClass words"
    );
    assert_eq!(
        i64::from(entity.queued_building_body_state().unwrap()),
        frame["queued_bstate"].as_i64().unwrap(),
        "{context}: queued body"
    );
    assert_eq!(
        u64::from(entity.building_actually_placed),
        frame["placed"].as_u64().unwrap(),
        "{context}: placement"
    );
    assert_eq!(
        u64::from(entity.building_last_operational),
        frame["old_operational"].as_u64().unwrap(),
        "{context}: operational sample"
    );
    // The middle native dword is unused timer storage, not logical state.
    let timer = entity.mission.dispatch_timer();
    assert_eq!(
        [i64::from(timer.start_frame()), i64::from(timer.delay())],
        [
            frame["mission_timer"][0].as_i64().unwrap(),
            frame["mission_timer"][2].as_i64().unwrap()
        ],
        "{context}: mission timer"
    );
    if let Some(rng) = frame.get("rng") {
        assert_eq!(
            sim.main_rng.native_state_hex(),
            rng["main"],
            "{context}: Main RNG"
        );
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            rng["scenario"],
            "{context}: Scenario RNG"
        );
    } else {
        assert_eq!(
            rng_digest(&sim.main_rng),
            frame["rng_sha256"]["main"],
            "{context}: complete Main state digest"
        );
        assert_eq!(
            rng_digest(&sim.scenario_rng),
            frame["rng_sha256"]["scenario"],
            "{context}: complete Scenario state digest"
        );
    }
    let native_anims = frame["anims"].as_array().unwrap();
    let rust_anims: Vec<_> = sim.anims().map(|(_, anim)| anim).collect();
    assert_eq!(
        rust_anims.len(),
        native_anims.len(),
        "{context}: live animations"
    );
    for (anim, expected) in rust_anims.iter().zip(native_anims) {
        let runtime = &anim.runtime;
        assert_eq!(
            sim.interner.resolve(anim.type_id),
            expected["type_name"].as_str().unwrap(),
            "{context}: retained type identity"
        );
        assert_eq!(
            i64::from(anim.native_unique_id),
            expected["native_id"].as_i64().unwrap(),
            "{context}: native ID"
        );
        assert_eq!(
            u64::from(anim.draw_flags),
            expected["draw_flags"].as_u64().unwrap(),
            "{context}: draw flags"
        );
        assert_eq!(
            i64::from(anim.z_adjust),
            expected["z_adjust"].as_i64().unwrap(),
            "{context}: Z adjustment"
        );
        let coord = sim.anim_absolute_coord(anim.stable_id).unwrap();
        assert_eq!(
            json!([coord.x, coord.y, coord.z]),
            expected["location"],
            "{context}: world location"
        );
        assert!(
            anim.owner_entity.is_none(),
            "{context}: slot reference is distinct from Object owner"
        );
        assert_eq!(expected["owner_object"], 0);
        assert_eq!(expected["alive"], 1);
        assert_eq!(expected["limbo"], 0);
        assert_eq!(expected["is_bouncing"], 0);
        assert!(anim.bounce.is_none());
        assert_eq!(
            json!({
                "constructor_reverse": u8::from(runtime.constructor_reverse),
                "current_frame": runtime.current_frame,
                "delay_remaining": runtime.delay_remaining,
                "first_ai_guard": u8::from(runtime.first_ai_guard),
                "frame_step": runtime.frame_step,
                "frame_timer": [runtime.frame_timer.start_frame(), runtime.frame_timer.duration()],
                "inactive": u8::from(runtime.inactive),
                "loop_remaining": runtime.loop_remaining,
                "paused": u8::from(runtime.paused),
                "rate_reload": runtime.rate_reload,
            }),
            expected["runtime"],
            "{context}: Anim runtime"
        );
    }
    // Join native pointers through the independently compared Abstract IDs.
    let native_pointer = |rust_id| {
        if rust_id == id {
            return frame["logic"][0].as_u64().unwrap();
        }
        let anim = sim.anim(rust_id).unwrap();
        native_anims
            .iter()
            .find(|row| row["native_id"].as_i64() == Some(i64::from(anim.native_unique_id)))
            .unwrap()["pointer"]
            .as_u64()
            .unwrap()
    };
    let logic: Vec<_> = sim
        .live_object_order_snapshot()
        .into_iter()
        .map(native_pointer)
        .collect();
    assert_eq!(json!(logic), frame["logic"], "{context}: live order");
    let slots: Vec<_> = entity
        .building_anim_slots
        .iter()
        .map(|slot| slot.map_or(0, native_pointer))
        .collect();
    assert_eq!(json!(slots), frame["slots"], "{context}: retained slots");
    assert!(
        entity.radio_contacts.slot(0).is_none(),
        "{context}: contact cleanup"
    );
}

#[test]
fn stock_opening_matches_original_building_and_same_pass_anim_visits() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    assets.register_neutral_archives().unwrap();
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let corpus = corpus();
    let roots: Vec<String> = corpus["selected_animation_list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap().to_owned())
        .collect();
    let mut stock_rules = fixture.rules;
    stock_rules.bind_building_buildup_assets(&assets, "TEMPERATE");
    assert_eq!(
        stock_rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE"),
        0
    );
    for expected in corpus["native_type_inputs"].as_array().unwrap() {
        let name = expected["name"].as_str().unwrap();
        let object = stock_rules.object(name).unwrap();
        assert_eq!(
            i64::from(object.strength),
            expected["strength"].as_i64().unwrap(),
            "{name}: original full type reader"
        );
        assert_eq!(
            json!(stock_rules.buildup_control(name)),
            expected["buildup_control"],
            "{name}: actual SHP-derived control"
        );
        assert_eq!(u8::from(object.has_stupid_guard_mode), 1);
        assert!(object.buildup_sound.is_none());
        assert!(object.free_unit.is_none());
        assert_eq!(object.produce_cash_startup, 0);
        assert!(!object.refinery);
        assert_eq!(
            u8::from(object.helipad),
            expected["helipad"].as_u64().unwrap() as u8
        );
        assert_eq!(
            u64::from(object.deploy_facing),
            expected["deploy_facing"].as_u64().unwrap()
        );
        assert_eq!(
            u64::from(crate::rules::object_type::ObjectType::BUILDING_FACING),
            expected["building_facing"].as_u64().unwrap(),
            "{name}: separate constructor-only facing datum"
        );
        assert!(object.super_weapon.is_none() && object.super_weapon2.is_none());
        let entry = stock_rules
            .art()
            .resolve_metadata_entry(name, &object.image)
            .unwrap();
        for slot in expected["slots"].as_array().unwrap() {
            let index = slot["slot"].as_u64().unwrap() as u8;
            let config = entry
                .building_anims
                .iter()
                .find(|config| config.native_slot == index)
                .unwrap();
            assert_eq!(config.anim_type, slot["name"].as_str().unwrap());
            assert_eq!(
                u8::from(entry.building_anim_power[usize::from(index)].powered),
                slot["powered"].as_u64().unwrap() as u8
            );
        }
    }
    assert_eq!(
        stock_rules.general.construction_sound.as_deref(),
        Some("Dummy")
    );
    for expected in corpus["native_anim_inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| {
            roots
                .iter()
                .any(|name| Some(name.as_str()) == row["name"].as_str())
        })
    {
        let name = expected["name"].as_str().unwrap();
        let config = stock_rules.art().anim_runtime_config(name).unwrap();
        assert_eq!(
            [
                config.start,
                config.loop_start,
                config.loop_end,
                config.end,
                config.loop_count
            ],
            [
                expected["start"].as_i64().unwrap() as i32,
                expected["loop_start"].as_i64().unwrap() as i32,
                expected["loop_end"].as_i64().unwrap() as i32,
                expected["end"].as_i64().unwrap() as i32,
                expected["loop_count"].as_i64().unwrap() as i32
            ],
            "{name}: original normalized bounds"
        );
        assert_eq!(
            i64::from(config.rate_logic_frames),
            expected["rate"].as_i64().unwrap(),
            "{name}: original rate"
        );
        assert_eq!(
            config.raw_shp_frame_count,
            expected["raw_shp_frame_count"]
                .as_i64()
                .map(|count| count as i32),
            "{name}: retail bytes"
        );
    }
    let mut compared = 0;
    for row in corpus["routes"].as_array().unwrap() {
        let input = &row["input"];
        let control = input["clear_stupid_guard_control"].as_bool().unwrap();
        let control_rules = if control {
            let mut ini = fixture.processed_rules.clone();
            ini.merge(&IniFile::from_str("[GAPOWR]\nHasStupidGuardMode=no\n"));
            let mut rules =
                RuleSet::from_ini_with_fixed_art_for_test(&ini, &fixture.fixed_art).unwrap();
            let audio = crate::rules::audio_sources::AudioDefinitions::select(&assets);
            rules.bind_type_sound_references(&ini, audio.sounds());
            rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(
                &fixture.fixed_art,
            ));
            rules.bind_building_buildup_assets(&assets, "TEMPERATE");
            assert_eq!(
                rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE"),
                0
            );
            Some(rules)
        } else {
            None
        };
        let rules = control_rules.as_ref().unwrap_or(&stock_rules);
        let (mut sim, id) = joined_fixture(&rules, row);
        let context = format!("{} {} control={control}", input["name"], input["route"]);
        assert_joined_frame(&sim, id, &row["creation"], &format!("{context} creation"));
        for frame in row["frames"].as_array().unwrap() {
            let now = frame["frame"].as_u64().unwrap() as u32;
            sim.session.binary_frame = now;
            sim.sound_events.clear();
            let (_, draws) = trace_draws(|| {
                sim.for_each_live_object(|sim, stable_id| {
                    if sim.anim(stable_id).is_some() {
                        sim.visit_anim(stable_id, &rules, None);
                    } else {
                        // Execute the same bounded join as the native corpus.
                        // Its header/mission/body slices exclude damage fires,
                        // Techno common AI, repair and factory AI. Full object
                        // integration is checked by construction_tests and the
                        // separate normal-match production capture.
                        sim.visit_building_operational(stable_id, &rules, None);
                        update_animation(sim, stable_id, Some(&rules));
                        ready_commence(sim, stable_id, true);
                        dispatch(sim, stable_id, Some(&rules), ObjectAiCtx::default());
                        ready_commence(sim, stable_id, false);
                        apply_queued_body(sim, stable_id);
                    }
                })
            });
            let actual: Vec<_> = draws
                .iter()
                .map(|draw| draw["value"].as_u64().unwrap())
                .collect();
            let expected: Vec<_> = row["rng_advances"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|draw| draw["frame"].as_u64() == Some(u64::from(now)))
                .map(|draw| draw["raw"].as_u64().unwrap())
                .collect();
            assert_eq!(
                actual, expected,
                "{context} frame{now}: raw advances including rejected draws"
            );
            // Each native Update unconditionally attempts its loop sound;
            // the extra Play at status0 is this Construction sound producer.
            // Dummy has no samples, so its pool/device lifecycle is separately
            // bounded by stock_dummy_sound rather than stored in simulation.
            let events = frame["events"].as_array().unwrap();
            let native_starts = events
                .iter()
                .filter(|event| event["event"] == "sound_play")
                .count()
                - 1;
            let native_building = row["creation"]["logic"][0].as_u64().unwrap();
            let native_releases = events
                .iter()
                .filter(|event| {
                    event["event"] == "sound_release"
                        && event["this"].as_u64() == Some(native_building + 0x6a0)
                })
                .count();
            assert_eq!(sim.sound_events.iter().filter(|event| matches!(event, crate::sim::world::SimSoundEvent::ObjectSoundStarted { owner, .. } if *owner == id)).count(), native_starts, "{context} frame{now}: sound start boundary");
            assert_eq!(sim.sound_events.iter().filter(|event| matches!(event, crate::sim::world::SimSoundEvent::ObjectSoundReleased { owner } if *owner == id)).count(), native_releases, "{context} frame{now}: construction handle release");
            assert_joined_frame(&sim, id, frame, &format!("{context} frame{now}"));
            compared += 1;
        }
        assert_eq!(sim.main_rng.native_state_hex(), row["rng_after"]["main"]);
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"]["scenario"]
        );
    }
    assert!(
        compared > 250,
        "all six complete original routes are compared"
    );
}

#[test]
fn place_selects_the_original_house_order_and_primary_factory() {
    use crate::sim::mission::authority::{commence_entity_mission, queue_entity_mission_deferred};
    use crate::sim::production::{ProductionCategory, find_factory, initialize_factory_primary};

    let corpus = corpus();
    let controls = &corpus["factory_controls"];
    let fixture = |naval: bool| {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[Countries]\n0=Americans\n[Sides]\nGDI=Americans\n\
             [BuildingTypes]\n0=GACNST\n1=GAPOWR\n\
             [GACNST]\nFactory=BuildingType\nOwner=Americans\nNaval={naval}\n\
             [GAPOWR]\nOwner=Americans\n"
        )))
        .unwrap();
        let mut sim = Simulation::new();
        sim.install_resolved_terrain_for_new_map(test_flat_ground_grid(24));
        let owner = sim.interner.intern("Americans");
        sim.houses.insert(
            owner,
            HouseState::new(owner, 0, Some(owner), true, 5000, 10),
        );
        let ids = [
            prior_building(&mut sim, &rules, "GACNST", 6, 6),
            prior_building(&mut sim, &rules, "GACNST", 12, 12),
        ];
        for id in ids {
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .finish_building_construction_for_test();
            sim.production.clear_primary_factory(id);
        }
        (sim, rules, owner, ids)
    };
    for row in controls["find"].as_array().unwrap() {
        let (mut sim, rules, owner, ids) = fixture(row["naval"] == 1);
        for (index, id) in ids.into_iter().enumerate() {
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            // These controls supply flags and House insertion order. They
            // compare FindFactory, not the separate Limbo/sale producers.
            entity.lifecycle.in_limbo = row["limbo"][index] == 1;
            match row["selling"][index].as_u64().unwrap() {
                1 => {
                    queue_entity_mission_deferred(
                        entity,
                        MissionId::from_known(MissionType::Selling),
                    );
                    commence_entity_mission(entity, 0);
                }
                2 => {
                    queue_entity_mission_deferred(
                        entity,
                        MissionId::from_known(MissionType::Selling),
                    );
                }
                _ => {}
            }
            if row["primary"][index] == 1 {
                sim.production.set_primary_factory_for_test(
                    owner,
                    ProductionCategory::Building,
                    id,
                );
            }
        }
        let expected = row["order"]
            .as_array()
            .unwrap()
            .iter()
            .position(|pointer| pointer == &row["found"])
            .map(|index| ids[index]);
        assert_eq!(
            find_factory(
                &sim,
                &rules,
                owner,
                rules.object("GAPOWR").unwrap(),
                false,
                false,
                false
            ),
            expected,
            "{}",
            row["name"]
        );
    }
    let (mut sim, rules, owner, ids) = fixture(false);
    for (index, row) in controls["set_primary"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let before = sim
            .production
            .primary_factory(owner, ProductionCategory::Building);
        initialize_factory_primary(&mut sim, ids[index], &rules);
        let after = sim
            .production
            .primary_factory(owner, ProductionCategory::Building);
        assert_eq!(
            u8::from(before != after),
            row["changed"].as_u64().unwrap() as u8
        );
        assert_eq!(
            json!(ids.map(|id| u8::from(after == Some(id)))),
            row["primary"]
        );
    }
}
