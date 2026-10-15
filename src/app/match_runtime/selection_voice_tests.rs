//! Native VoiceSelect708EB0 / QueueVoice708D90 comparisons, using the
//! executable corpus and production Rules/Simulation owners. Request controls
//! need no device; the cleanup regression also exercises the real SFX consumer.

use super::selection_voice_event;
use crate::app::match_runtime::sound_dispatch;
use crate::audio::events::{GameSoundEvent, SoundEventQueue};
use crate::audio::voice_queue::VoiceQueue;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::Simulation;

#[test]
fn pack_voice_requests_match_native_slots_suppression_and_pending_owner() {
    let native = crate::rules::deploy_voice_tests::native();
    let controls = native["voice_controls"].as_array().unwrap();
    assert_eq!(controls.len(), 14);
    for row in controls {
        let context = row["name"].as_str().unwrap();
        let input = &row["input"];
        let rules = crate::rules::deploy_voice_tests::control_rules(&native, input);
        let mut sim = simulation(
            "YAREFN",
            EntityCategory::Structure,
            if input["owner_is_local"] == true {
                "Local"
            } else {
                "Other"
            },
            false,
        );
        let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(16);
        if input["resource_cell"] == true {
            terrain.cell_mut(10, 10).unwrap().land_type =
                crate::rules::terrain_rules::LandType::Tiberium.as_index();
        }
        sim.install_resolved_terrain_for_new_map(terrain);
        let entity = sim.entities_mut().get_mut(1).unwrap();
        if input["archive_cell"] == true {
            entity.set_archive_target(Some(crate::sim::combat::TargetKind::Cell(10, 10)));
        }
        if input["suppression"] == true {
            entity.suppress_deploy_voice.suppress();
        }
        let archive_before = entity.archive_target();
        let mission_before = bincode::serialize(&entity.mission).unwrap();
        let main_before = sim.rng_views().main.native_state_hex();
        let scenario_before = sim.scenario_rng.native_state_hex();
        let mapgen_before = sim.mapgen_rng.native_state_hex();
        let mut queue = VoiceQueue::new();
        if let Some(prior) = input["pending"].as_str() {
            queue.queue(1, prior);
        }
        if let Some(sound) =
            sim.deploy_voice_request(&rules, 1, input["voices_enabled"].as_bool().unwrap())
        {
            queue.queue(1, sound);
        }
        assert_eq!(
            queue.pending_for(1),
            row["output"]["pending"].as_str(),
            "{context}"
        );
        assert_eq!(
            queue.playing_for(1),
            None,
            "{context}: request does not play"
        );
        let entity = sim.entities().get(1).unwrap();
        assert_eq!(
            entity.deploy_voice_suppressed(),
            row["output"]["suppression"].as_bool().unwrap(),
            "{context}: suppression"
        );
        assert_eq!(
            entity.archive_target(),
            archive_before,
            "{context}: Archive retained"
        );
        assert_eq!(
            bincode::serialize(&entity.mission).unwrap(),
            mission_before,
            "{context}: timers"
        );
        assert_eq!(
            sim.rng_views().main.native_state_hex(),
            main_before,
            "{context}: Main"
        );
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            scenario_before,
            "{context}: Scenario"
        );
        assert_eq!(
            sim.mapgen_rng.native_state_hex(),
            mapgen_before,
            "{context}: MapGen"
        );
    }
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn pack_voice_destroyed_before_a_head_is_forgotten_by_the_production_consumer() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let Some(mut player) = crate::audio::sfx::SfxPlayer::new() else {
        eprintln!("pack-voice production consumer check needs an audio device");
        return;
    };
    player.set_volume(0.0);
    player
        .observe_pcm(crate::audio::sfx::PcmObservationConfig {
            sound_ids: vec!["SlaveMinerUnDeployVoice".into()],
            max_events: 1,
            max_samples_per_event: 1,
        })
        .unwrap();
    let mut process = crate::app::process_assets::ProcessAssets::new(
        crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
        true,
    );
    process.initialize_sources_if_needed(&assets).unwrap();
    let playback = crate::app::presentation::building_anim::SoundPlaybackInputs {
        assets: &assets,
        catalog: process.audio_catalog().unwrap(),
        listener: crate::audio::sfx::SpatialListener {
            tactical_width: 640,
            tactical_height: 480,
            origin_x: 0.0,
            origin_y: 0.0,
            zoom: 1.0,
        },
        eva_side: crate::rules::sound_ini::EvaSide::Yuri,
    };
    let mut sim = simulation("YAREFN", EntityCategory::Structure, "Local", false);
    let mut events = SoundEventQueue::new();
    crate::sim::production::sell_stage_zero(
        &mut sim,
        Some(&retail.rules),
        1,
        crate::sim::world::FrameEffects::empty(),
    );
    let requests = std::mem::take(&mut sim.sound_events);
    sound_dispatch::dispatch_sim_sound_events(
        requests,
        &sim,
        &retail.rules,
        Some("Local"),
        &mut |_| false,
        &mut events,
    );
    playback.consume(&mut player, &events.drain(), Some(&sim), Some("Local"));
    assert!(player.pending_unit_voice_owners().contains(&1));

    // Original Techno destructor6F4607 ->405C00 stops its handle and forgets
    // pending/playing slots. Destroy between Selling44A9DF and the next head:
    // use the actual lifecycle producer, dispatcher and shared SFX consumer.
    sim.uninit_with_context(
        1,
        crate::sim::world::UninitContext::with_rules(&retail.rules),
    );
    assert!(player.pending_unit_voice_owners().contains(&1));
    sim.process_pending_delete_with(
        Some(&retail.rules),
        None,
        crate::sim::world::FrameEffects::empty(),
        1,
        true,
    );
    assert!(sim.entities().get(1).is_none());
    let destruction = std::mem::take(&mut sim.sound_events);
    sound_dispatch::dispatch_sim_sound_events(
        destruction,
        &sim,
        &retail.rules,
        Some("Local"),
        &mut |_| false,
        &mut events,
    );
    playback.consume(&mut player, &events.drain(), Some(&sim), Some("Local"));
    assert!(!player.pending_unit_voice_owners().contains(&1));
    // Even a stale downstream head cannot revive the removed owner's line.
    playback.consume(
        &mut player,
        &[GameSoundEvent::UnitVoiceVisit { owner: 1 }],
        Some(&sim),
        Some("Local"),
    );
    let report = player.finish_pcm_observation().unwrap().into_json();
    assert!(report["outputs"].as_array().unwrap().is_empty());
    let actions: Vec<_> = report["voice_actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|action| action["owner"] == 1)
        .collect();
    assert_eq!(actions.len(), 2);
    assert_eq!(actions[0]["action"]["kind"], "queued");
    assert_eq!(actions[0]["after"]["pending"], "SLAVEMINERUNDEPLOYVOICE");
    assert_eq!(actions[1]["action"]["kind"], "destroyed");
    assert_eq!(actions[1]["before"]["pending"], "SLAVEMINERUNDEPLOYVOICE");
    assert!(actions[1]["after"]["pending"].is_null());
    assert!(actions[1]["after"]["playing"].is_null());
    player.stop_all();
}

fn corpus() -> serde_json::Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/selection_navigation.json",
    ))
    .unwrap()
}

fn sound_name(id: i64) -> String {
    if id == -1 {
        String::new()
    } else {
        format!("Sound{id}")
    }
}

fn simulation(
    type_id: &str,
    category: EntityCategory,
    owner_name: &str,
    passive: bool,
) -> Simulation {
    let mut sim = Simulation::with_seed(1234);
    let local = sim.interner.intern("Local");
    let owner = sim.interner.intern(owner_name);
    let type_ref = sim.interner.intern(type_id);
    let mut house = HouseState::new(owner, 0, None, owner == local, 0, 10);
    house.multiplay_passive = passive;
    sim.houses.insert(owner, house);
    sim.session.game_mode_nonzero = true;
    sim.session.current_house = Some(local);
    sim.entities_mut()
        .insert(GameEntity::new_at_frame_zero_for_test(
            1,
            10,
            10,
            0,
            0,
            owner,
            Health::new(100),
            type_ref,
            category,
            0,
            5,
            false,
        ));
    sim
}

#[test]
fn select_wrapper_gates_precede_voice_draw_and_passive_queue_rejection_follows_it() {
    let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[VehicleTypes]\n0=TANK\n[TANK]\nVoiceSelect=Sound101,Sound202,Sound303\n",
    ))
    .unwrap();
    for (owner, passive, enabled, drew, queued) in [
        ("Local", false, true, true, true),
        ("Local", false, false, false, false),
        ("Other", false, true, false, false),
        ("Other", true, true, true, false),
        ("Other", true, false, false, false),
    ] {
        let mut sim = simulation("TANK", EntityCategory::Unit, owner, passive);
        let before_sim = sim.scenario_rng.native_state_hex();
        let before = sim.main_rng.native_state_hex();
        let event = selection_voice_event(&mut sim, &rules, 1, enabled);
        assert_eq!(event.is_some(), queued, "{owner}/{passive}/{enabled}");
        assert_eq!(
            before != sim.main_rng.native_state_hex(),
            drew,
            "{owner}/{passive}/{enabled}"
        );
        assert_eq!(sim.scenario_rng.native_state_hex(), before_sim);
    }
}

#[test]
fn retail_normal_selection_reaches_fixed_voice_list_and_queue_without_an_audio_device() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let native = corpus();
    for (type_id, kind, category) in [
        ("E1", "infantry", EntityCategory::Infantry),
        ("MTNK", "unit", EntityCategory::Unit),
    ] {
        let object = fixture.rules.object(type_id).unwrap();
        assert!(
            !object.voice_select.is_empty(),
            "retail {type_id} has a selection voice"
        );
        // Bind native's supplied numeric Voc identities to the actual fixed
        // SOUNDMD names at the same slots. The original execution supplies
        // the chosen slot and full Main RNG continuation; no modulo oracle
        // is calculated in this test.
        let history = native["voice_histories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| {
                row["kind"] == kind
                    && row["seed"] == 1
                    && row["voice_enabled"].is_null()
                    && row["owner"].is_null()
                    && row["slave"].is_null()
                    && row["robot_offline"].is_null()
                    && row["advance_main_raw"].is_null()
                    && row["voice_list"].as_array().unwrap().len() == object.voice_select.len()
            })
            .expect("executed native list cardinality for retail common path");
        let slots = history["voice_list"].as_array().unwrap();
        let mut sim = simulation(type_id, category, "Local", false);
        sim.initialize_random_number_system(1);
        let before_sim = sim.scenario_rng.native_state_hex();
        let mut events = SoundEventQueue::new();
        let mut queue = VoiceQueue::new();
        for step in history["steps"].as_array().unwrap() {
            let request = &step["voice_requests"][0];
            let slot = slots
                .iter()
                .position(|id| *id == request["sound_id"])
                .unwrap();
            let event = selection_voice_event(&mut sim, &fixture.rules, 1, true)
                .expect("normal retail selection queues its resolved voice without output");
            assert_eq!(event.sound_id(), object.voice_select[slot]);
            events.push(event);
            for event in events.iter() {
                let GameSoundEvent::UnitSelected {
                    speaker_id,
                    sound_id,
                } = event
                else {
                    panic!("unexpected selection event {event:?}");
                };
                queue.queue(*speaker_id, sound_id);
            }
            let _ = events.drain();
            assert_eq!(
                queue.pending_for(1),
                Some(object.voice_select[slot].as_str())
            );
        }
        assert_eq!(
            sim.main_rng.native_state_hex(),
            history["rng_after_hex"]["main"].as_str().unwrap()
        );
        assert_eq!(sim.scenario_rng.native_state_hex(), before_sim);
    }
}

fn prefixed_selection_fixture() -> (Simulation, RuleSet, serde_json::Value) {
    let native = corpus();
    let history = native["voice_histories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|history| history["id"] == "unit_multiple_seed1_after248_main_raw")
        .unwrap()
        .clone();
    let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[VehicleTypes]\n0=TANK\n[TANK]\nVoiceSelect=Sound101,Sound202,Sound303\n",
    ))
    .unwrap();
    let mut sim = simulation("TANK", EntityCategory::Unit, "Local", false);
    sim.initialize_random_number_system(history["seed"].as_u64().unwrap() as u32);
    {
        let (_, main) = sim.terrain_load_draws();
        for expected in history["main_prefix"]["draws"].as_array().unwrap() {
            assert_eq!(u64::from(main.next_u32()), expected.as_u64().unwrap());
        }
    }
    assert_eq!(
        sim.main_rng.native_state_hex(),
        history["rng_before_hex"]["main"].as_str().unwrap()
    );
    (sim, rules, history)
}

#[test]
fn selection_after_terrain_prefix_advances_existing_main_without_changing_lockstep_state() {
    let (mut sim, rules, history) = prefixed_selection_fixture();
    let scenario_before = sim.scenario_rng.logical_state();
    let mapgen_before = sim.mapgen_rng.logical_state();
    let hash_before = sim.state_hash();
    for step in history["steps"].as_array().unwrap() {
        let main_before = sim.main_rng.logical_state();
        let event = selection_voice_event(&mut sim, &rules, 1, true)
            .expect("successful normal selection requests its voice");
        assert_ne!(
            sim.main_rng.logical_state(),
            main_before,
            "VoiceSelect must consume the existing terrain-advanced Simulation Main cursor"
        );
        assert_eq!(
            event.sound_id(),
            sound_name(step["voice_requests"][0]["sound_id"].as_i64().unwrap())
        );
        assert_eq!(sim.scenario_rng.logical_state(), scenario_before);
        assert_eq!(sim.mapgen_rng.logical_state(), mapgen_before);
        assert_eq!(sim.state_hash(), hash_before);
    }
    assert_eq!(
        sim.main_rng.native_state_hex(),
        history["rng_after_hex"]["main"].as_str().unwrap()
    );
    for expected in history["main_next_four"].as_array().unwrap() {
        assert_eq!(
            u64::from(sim.main_rng.next_u32()),
            expected.as_u64().unwrap()
        );
    }
}

#[test]
fn selection_main_continuation_survives_in_scenario_snapshot_handoff() {
    let (mut sim, rules, history) = prefixed_selection_fixture();
    let rules_hash = rules.simulation_config_hash();
    let bytes = GameSnapshot::save_validated(&sim, 0, rules_hash, "selection voice", 0);
    let steps = history["steps"].as_array().unwrap();
    for step in &steps[..2] {
        let event = selection_voice_event(&mut sim, &rules, 1, true).unwrap();
        assert_eq!(
            event.sound_id(),
            sound_name(step["voice_requests"][0]["sound_id"].as_i64().unwrap())
        );
    }
    let live_rng = sim.rng_state();
    let live_hash = sim.state_hash();
    let mut restored = GameSnapshot::load_validated(&bytes, 0, rules_hash, &sim.session.map_name)
        .unwrap()
        .sim;
    assert_ne!(
        restored.main_rng.logical_state(),
        live_rng.main,
        "snapshot bytes do not carry the process Main cursor"
    );
    // This is PreparedLoad's existing in-scenario process-state handoff.
    // It retains the current cursor, including voices after the saved frame;
    // Scenario's post-read reset remains independent of that continuation.
    restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(&sim));
    assert_eq!(restored.main_rng.logical_state(), live_rng.main);
    assert_eq!(restored.mapgen_rng.logical_state(), live_rng.mapgen);
    assert_eq!(
        sim.rng_state(),
        live_rng,
        "preparation must not mutate the outgoing owner"
    );
    assert_eq!(sim.state_hash(), live_hash);
    sim = restored;
    let scenario_before = sim.scenario_rng.logical_state();
    let mapgen_before = sim.mapgen_rng.logical_state();
    let hash_before = sim.state_hash();
    for step in &steps[2..] {
        let event = selection_voice_event(&mut sim, &rules, 1, true).unwrap();
        assert_eq!(
            event.sound_id(),
            sound_name(step["voice_requests"][0]["sound_id"].as_i64().unwrap())
        );
        assert_eq!(sim.scenario_rng.logical_state(), scenario_before);
        assert_eq!(sim.mapgen_rng.logical_state(), mapgen_before);
        assert_eq!(sim.state_hash(), hash_before);
    }
    assert_eq!(
        sim.main_rng.native_state_hex(),
        history["rng_after_hex"]["main"].as_str().unwrap()
    );
    for expected in history["main_next_four"].as_array().unwrap() {
        assert_eq!(
            u64::from(sim.main_rng.next_u32()),
            expected.as_u64().unwrap()
        );
    }
}
