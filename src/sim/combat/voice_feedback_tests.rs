//! Original result-2 VoiceFeedback702695..7027F7 arithmetic/admission controls.
//! The native fixture supplies entry to that result arm. Rust reaches it through
//! the existing health receiver with an authored 100->40 hit; the neighboring
//! combat test retains non-crossing/red/dead/empty-list admission contrasts.
//! Stock E1 controls additionally execute the complete native Infantry/Foot/
//! Techno/Object receiver before its separate audio-service boundary.

use super::*;
use crate::rules::sound_ini::SoundRegistry;
use crate::sim::components::DriveCoord;
use crate::sim::world::Simulation;
use serde_json::Value;

fn boundary<'a>(row: &'a Value, phase: &str, stream: &str) -> &'a str {
    let key = row[phase]["rng"][stream].as_str().unwrap();
    row["complete_rng_states"][key]["bytes"].as_str().unwrap()
}

/// Actual125-HP E1, original517FA0→4D7330→701900→5F5390 and fixed retail
/// sound binding. Four supplied direct hits are not a FireAt/Logic schedule.
#[test]
#[ignore = "slow; requires retail Battle rules and fixed sound catalog"]
fn stock_e1_feedback_matches_whole_native_receiver_before_audio_service() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let native = crate::test_fixture::json("tools/input_oracle/stock_e1_feedback.json");
    let rules = &retail.rules;
    let e1 = rules.object("E1").unwrap();
    let mtnk = rules.object("MTNK").unwrap();
    assert_eq!(i64::from(e1.strength), native["retail"]["e1"]["strength"]);
    assert_eq!(
        serde_json::json!(e1.voice_feedback),
        native["retail"]["e1"]["voice_feedback"]["names"]
    );
    assert_eq!(mtnk.primary(), native["retail"]["mtnk"]["primary"].as_str());
    let weapon = rules.weapon(mtnk.primary().unwrap()).unwrap();
    assert_eq!(
        i64::from(weapon.damage),
        native["retail"]["weapon"]["damage"]
    );
    assert_eq!(
        weapon.warhead.as_deref(),
        native["retail"]["weapon"]["warhead"].as_str()
    );
    let rng_state = |hit: &Value, phase: &str, stream: &str| {
        let key = hit[phase][stream].as_str().unwrap();
        native["complete_rng_states"][key]["bytes"]
            .as_str()
            .unwrap()
    };
    let rows = native["rows"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        3,
        "admission, rejection and live unbound removal"
    );
    for row in rows {
        let label = row["name"].as_str().unwrap();
        let hits = row["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 4, "{label}: complete receiver visits");
        let coordinate: [i32; 3] = std::array::from_fn(|i| {
            i32::try_from(hits[0]["coordinate"][i].as_i64().unwrap()).unwrap()
        });
        let cell = [
            u16::try_from(coordinate[0] >> 8).unwrap(),
            u16::try_from(coordinate[1] >> 8).unwrap(),
        ];
        let mut sim = Simulation::new();
        sim.fog.width = 128;
        sim.fog.height = 128;
        sim.intern_rule_type_ids(rules);
        let owner = sim.interner.intern("Americans");
        let mut house = HouseState::new(owner, 0, None, true, 0, 10);
        house.player_control = true;
        sim.houses.insert(owner, house);
        sim.session.current_house = Some(owner);
        let source = sim
            .spawn_object_at_height("MTNK", "Americans", cell[0] - 1, cell[1], 0, 0, rules)
            .expect("ordinary stock source");
        let target = sim
            .spawn_object_at_height("E1", "Americans", cell[0], cell[1], 0, 0, rules)
            .expect("ordinary stock receiver");
        // The comparison supplies native GetCoords, not a fitted screen point.
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .position
            .put_location(DriveCoord {
                x: coordinate[0],
                y: coordinate[1],
                z: coordinate[2],
            });
        let warhead = sim.interner.intern(weapon.warhead.as_deref().unwrap());
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(rng_state(&hits[0], "rng_before", "main"))
                .into();
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(rng_state(&hits[0], "rng_before", "scenario"));
        sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(rng_state(&hits[0], "rng_before", "mapgen"));
        for (index, hit) in hits.iter().enumerate() {
            let phase = format!("{label}/receiver_{index}");
            for (stream, actual) in [
                ("main", sim.main_rng.native_state_hex()),
                ("scenario", sim.scenario_rng.native_state_hex()),
                ("mapgen", sim.mapgen_rng.native_state_hex()),
            ] {
                assert_eq!(
                    actual,
                    rng_state(hit, "rng_before", stream),
                    "{phase}: before {stream}"
                );
            }
            assert_eq!(
                i64::from(sim.entities().get(target).unwrap().health.current()),
                hit["health_before"],
                "{phase}: no authored HP or per-hit reseed"
            );
            let ((result, _), draws) = crate::sim::rng::trace_draws(|| {
                world_receiver::commit_entities(
                    &mut sim,
                    &mut world_receiver::ReceiverRun::default(),
                    &[EntityDamageEvent::area(
                        target,
                        weapon.damage,
                        0,
                        source,
                        None,
                        warhead,
                    )],
                    None,
                    rules,
                    None,
                )
            });
            assert!(
                result.despawned_ids.is_empty(),
                "{phase}: surviving receiver"
            );
            let actor = sim.entities().get(target).unwrap();
            assert_eq!(
                i64::from(actor.health.current()),
                hit["health_after"],
                "{phase}: HP"
            );
            assert_eq!(
                crate::sim::infantry::is_prone_for_damage(actor),
                hit["prone_after"].as_u64().unwrap() != 0,
                "{phase}: prone owner"
            );
            for (stream, actual) in [
                ("main", sim.main_rng.native_state_hex()),
                ("scenario", sim.scenario_rng.native_state_hex()),
                ("mapgen", sim.mapgen_rng.native_state_hex()),
            ] {
                assert_eq!(
                    actual,
                    rng_state(hit, "rng_after", stream),
                    "{phase}: after {stream}"
                );
            }
            let mut expected_draws: Vec<_> = hit["ranged_raw_advances"]
                .as_array()
                .unwrap()
                .iter()
                .map(|draw| draw["raw"].as_u64().unwrap())
                .collect();
            expected_draws.extend(
                hit["requests"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|request| request["args"].as_array().unwrap().is_empty())
                    .map(|request| request["result"].as_u64().unwrap()),
            );
            assert_eq!(
                draws
                    .iter()
                    .map(|draw| draw["value"].as_u64().unwrap())
                    .collect::<Vec<_>>(),
                expected_draws,
                "{phase}: raw draw count/order"
            );
            let expected_calls = hit["calls"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|call| call["kind"] == "PlayAtPosition")
                .count();
            let requests: Vec<_> = sim
                .sound_events
                .iter()
                .filter_map(|event| match event {
                    SimSoundEvent::VocAt {
                        sound_id,
                        audible_to: None,
                        rx,
                        ry,
                        sub_x,
                        sub_y,
                        world_z_leptons,
                    } if e1.voice_feedback.iter().any(|name| name == sound_id) => Some((
                        sound_id,
                        [
                            i32::from(*rx) * 256 + sub_x.to_num::<i32>(),
                            i32::from(*ry) * 256 + sub_y.to_num::<i32>(),
                            *world_z_leptons,
                        ],
                    )),
                    _ => None,
                })
                .collect();
            assert_eq!(
                requests.len(),
                expected_calls,
                "{phase}: unbound feedback admission"
            );
            for (_, actual) in requests {
                assert_eq!(actual, coordinate, "{phase}: native GetCoords");
            }
        }
    }
}

#[test]
fn damage_feedback_matches_original_draws_house_admission_coordinates_and_continuation() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/unit_voice_playback.json",
    ))
    .unwrap();
    let rows = corpus["feedback_cases"].as_array().unwrap();
    assert_eq!(rows.len(), 10, "all executed selected-arm controls");
    let mut sound_ini = String::from("[SoundList]\n");
    for (index, sound) in corpus["retail"]["sound_sections"]["SoundList"]
        .as_object()
        .unwrap()
    {
        sound_ini.push_str(&format!("{index}={}\n", sound.as_str().unwrap()));
    }
    let sounds = SoundRegistry::from_ini(&IniFile::from_str(&sound_ini));
    for row in rows {
        let label = row["name"].as_str().unwrap();
        let input = &row["input"];
        let names: Vec<&str> = input["voice_feedback"]["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect();
        // This is a bounded result-2 receiver fixture, not a claim that its
        // authored health/warhead configuration is the physical E1 rules.
        let ini = IniFile::from_str(&format!(
            "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\nArmor=none\nVoiceFeedback={}\n\
             [AudioVisual]\nConditionRed=25%\n[Warheads]\n0=FeedbackWH\n\
             [FeedbackWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            names.join(","),
        ));
        let mut rules = RuleSet::from_ini(&ini).unwrap();
        rules.bind_type_sound_references(&ini, &sounds);
        assert_eq!(
            rules.object("E1").unwrap().voice_feedback,
            names,
            "{label}: reader input"
        );
        let mut target = GameEntity::test_default(1, "E1", "FeedbackHouse", 13, 14);
        target.set_category_for_test(EntityCategory::Infantry);
        target.health.set_for_test(100);
        target.lifecycle.set_in_limbo_for_test(false);
        target.lifecycle.set_cell_marked(true);
        target.set_in_playfield(true);
        let coordinate = &input["coordinate"];
        target.position.put_location(DriveCoord {
            x: coordinate[0].as_i64().unwrap() as i32,
            y: coordinate[1].as_i64().unwrap() as i32,
            z: coordinate[2].as_i64().unwrap() as i32,
        });
        let mut sim = Simulation::new();
        sim.interner = test_interner();
        let owner = target.owner();
        let other = sim.interner.intern("OtherHouse");
        let mut house = HouseState::new(owner, 0, None, input["human"].as_bool().unwrap(), 0, 10);
        house.player_control = input["player_control"].as_bool().unwrap();
        sim.houses.insert(owner, house);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
                input["game_mode"].as_u64().unwrap() != 0,
            ),
        );
        sim.session.current_house = Some(if input["current_house_equal"].as_bool().unwrap() {
            owner
        } else {
            other
        });
        sim.substrate.entities.insert(target);
        let warhead = sim.interner.intern("FeedbackWH");
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(boundary(row, "before", "main")).into();
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(boundary(row, "before", "scenario"));
        sim.mapgen_rng = SimRng::from_native_state_hex_for_test(boundary(row, "before", "mapgen"));
        let (result, actual_draws) = crate::sim::rng::trace_draws(|| {
            world_receiver::commit_entities(
                &mut sim,
                &mut world_receiver::ReceiverRun::default(),
                &[EntityDamageEvent::area(
                    1,
                    60,
                    0,
                    RAD_NO_ATTACKER,
                    None,
                    warhead,
                )],
                None,
                &rules,
                None,
            )
        });
        assert!(
            result.0.despawned_ids.is_empty(),
            "{label}: surviving result fixture"
        );
        assert_eq!(
            sim.entities().get(1).unwrap().health.current(),
            40,
            "{label}: health-result admission"
        );
        // The native block has one ranged request then at most one raw pick.
        // Use its actual rejected/accepted raw words, never calculate goldens.
        let mut expected_draws: Vec<u64> = row["advances"]
            .as_array()
            .unwrap()
            .iter()
            .map(|draw| draw["raw"].as_u64().unwrap())
            .collect();
        expected_draws.extend(
            row["requests"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|draw| draw["args"].as_array().unwrap().is_empty())
                .map(|draw| draw["result"].as_u64().unwrap()),
        );
        assert_eq!(
            actual_draws
                .iter()
                .map(|draw| draw["value"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            expected_draws,
            "{label}: raw Main order"
        );
        for (stream, actual) in [
            ("main", sim.main_rng.native_state_hex()),
            ("scenario", sim.scenario_rng.native_state_hex()),
            ("mapgen", sim.mapgen_rng.native_state_hex()),
        ] {
            assert_eq!(
                actual,
                boundary(row, "after", stream),
                "{label}: complete {stream}"
            );
        }
        let actual: Vec<_> = sim
            .sound_events
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::VocAt {
                    sound_id,
                    audible_to: None,
                    rx,
                    ry,
                    sub_x,
                    sub_y,
                    world_z_leptons,
                } => Some((
                    sound_id.as_str(),
                    [
                        i32::from(*rx) * 256 + sub_x.to_num::<i32>(),
                        i32::from(*ry) * 256 + sub_y.to_num::<i32>(),
                        *world_z_leptons,
                    ],
                )),
                _ => None,
            })
            .collect();
        let expected_names: Vec<_> = row["after"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| event["sound_name"].as_str().unwrap())
            .collect();
        let expected_coords: Vec<[i32; 3]> = row["ordered_calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call["kind"] == "PlayAtPosition")
            .map(|call| std::array::from_fn(|i| call["coordinate"][i].as_i64().unwrap() as i32))
            .collect();
        assert_eq!(
            actual,
            expected_names
                .into_iter()
                .zip(expected_coords)
                .collect::<Vec<_>>(),
            "{label}: ordered positional requests"
        );
        assert_eq!(
            u64::from(sim.main_rng.next_u32()),
            row["continuation"]["returned"].as_u64().unwrap(),
            "{label}: live continuation"
        );
        let key = row["continuation"]["main_after"].as_str().unwrap();
        assert_eq!(
            sim.main_rng.native_state_hex(),
            row["complete_rng_states"][key]["bytes"].as_str().unwrap(),
            "{label}: complete continuation state"
        );
    }
}
