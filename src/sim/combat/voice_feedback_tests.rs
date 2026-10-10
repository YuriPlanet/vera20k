//! Original result-2 VoiceFeedback702695..7027F7 arithmetic/admission controls.
//! The native fixture supplies entry to that result arm. Rust reaches it through
//! the existing health receiver with an authored 100->40 hit; the neighboring
//! combat test retains non-crossing/red/dead/empty-list admission contrasts.

use super::*;
use crate::rules::sound_ini::SoundRegistry;
use crate::sim::components::DriveCoord;
use crate::sim::world::Simulation;
use serde_json::Value;

fn boundary<'a>(row: &'a Value, phase: &str, stream: &str) -> &'a str {
    let key = row[phase]["rng"][stream].as_str().unwrap();
    row["complete_rng_states"][key]["bytes"].as_str().unwrap()
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
        target.category = EntityCategory::Infantry;
        target.health.current = 100;
        target.lifecycle.in_limbo = false;
        target.lifecycle.cell_marked = true;
        target.in_playfield = true;
        let coordinate = &input["coordinate"];
        crate::sim::movement::ground_pose::put_location(
            &mut target.position,
            DriveCoord {
                x: coordinate[0].as_i64().unwrap() as i32,
                y: coordinate[1].as_i64().unwrap() as i32,
                z: coordinate[2].as_i64().unwrap() as i32,
            },
        );
        let mut sim = Simulation::new();
        sim.interner = test_interner();
        let owner = target.owner();
        let other = sim.interner.intern("OtherHouse");
        let mut house = HouseState::new(owner, 0, None, input["human"].as_bool().unwrap(), 0, 10);
        house.player_control = input["player_control"].as_bool().unwrap();
        sim.houses.insert(owner, house);
        sim.session.game_mode_nonzero = input["game_mode"].as_u64().unwrap() != 0;
        sim.session.current_house = Some(if input["current_house_equal"].as_bool().unwrap() {
            owner
        } else {
            other
        });
        sim.substrate.entities.insert(target);
        let warhead = sim.interner.intern("FeedbackWH");
        sim.main_rng = SimRng::from_native_state_hex_for_test(boundary(row, "before", "main"));
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
            sim.entities().get(1).unwrap().health.current,
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
            ("main", &sim.main_rng),
            ("scenario", &sim.scenario_rng),
            ("mapgen", &sim.mapgen_rng),
        ] {
            assert_eq!(
                actual.native_state_hex(),
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
