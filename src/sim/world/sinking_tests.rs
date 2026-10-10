//! Unit AI suffix comparison. The native rows stop at Anim constructor or
//! UnInit entry; animation playback and full cleanup assertions are separate
//! Rust integration checks, not coverage claimed by those native boundaries.

use super::*;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::movement::ShipLocomotionRuntime;
use crate::sim::rng::SimRng;
use crate::util::fixed_math::SimFixed;

fn fixture(relative_z: i32) -> (Simulation, RuleSet, u64) {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nWake=WAKE1\n[VehicleTypes]\n0=TEST\n\
         [TEST]\nStrength=800\nNaval=yes\nWeight=4\nCrewed=no\n\
         Speed=4\nSpeedType=Float\nMovementZone=Water\n\
         Locomotor={2BEA74E1-7CCA-11d3-BE14-00104B62A16C}\n",
    ))
    .unwrap();
    rules.replace_art_registry_for_test(crate::rules::art_data::ArtRegistry::from_ini(
        &IniFile::from_str("[WAKE1]\nEnd=10\nRate=100\n"),
    ));
    rules.bind_anim_frame_count_for_test("WAKE1", 10);
    let mut sim = Simulation::new();
    super::super::lifecycle_tests::install_common_raw_terrain(&mut sim, 128, 82, 2, None);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, false, 0, 10));
    sim.session.house_order.push(owner);
    let id = sim.allocate_stable_id();
    let mut entity = GameEntity::test_default(id, "TEST", "Americans", 113, 59);
    entity.type_ref = sim.interner.intern("TEST");
    entity.owner = owner;
    entity.category = EntityCategory::Unit;
    entity.health.current = 0;
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = false;
    entity.position.sub_x = SimFixed::from_num(128);
    entity.position.sub_y = SimFixed::from_num(128);
    entity.position.z = 2;
    entity.position.exact_z_leptons = Some(208 + relative_z);
    sim.substrate.entities.insert(entity);
    sim.begin_receiver_kill_record(id);
    sim.record_destruction_once(id);
    sim.begin_ship_sinking(id, &rules);
    (sim, rules, id)
}

#[test]
fn native_sink_suffix_preserves_cadence_coordinates_and_complete_rng_states() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/naval_sink_tick.json",
    ))
    .unwrap();
    for row in corpus["cases"].as_array().unwrap() {
        let input = &row["input"];
        let expected = &row["output"];
        let name = input["name"].as_str().unwrap();
        let (mut sim, rules, id) = fixture(input["relative_z"].as_i64().unwrap_or(0) as i32);
        sim.session.binary_frame = input["frame"].as_u64().unwrap() as u32;
        let seed = input["seed"].as_u64().unwrap_or(1);
        sim.main_rng = SimRng::new(seed);
        sim.scenario_rng = SimRng::new(seed);
        sim.mapgen_rng = SimRng::new(seed);
        sim.substrate.entities.get_mut(id).unwrap().sinking.active =
            input["sinking"].as_u64().unwrap_or(1) != 0;
        let terminal = sim.tick_ship_sinking(id, &rules, None);
        let entity = sim.substrate.entities.get(id).unwrap();
        let coord = position_world_coord(&entity.position);
        assert_eq!(
            serde_json::json!([coord.x, coord.y, coord.z]),
            expected["coord"],
            "{name}"
        );
        assert_eq!(
            entity.health.current,
            expected["health"].as_i64().unwrap() as i32,
            "{name}"
        );
        assert_eq!(terminal, expected["endpoint"] == "0x4de5d0", "{name}");
        assert_eq!(
            sim.houses[&entity.owner()].stats.units_lost(),
            expected["owner_units_lost"].as_u64().unwrap() as u32,
            "{name}"
        );
        for (stream, rng) in [
            ("main", &sim.main_rng),
            ("scenario", &sim.scenario_rng),
            ("mapgen", &sim.mapgen_rng),
        ] {
            assert_eq!(
                rng.native_state_hex(),
                expected["rng"][stream]["after_hex"].as_str().unwrap(),
                "{name} {stream}"
            );
        }
        let wake = expected["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["kind"] == "wake_constructor_boundary");
        assert_eq!(
            sim.substrate.anims.len(),
            usize::from(wake.is_some()),
            "{name}"
        );
        if let Some(wake) = wake {
            let (_, anim) = sim.substrate.anims.iter().next().unwrap();
            assert_eq!(
                serde_json::json!([anim.world_coord.x, anim.world_coord.y, anim.world_coord.z]),
                wake["coord"],
                "{name}"
            );
            assert_eq!(
                anim.draw_flags,
                wake["draw_flags"].as_u64().unwrap() as u32,
                "{name}"
            );
        }
    }
}

#[test]
fn sinking_state_is_hashed_and_survives_snapshot() {
    use crate::sim::snapshot::GameSnapshot;

    let (mut sim, rules, id) = fixture(-125);
    sim.session.map_name = "SINK.MAP".to_owned();
    // Both futures begin with the production rule handles initialized. Their
    // warhead names must already be interned when the first wake is allocated.
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    // Native load restarts Scenario from Seed0. Begin both compared futures
    // at that same cursor; this test does not assert saved Scenario retention.
    sim.scenario_rng = SimRng::new(0);
    let active_hash = sim.state_hash();
    sim.sinking_edge_sounds(id, &rules);
    assert_ne!(
        sim.state_hash(),
        active_hash,
        "the observed sound edge is authority"
    );
    let expected_hash = sim.state_hash();
    let bytes = GameSnapshot::save_validated(&sim, 17, 18, "sinking", 19);
    let mut loaded = GameSnapshot::load_validated(&bytes, 17, 18, "SINK.MAP")
        .unwrap()
        .sim;
    // Production keeps Main/MapGen as process state, outside serialized sim.
    loaded.retain_in_scenario_process_state_from(&sim);
    // Map-derived floor data is supplied by the ordinary map reload.
    loaded.resolved_terrain = sim.resolved_terrain.clone();
    loaded.restore_after_snapshot_load().unwrap();
    loaded.resolve_type_handles(&rules);
    assert_eq!(loaded.state_hash(), expected_hash);
    assert!(
        loaded
            .substrate
            .entities
            .get(id)
            .unwrap()
            .sinking
            .sound_edge_seen()
    );
    for frame in 1..=60 {
        sim.session.binary_frame = frame;
        loaded.session.binary_frame = frame;
        let terminal = sim.tick_ship_sinking(id, &rules, None);
        assert_eq!(loaded.tick_ship_sinking(id, &rules, None), terminal);
        assert_eq!(loaded.state_hash(), sim.state_hash(), "frame {frame}");
        assert_eq!(loaded.rng_state(), sim.rng_state(), "frame {frame}");
        if terminal {
            let hull = loaded.substrate.entities.get(id).unwrap();
            assert!(!hull.lifecycle.object_alive);
            assert!(hull.lifecycle.in_limbo);
            assert!(loaded.substrate.pending_delete.contains(&id));
            assert_eq!(loaded.houses[&hull.owner()].stats.units_lost(), 2);
            return;
        }
    }
    panic!("the restored retained hull must reach its terminal lifetime");
}

fn sections_ini(sections: &serde_json::Value) -> String {
    let mut text = String::new();
    for (section, entries) in sections.as_object().unwrap() {
        text.push_str(&format!("[{section}]\n"));
        for (key, value) in entries.as_object().unwrap() {
            text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
    }
    text
}

#[test]
fn native_sinking_sound_readers_and_reachable_edges_match() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::sound_ini::SoundRegistry;

    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/naval_lifetime_audio.json",
    ))
    .unwrap();
    let sounds =
        SoundRegistry::from_ini(&IniFile::from_str(&sections_ini(&corpus["sound_sections"])));
    let sound_name = corpus["sound"]["name"].as_str().unwrap();
    let sound = sounds.get(sound_name).unwrap();
    assert_eq!(
        serde_json::json!(sound.sounds),
        corpus["sound"]["sample_names"]
    );
    // The fixed one-entry catalog is native fixture index0, preserving the
    // physical name and section bytes without inventing a full-retail ordinal.
    let mut layers = RulesLayerStack::new(IniFile::from_str(&format!(
        "[VehicleTypes]\n0=AEGIS\n{}",
        sections_ini(&corpus["layers"][0]["sections"])
    )));
    for (index, layer) in corpus["layers"].as_array().unwrap().iter().enumerate() {
        if index != 0 {
            layers.push(
                if index == 1 {
                    RulesLayerKind::GameMode
                } else {
                    RulesLayerKind::Scenario
                },
                IniFile::from_str(&sections_ini(&layer["sections"])),
            );
        }
        let processed = layers.process().unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.bind_type_sound_references(processed.ini(), &sounds);
        let object = rules.object("AEGIS").unwrap();
        for (field, actual) in [
            ("type_sinking", object.sinking_sound.as_deref()),
            ("voice_sinking", object.voice_sinking.as_deref()),
            ("rules_sinking", rules.general.sinking_sound.as_deref()),
        ] {
            let index = layer["after"][field].as_i64().unwrap();
            assert_eq!(actual, (index == 0).then_some(sound_name), "{field}");
        }
    }

    let mut compared = 0;
    for row in corpus["cases"].as_array().unwrap() {
        let input = &row["input"];
        // Raw255 rows are supplied counterfactuals: constructor and the
        // original fatal producer only write zero/one to the private state.
        if input["sinking"].as_u64().unwrap() > 1 {
            continue;
        }
        compared += 1;
        let name = input["name"].as_str().unwrap();
        let (mut sim, mut rules, id) = fixture(0);
        let quiet = input["quiet"].as_bool().unwrap_or(false);
        let fallback = input["fallback"].as_bool().unwrap_or(false);
        let reader_input = IniFile::from_str(&format!(
            "[TEST]\nSinkingSound={}\nVoiceSinking={}\n[AudioVisual]\nSinkingSound={}\n",
            if quiet || fallback {
                "unknown"
            } else {
                sound_name
            },
            if input["voice"].as_bool().unwrap_or(false) {
                sound_name
            } else {
                "unknown"
            },
            if quiet { "unknown" } else { sound_name },
        ));
        rules.bind_type_sound_references(&reader_input, &sounds);
        let hull = sim.substrate.entities.get_mut(id).unwrap();
        hull.sinking.active = input["sinking"].as_u64().unwrap() != 0;
        hull.sinking.sound_edge_seen = input["seen"].as_u64().unwrap() != 0;
        hull.move_sound = crate::sim::world::MoveSoundState::from_raw_for_test(
            input["move_sound"].as_u64().unwrap_or(0) != 0,
            0,
        );
        sim.main_rng = SimRng::new(1);
        sim.scenario_rng = SimRng::new(1);
        sim.mapgen_rng = SimRng::new(1);
        sim.sound_events.clear();
        sim.sinking_edge_sounds(id, &rules);
        let events: Vec<_> = sim
            .sound_events
            .iter()
            .map(|event| match event {
                SimSoundEvent::AnimationStarted {
                    anim_id,
                    sound_id,
                    world,
                } => {
                    assert_eq!(*anim_id, id);
                    assert_eq!(sound_id, sound_name);
                    serde_json::json!(["sound_boundary", "Foot+544", [world.x, world.y, world.z]])
                }
                SimSoundEvent::VocAt {
                    sound_id,
                    audible_to,
                    world_z_leptons,
                    ..
                } => {
                    assert_eq!(sound_id, sound_name);
                    assert!(audible_to.is_none(), "VoiceSinking has no human-owner gate");
                    serde_json::json!(["sound_boundary", "0x0", [29056, 15232, world_z_leptons]])
                }
                SimSoundEvent::ObjectSoundReleased { owner } => {
                    assert_eq!(*owner, id);
                    serde_json::json!(["original_release", "Foot+544", null])
                }
                _ => panic!("unexpected sound edge output {name}: {event:?}"),
            })
            .collect();
        let expected: Vec<_> = row["output"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| serde_json::json!([event["kind"], event["handle"], event["xyz"]]))
            .collect();
        assert_eq!(events, expected, "{name}");
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .sinking
                .sound_edge_seen,
            row["output"]["seen"].as_u64().unwrap() != 0,
            "{name}"
        );
        for (stream, rng) in [
            ("main", &sim.main_rng),
            ("scenario", &sim.scenario_rng),
            ("mapgen", &sim.mapgen_rng),
        ] {
            assert_eq!(
                rng.native_state_hex(),
                row["output"]["rng_after"][stream].as_str().unwrap(),
                "{name} {stream}"
            );
        }
    }
    assert_eq!(compared, 7);
}

#[test]
fn load_preserves_sinking_edge_and_resets_shared_foot_sound_bytes() {
    use crate::rules::sound_ini::SoundRegistry;
    use crate::sim::snapshot::GameSnapshot;
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/naval_lifetime_controls.json",
    ))
    .unwrap();
    for row in corpus["load"].as_array().unwrap() {
        let input = &row["input"];
        let seen = input["saved_seen"].as_i64().unwrap() != 0;
        let (mut sim, mut rules, id) = fixture(-30);
        let sounds =
            SoundRegistry::from_ini(&IniFile::from_str("[SoundList]\n0=GenLargeWaterDie\n"));
        rules.bind_type_sound_references(
            &IniFile::from_str("[TEST]\nSinkingSound=GenLargeWaterDie\n"),
            &sounds,
        );
        sim.session.map_name = "SINK.MAP".to_owned();
        let hull = sim.substrate.entities.get_mut(id).unwrap();
        hull.sinking.sound_edge_seen = seen;
        hull.move_sound = crate::sim::world::MoveSoundState::from_raw_for_test(
            input["saved_move_sound"].as_i64().unwrap() != 0,
            i32::try_from(input["saved_move_countdown"].as_i64().unwrap()).unwrap(),
        );
        let bytes = GameSnapshot::save_validated(&sim, 17, 18, "sound edge", 0);
        let mut loaded = GameSnapshot::load_validated(&bytes, 17, 18, "SINK.MAP")
            .unwrap()
            .sim;
        loaded.retain_in_scenario_process_state_from(&sim);
        loaded.restore_after_snapshot_load().unwrap();
        loaded.resolve_type_handles(&rules);
        let hull = loaded.substrate.entities.get(id).unwrap();
        assert_eq!(hull.sinking.is_active(), row["after_noinit"][0] == 1);
        assert_eq!(hull.sinking.sound_edge_seen, row["after_noinit"][1] == 1);
        assert_eq!(
            hull.move_sound.is_active(),
            row["after_tail"]["move_sound"] == 1
        );
        assert_eq!(
            i64::from(hull.move_sound.countdown()),
            row["after_tail"]["move_countdown"].as_i64().unwrap()
        );
        let rng = loaded.rng_state();
        assert!(loaded.sound_events.is_empty());
        loaded.sinking_edge_sounds(id, &rules);
        assert_eq!(
            loaded.sound_events.len(),
            row["events"].as_array().unwrap().len()
        );
        assert_eq!(
            loaded
                .substrate
                .entities
                .get(id)
                .unwrap()
                .sinking
                .sound_edge_seen,
            row["after_edge_seen"] == 1
        );
        assert_eq!(loaded.rng_state(), rng);
    }
}

#[test]
fn retained_health_one_ship_is_illegal_as_firer_and_target() {
    use crate::sim::combat::TargetKind;
    use crate::sim::combat::fire_error::FireError;
    use crate::sim::combat::fire_error_world::FireSubject;
    use crate::sim::mission::concrete_effects::assign_target_commits;

    let (mut sim, _, target_id) = fixture(0);
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=TEST\n[TEST]\nStrength=800\nPrimary=Gun\n\
         [Gun]\nDamage=50\nROF=30\nRange=6\nWarhead=AP\nOmniFire=yes\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap();
    let firer_id = sim.allocate_stable_id();
    let mut firer = GameEntity::test_default(firer_id, "TEST", "Russians", 112, 59);
    firer.type_ref = sim.interner.intern("TEST");
    firer.owner = sim.interner.intern("Russians");
    firer.lifecycle.in_limbo = false;
    firer.health.current = 800;
    firer.position.z = 2;
    firer.position.exact_z_leptons = Some(208);
    sim.substrate.entities.insert(firer);
    sim.resolve_type_handles(&rules);
    let query = |sim: &Simulation| {
        FireSubject {
            world: sim,
            rules: &rules,
            overlay_registry: None,
            fog: None,
            firer: sim.substrate.entities.get(firer_id).unwrap(),
            obj: rules.object("TEST").unwrap(),
            target: Some(TargetKind::Entity(target_id)),
            weapon_index: 0,
        }
        .fire_error(false)
    };
    assert_eq!(query(&sim), FireError::Illegal, "native target+3CD6FCBCD");
    assert!(!assign_target_commits(
        &sim.substrate.entities,
        Some(TargetKind::Entity(target_id))
    ));
    sim.substrate
        .entities
        .get_mut(target_id)
        .unwrap()
        .sinking
        .active = false;
    assert_eq!(
        query(&sim),
        FireError::Ok,
        "same Health1/Alive1 without sinking"
    );
    assert!(assign_target_commits(
        &sim.substrate.entities,
        Some(TargetKind::Entity(target_id))
    ));
    sim.substrate
        .entities
        .get_mut(firer_id)
        .unwrap()
        .sinking
        .active = true;
    assert_eq!(query(&sim), FireError::Illegal, "native firer+3CD6FC125");
}

#[test]
fn retained_ship_skips_process_but_keeps_its_head_and_runs_sinking_ai() {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::components::{DriveCoord, TrackProgress};
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::sim::movement::slope_transition;
    use crate::sim::world::techno_ai::ObjectAiCtx;

    let (mut sim, rules, id) = fixture(0);
    sim.session.binary_frame = 1;
    let head = DriveCoord::cell(114, 59, 208);
    let hull = sim.substrate.entities.get_mut(id).unwrap();
    hull.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
    assert!(
        hull.locomotor
            .as_mut()
            .unwrap()
            .install_ship_state_for_test(Some(
                ShipLocomotionRuntime::default()
                    .with_head_to_for_test(Some(head))
                    .with_track_valid_for_test(true)
                    .with_target_speed_fraction_for_test(SimFixed::from_num(1))
                    .with_track_for_test(TrackProgress {
                        turn_index: 0,
                        cursor: 0,
                        reversed: false,
                        residual: 0,
                    })
            ))
    );
    slope_transition::snap_after_successful_unlimbo(hull, 5, 0);
    let slope_before = *slope_transition::state_for_entity(hull).unwrap();
    // A second Stun (Unit setter, Path[0] = -1, Stop_Driver) clears the
    // destination, retaining the committed head.
    sim.techno_death_stun(id, UninitContext::with_rules(&rules));
    sim.advance_live_object_turn(id, Some(&rules), ObjectAiCtx::default())
        .unwrap();
    let hull = sim.substrate.entities.get(id).unwrap();
    assert_eq!(
        hull.locomotor
            .as_ref()
            .and_then(|l| l.selected_ship_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .head_to(),
        Some(head)
    );
    assert_eq!(
        hull.locomotor
            .as_ref()
            .and_then(|l| l.selected_ship_runtime())
            .and_then(|r| r.retained())
            .unwrap()
            .track()
            .cursor,
        0
    );
    assert_eq!(
        *slope_transition::state_for_entity(hull).unwrap(),
        slope_before
    );
    assert_eq!(position_world_coord(&hull.position).z, 203);
    assert_eq!((hull.position.rx, hull.position.ry), (113, 59));
    assert_eq!(hull.body_frame_counter, 0);
    assert!(hull.sinking.sound_edge_seen());
}
