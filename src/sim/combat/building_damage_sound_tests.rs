//! Recorded whole Building442230/Techno701900/Object5F5390 survivor controls.
//! Native identity, producer and coverage: building_damage_sound_joined.meta.json.
//! Original SHA2561cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c.
//! Health starts at the ordinary production constructor and is retained between
//! requests. The adopted RNG boundary follows construction/admission: this test
//! compares the receiver, not the native fixture's complete setup or audio pump.
//! Declared House/control and clear-arena inputs follow the pinned joined
//! producer; whole scenario/House/camera construction is outside this coverage.

use super::*;
use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use crate::rules::sound_ini::SoundRegistry;
use crate::sim::components::DriveCoord;
use crate::sim::world::Simulation;
use serde_json::Value;
use std::sync::Arc;

fn corpus() -> Value {
    let value: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_damage_sound_joined.json",
    ))
    .unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["selected_profile"], "all");
    assert_eq!(
        value["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    value
}

fn recorded_sections(sections: &Value) -> IniFile {
    let mut text = String::new();
    for (section, keys) in sections.as_object().unwrap() {
        text.push_str(&format!("[{section}]\n"));
        for (key, value) in keys.as_object().unwrap() {
            text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
    }
    IniFile::from_str(&text)
}

fn recorded_layer(corpus: &Value, filename: &str) -> Option<IniFile> {
    let layer = corpus["death_layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| {
            layer["file"]
                .as_str()
                .unwrap()
                .eq_ignore_ascii_case(filename)
        })
        .unwrap();
    if layer["absent"] == true {
        return None;
    }
    Some(recorded_sections(&layer["physical_input_sections"]))
}

fn production_rules(corpus: &Value, map_control: Option<&IniFile>) -> Option<RuleSet> {
    let (root, art) = crate::rules::retail_ini_fixture::retail_rules_and_art()?;
    let sound = crate::rules::retail_ini_fixture::retail_ini("soundmd.ini")?;
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        recorded_layer(corpus, "LANGRULE.INI"),
        art,
        Arc::new(SoundRegistry::from_ini(&sound)),
        Default::default(),
    )
    .unwrap();
    let mode = recorded_layer(corpus, "MPBattleMD.ini");
    let mut map = recorded_layer(corpus, "Hills.map").unwrap();
    if let Some(control) = map_control {
        map.merge(control);
    }
    Some(
        owner
            .load_scenario(NativeScenarioRulesPrefix::NonCampaign(mode.as_ref()), &map)
            .unwrap()
            .into_parts()
            .0,
    )
}

fn sound_name(corpus: &Value, index: i32) -> Option<&str> {
    if index == -1 {
        return None;
    }
    Some(
        corpus["selected_sounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["fixture_index"].as_i64().unwrap() as i32 == index)
            .unwrap()["name"]
            .as_str()
            .unwrap(),
    )
}

fn coordinate(value: &Value) -> DriveCoord {
    DriveCoord {
        x: value[0].as_i64().unwrap() as i32,
        y: value[1].as_i64().unwrap() as i32,
        z: value[2].as_i64().unwrap() as i32,
    }
}

fn rng_boundary<'a>(history: &'a Value, state: &Value, stream: &str) -> &'a str {
    let key = state["rng"][stream].as_str().unwrap();
    history["audio"]["complete_rng_states"][key]["bytes"]
        .as_str()
        .unwrap()
}

#[test]
fn damage_sound_retained_retail_readers_match_the_native_selected_references() {
    let corpus = corpus();
    let Some(rules) = production_rules(&corpus, None) else {
        return;
    };
    let native = &corpus["selected_inputs"];
    let target = rules.object(native["target"].as_str().unwrap()).unwrap();
    assert_eq!(target.strength, native["strength"].as_i64().unwrap() as i32);
    assert_eq!(
        target.can_c4,
        corpus["receiver_controls"]["input"]["can_c4"]
            .as_u64()
            .unwrap()
            != 0
    );
    // The oracle's selected SoundList has fixture ordinals. Resolve names
    // through it; these numbers are not the full retail catalog's ordinals.
    assert_eq!(
        target.damage_sound(),
        sound_name(&corpus, native["per_type_id"].as_i64().unwrap() as i32)
    );
    assert_eq!(
        rules.general.building_damage_sound(),
        sound_name(&corpus, native["global_id"].as_i64().unwrap() as i32)
    );
}

fn replay_building_history(corpus: &Value, rules: &RuleSet, history: &Value, type_name: &str) {
    // Independent forced Object/Techno rows retain their native bounds.
    // This production transaction enters Building442230, so compare its rows.
    let steps: Vec<_> = history["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["entry"] == 0x0044_2230)
        .collect();
    let first = steps.first().unwrap();
    let mut world = Simulation::new();
    let recipient = world.interner.intern("SoundRecipient");
    let source_owner = world.interner.intern("SoundSource");
    // The pinned joined fixture supplies source+1EC=0/+1ED=1 and
    // target+1EC=1/+1ED=0. Existing IsHuman/PlayerControl ownership gives
    // their semantics; these are declared setup bytes, not read outputs.
    for (owner, identity, human, player_control) in [
        (
            source_owner,
            &history["input"]["source_object"],
            false,
            true,
        ),
        (recipient, &history["input"]["target_object"], true, false),
    ] {
        let country = world
            .interner
            .intern(identity["house_type_name"].as_str().unwrap());
        let mut house = HouseState::new(owner, 0, Some(country), human, 0, 10);
        house.set_player_control_for_test(player_control);
        world.houses.register(house);
    }
    // Inherited make_dock_fixture declares mode1 and source PlayerPtr.
    world
        .session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    world.session.set_current_house_for_test(Some(source_owner));
    crate::sim::arena_fixture::flat_arena(&mut world, rules);
    let location = coordinate(&history["input"]["target_object"]["location"]);
    assert_eq!(
        location,
        coordinate(&first["before"]["building"]["location"])
    );
    let target_id = world
        .spawn_object(
            type_name,
            "SoundRecipient",
            u16::try_from(location.x.div_euclid(256)).unwrap(),
            u16::try_from(location.y.div_euclid(256)).unwrap(),
            0,
            rules,
        )
        .unwrap();
    let source_location = coordinate(&history["input"]["source_object"]["location"]);
    let source_id = world
        .spawn_object(
            "MTNK",
            "SoundSource",
            u16::try_from(source_location.x.div_euclid(256)).unwrap(),
            u16::try_from(source_location.y.div_euclid(256)).unwrap(),
            128,
            rules,
        )
        .unwrap();
    assert_eq!(
        crate::sim::movement::ground_pose::position_world_coord(
            &world.entities().get(source_id).unwrap().position
        ),
        source_location,
        "ordinary source Unlimbo coordinate"
    );
    world.resolve_type_handles(rules);
    let target = world.substrate.entities.get_mut(target_id).unwrap();
    assert_eq!(
        target.health.current(),
        history["building_constructor"]["health"].as_i64().unwrap() as i32
    );
    assert_eq!(
        crate::sim::movement::ground_pose::position_world_coord(&target.position),
        location
    );
    target.sample_building_health_for_power();
    // Use the production MTNK/weapon readers rather than supplying an armor
    // kernel. The oracle declares the original stock AP receiver argument.
    assert_eq!(history["input"]["warhead"], "original stock AP");
    let weapon = rules
        .weapon(rules.object("MTNK").unwrap().weapon_at(0).unwrap())
        .unwrap();
    assert_eq!(weapon.warhead.as_deref(), Some("AP"));
    let warhead = world.interner.intern(weapon.warhead.as_deref().unwrap());
    world.main_rng =
        SimRng::from_native_state_hex_for_test(rng_boundary(history, &first["before"], "main"))
            .into();
    world.scenario_rng =
        SimRng::from_native_state_hex_for_test(rng_boundary(history, &first["before"], "scenario"));
    world.mapgen_rng =
        SimRng::from_native_state_hex_for_test(rng_boundary(history, &first["before"], "mapgen"));

    for row in steps {
        let label = row["label"].as_str().unwrap();
        world
            .session
            .set_binary_frame_for_test(row["before"]["frame"].as_u64().unwrap() as u32);
        let target = world.entities().get(target_id).unwrap();
        assert_eq!(
            target.health.current(),
            row["before"]["building"]["health"].as_i64().unwrap() as i32,
            "{label}: naturally retained health"
        );
        assert_eq!(
            target.building_power_health_sample().unwrap(),
            row["before"]["building"]["sampled_health"]
                .as_i64()
                .unwrap() as i32,
            "{label}: prior health sample"
        );
        for (stream, actual) in [
            ("main", world.main_rng.native_state_hex()),
            ("scenario", world.scenario_rng.native_state_hex()),
            ("mapgen", world.mapgen_rng.native_state_hex()),
        ] {
            assert_eq!(
                actual,
                rng_boundary(history, &row["before"], stream),
                "{label}: retained {stream} before receiver"
            );
        }
        let event = EntityDamageEvent::direct_receiver(
            target_id,
            row["supplied_damage"].as_i64().unwrap() as i32,
            history["input"]["distance"].as_i64().unwrap() as i32,
            source_id,
            None,
            warhead,
            ReceiverCallFlags {
                ignore_defenses: row["ignore_defenses"].as_bool().unwrap(),
                arg6: false,
            },
        );
        let resolved = resolve_receive_damage(
            &event,
            &world.substrate.entities,
            rules,
            &world.interner,
            &world.houses,
            &world.house_alliances,
            world.session.no_damage(),
            u64::from(world.session.binary_frame()),
            world.resolved_terrain.as_ref(),
        )
        .unwrap();
        assert_eq!(
            resolved.outcome.hp_delta,
            row["damage_packet_after"].as_i64().unwrap() as i32,
            "{label}: existing defended kernel (surviving CanC4 case)"
        );
        // Snapshot the existing health owner only to observe its return value.
        // The actual world transaction below owns the live entity and effects.
        let mut local_world = Simulation::new();
        local_world.substrate.entities = world.substrate.entities.clone();
        local_world.interner = world.interner.clone();
        local_world.house_alliances = world.house_alliances.clone();
        local_world
            .session
            .set_binary_frame_for_test(world.session.binary_frame());
        let state = receiver_health::commit_receiver_health(
            &event,
            &mut local_world,
            rules,
            None,
            Some(resolved),
            crate::sim::world::FrameEffects::default(),
            false,
        )
        .unwrap()
        .state;
        assert_eq!(state as u64, row["result"].as_u64().unwrap(), "{label}");

        let sound_start = world.sound_events.len();
        let (_, actual_draws) = crate::sim::rng::trace_draws(|| {
            world_receiver::commit_entities(
                &mut world,
                &mut world_receiver::ReceiverRun::default(),
                std::slice::from_ref(&event),
                None,
                rules,
                None,
            )
        });
        // Reuse the native observer's actual raw words, including ranged
        // rejection advances. Complete per-stream state checks follow below.
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
            "{label}: receiver raw RNG order"
        );
        let target = world.entities().get(target_id).unwrap();
        assert_eq!(
            target.health.current(),
            row["after_receiver"]["building"]["health"]
                .as_i64()
                .unwrap() as i32,
            "{label}: production receiver health"
        );
        assert_eq!(
            target.building_power_health_sample().unwrap(),
            row["after_receiver"]["building"]["sampled_health"]
                .as_i64()
                .unwrap() as i32,
            "{label}: receiver leaves AI sample retained"
        );
        let actual: Vec<_> = world.sound_events[sound_start..]
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::VocAtCoord {
                    sound_id,
                    coordinate,
                    flags,
                } => Some((sound_id.as_str(), *coordinate, *flags)),
                _ => None,
            })
            .collect();
        let expected: Vec<_> = row["coordinate_attempts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|call| {
                (
                    sound_name(corpus, call["register_ecx"].as_i64().unwrap() as i32).unwrap(),
                    coordinate(&call["coordinate"]),
                    call["args"][0].as_u64().unwrap() as u32,
                )
            })
            .collect();
        assert_eq!(
            world.sound_events.len() - sound_start,
            expected.len(),
            "{label}: no additional sound transport"
        );
        assert_eq!(actual, expected, "{label}: ordered750E20 requests");
        for (stream, actual) in [
            ("main", world.main_rng.native_state_hex()),
            ("scenario", world.scenario_rng.native_state_hex()),
            ("mapgen", world.mapgen_rng.native_state_hex()),
        ] {
            assert_eq!(
                actual,
                rng_boundary(history, &row["after_receiver"], stream),
                "{label}: complete receiver {stream}"
            );
        }
        // The native fixture runs whole BuildingAI43FB20 between receiver steps.
        // Replay its two owners whose state this test retains, in native order:
        // the damage-fire producer43FC39..43FCBE, which spends one Scenario
        // type draw on the yellow edge even when the Image has no
        // DamageFireOffset slots, then the health sample440042..44006C.
        // Other BuildingAI callbacks and House dirty propagation are not
        // compared by this test.
        world.update_building_damage_fire(target_id, rules);
        let target = world.substrate.entities.get_mut(target_id).unwrap();
        target.sample_building_health_for_power();
        assert_eq!(
            target.building_power_health_sample().unwrap(),
            row["after_sample"]["building"]["sampled_health"]
                .as_i64()
                .unwrap() as i32,
            "{label}: sampled-health owner"
        );
        for (stream, actual) in [
            ("main", world.main_rng.native_state_hex()),
            ("scenario", world.scenario_rng.native_state_hex()),
            ("mapgen", world.mapgen_rng.native_state_hex()),
        ] {
            assert_eq!(
                actual,
                rng_boundary(history, &row["after_sample"], stream),
                "{label}: BuildingAI {stream}"
            );
        }
    }
}

#[test]
fn damage_sound_serial_building_receivers_match_native_health_packets_and_requests() {
    let corpus = corpus();
    let Some(rules) = production_rules(&corpus, None) else {
        return;
    };
    let history = &corpus["receiver_controls"];
    replay_building_history(
        &corpus,
        &rules,
        history,
        history["input"]["target"].as_str().unwrap(),
    );
}

#[test]
fn damage_sound_registered_reference_controls_match_native_threshold_requests() {
    let corpus = corpus();
    let histories = corpus["threshold_controls"]["histories"]
        .as_array()
        .unwrap();
    assert_eq!(histories.len(), 5, "all executed reference controls");
    for history in histories {
        let label = history["label"].as_str().unwrap();
        let section = history["synthetic_input_section"].as_object().unwrap();
        assert_eq!(section.len(), 1, "{label}: declared native type");
        let type_name = section.keys().next().unwrap();
        let mut control = recorded_sections(&history["synthetic_input_section"]);
        // Native explicitly constructs this named BuildingType. Register the
        // same declared type through the ordinary Rust scenario reader; its
        // fields are the recorded physical CAMISC03 section/Image translation.
        control.merge(&IniFile::from_str(&format!(
            "[BuildingTypes]\n0={type_name}\n"
        )));
        let cold = RuleSet::from_ini(&control).unwrap();
        assert_eq!(
            cold.object(type_name).unwrap().damage_sound(),
            sound_name(&corpus, history["constructor_id"].as_i64().unwrap() as i32),
            "{label}: pre-catalog constructor default"
        );
        let Some(rules) = production_rules(&corpus, Some(&control)) else {
            return;
        };
        assert_eq!(history["reader_result"], 1, "{label}: native admission");
        let target = rules.object(type_name).unwrap();
        assert_eq!(
            target.strength,
            history["building_constructor"]["health"].as_i64().unwrap() as i32,
            "{label}: production strength/constructor health"
        );
        assert_eq!(
            target.damage_sound(),
            sound_name(&corpus, history["native_bound_id"].as_i64().unwrap() as i32),
            "{label}: production retained catalog binding"
        );
        assert_eq!(
            rules.general.building_damage_sound(),
            sound_name(&corpus, history["global_bound_id"].as_i64().unwrap() as i32),
            "{label}: global retained binding"
        );
        replay_building_history(&corpus, &rules, history, type_name);
    }
}
