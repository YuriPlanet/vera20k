use super::*;

#[test]
fn scene_without_voxels_reports_an_empty_unit_atlas() {
    assert_eq!(
        serde_json::to_value(unit_atlas_statistics(None).unwrap()).unwrap(),
        json!({"resident_sprite_count": 0, "last_build_rasterized_sprite_count": 0,
            "pages": [], "total_texel_payload_bytes": 0})
    );
}

fn initialized_map() -> MapObservation {
    MapObservation {
        initial: Some(json!({"tick": 0})),
        ..Default::default()
    }
}

fn times(ms: u64) -> GameRenderTimes {
    GameRenderTimes {
        radar_ms: ms,
        tooltip_ms: ms,
        message_ms: Some(ms),
    }
}

#[test]
fn crate_observation_preserves_occupied_slot_identity_without_mutating_simulation() {
    let mut sim = crate::sim::world::Simulation::new();
    *sim.crate_authority_mut_for_test().slot_mut_for_test(3) = crate::sim::crates::CrateSlot {
        start_frame: 50,
        aux: 777,
        duration: 100,
        cell_x: 10,
        cell_y: 12,
    };
    *sim.crate_authority_mut_for_test().slot_mut_for_test(7) = crate::sim::crates::CrateSlot {
        cell_x: -1,
        cell_y: 4,
        ..Default::default()
    };
    let before = sim.state_hash();
    let rows = MapCrateSlotObservation::capture(&sim);
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].slot, rows[0].cell), (3, [10, 12]));
    assert_eq!(
        (rows[0].start_frame, rows[0].aux, rows[0].duration),
        (50, 777, 100)
    );
    assert_eq!((rows[1].slot, rows[1].cell), (7, [-1, 4]));
    assert!(rows.iter().all(|row| row.overlay.is_none()));
    assert_eq!(sim.state_hash(), before);
}

#[test]
fn capture_retains_only_actual_draws_in_committed_order() {
    let mut zero = initialized_map();
    zero.observe_draw(0, 0, times(0)).unwrap();
    assert!(zero.observe_draw(0, 0, times(0)).is_err());
    let mut map = initialized_map();
    assert!(map.observe_draw(3, 0, times(0)).is_err());
    map.observe_draw(3, 1, times(22)).unwrap();
    assert!(map.observe_draw(3, 1, times(22)).is_err());
    assert!(map.observe_draw(3, 3, times(66)).is_err());
    map.observe_draw(3, 2, times(44)).unwrap();
    map.observe_draw(3, 3, times(66)).unwrap();
    let evidence = serde_json::to_value(&map.draws).unwrap();
    assert_eq!(evidence[0]["completed_steps"], 1);
    assert_eq!(evidence[2]["radar_ms"], 66);
    assert_eq!(map.draws.len(), 3);
    assert!(map.observe_draw(3, 4, times(88)).is_err());
}

#[test]
fn quickload_rewind_retains_actual_clock_without_repeating_capture_step_numbers() {
    let mut map = initialized_map();
    map.allow_load_segments = true;
    map.observe_draw(4, 1, times(22)).unwrap();
    map.observe_draw(4, 2, times(44)).unwrap();
    map.load_segments.push(MapLoadSegment {
        after_step: 2,
        gesture_ordinal: 1,
        before: MapClock {
            simulation_tick: 2,
            binary_frame: 2,
            total_simulation_ms: 44,
        },
        after: MapClock {
            simulation_tick: 1,
            binary_frame: 1,
            total_simulation_ms: 22,
        },
        restored_audio_state: None,
    });
    assert_eq!(map.expected_clock(2, false), (2, 2));
    assert_eq!(map.expected_clock(2, true), (1, 1));
    assert!(
        map.observe_draw(4, 3, times(66)).is_err(),
        "cannot conceal a load by using capture count as native time"
    );
    map.observe_draw(4, 3, times(44)).unwrap();
    map.observe_draw(4, 4, times(66)).unwrap();
    let rows = serde_json::to_value(&map.draws).unwrap();
    assert_eq!(rows[2]["completed_steps"], 3);
    assert_eq!(rows[2]["simulation_tick"], 2);
    assert_eq!(rows[3]["radar_ms"], 66);
}

#[test]
fn audio_frames_and_immediate_restore_charge_actors_and_both_rng_cursors() {
    let audio = json!({"main_rng_cursor": [0, 103], "scenario_rng_cursor": [0, 103],
        "actors": [{"stable_id": 7, "body_counter": 1, "active": true, "countdown": 2,
            "gattling_stage": 1, "gattling_value": 201}]});
    assert_eq!(sound_state_sample_count(&audio), 3);
    let mut historical = audio.clone();
    let row = historical["actors"][0].as_object_mut().unwrap();
    row.remove("gattling_stage");
    row.remove("gattling_value");
    assert_eq!(sound_state_sample_count(&historical), 3);
    let mut retained_requests = audio.clone();
    retained_requests["coordinate_requests"] = json!([{}, {}]);
    assert_eq!(sound_state_sample_count(&retained_requests), 5);
    for available in [2, 3] {
        let mut frames = MapObservation {
            sample_count: MAX_OBSERVATION_SAMPLES - available,
            ..Default::default()
        };
        let frame = frames.observe_frame(
            MapFrameObservation {
                completed_steps: 0,
                simulation_tick: 0,
                binary_frame: 0,
                total_simulation_ms: 0,
                actors: vec![],
                houses: vec![],
                missing_actor_ids: vec![],
                terrain: vec![],
                effects: None,
                input: None,
                lasers: None,
                electric_bolts: None,
                radiation_beams: None,
                bombs: None,
                crates: None,
                audio_state: Some(audio.clone()),
            },
            BTreeSet::new(),
        );
        let mut restored = MapObservation {
            sample_count: MAX_OBSERVATION_SAMPLES - available,
            ..Default::default()
        };
        let segment = restored.observe_load_segment(MapLoadSegment {
            after_step: 6,
            gesture_ordinal: 1,
            before: MapClock {
                simulation_tick: 6,
                binary_frame: 6,
                total_simulation_ms: 132,
            },
            after: MapClock {
                simulation_tick: 4,
                binary_frame: 4,
                total_simulation_ms: 88,
            },
            restored_audio_state: Some(audio.clone()),
        });
        assert_eq!(frame.is_ok(), available == 3);
        assert_eq!(segment.is_ok(), available == 3);
        let accepted = usize::from(available == 3);
        assert_eq!(frames.frames.len(), accepted);
        assert_eq!(restored.load_segments.len(), accepted);
        assert_eq!(
            frames.sample_count,
            MAX_OBSERVATION_SAMPLES - available + accepted * 3
        );
        assert_eq!(restored.sample_count, frames.sample_count);
    }
}

#[test]
fn coordinate_sound_scope_keeps_legacy_all_and_selects_only_watched_ids() {
    let config = json!({"sound_ids": ["RadarOn"], "max_events": 4,
        "max_samples_per_event": 262144, "completion_tail_ms": 4000});
    let mut audio: MapAudioProfile = serde_json::from_value(config.clone()).unwrap();
    assert_eq!(serde_json::to_value(&audio).unwrap(), config);
    let rows = vec![
        json!({"request_id": 1, "sound_id": "rAdArOn"}),
        json!({"request_id": 2, "sound_id": "Ambient"}),
        json!({"request_id": 3, "sound_id": null}),
    ];
    let mut retained = rows.clone();
    audio.retain_coordinate_requests(&mut retained);
    assert_eq!(retained, rows);
    audio.coordinate_request_scope = Some(MapCoordinateRequestScope::All);
    audio.retain_coordinate_requests(&mut retained);
    assert_eq!(retained, rows);
    audio.coordinate_request_scope = Some(MapCoordinateRequestScope::Watched);
    audio.retain_coordinate_requests(&mut retained);
    assert_eq!(retained, rows[..1]);
    for invalid in [Value::Null, json!(false), json!("unknown")] {
        let mut config = config.clone();
        config["coordinate_request_scope"] = invalid;
        assert!(serde_json::from_value::<MapAudioProfile>(config).is_err());
    }
}

#[test]
fn capture_rejects_missing_hud_update_or_any_wall_clock_sample() {
    assert!(
        MapObservation::default()
            .observe_draw(1, 1, times(22))
            .is_err()
    );
    let mut map = initialized_map();
    for sample in [
        GameRenderTimes {
            radar_ms: 9_876,
            ..times(22)
        },
        GameRenderTimes {
            tooltip_ms: 9_876,
            ..times(22)
        },
        GameRenderTimes {
            message_ms: Some(0),
            ..times(22)
        },
        GameRenderTimes {
            message_ms: None,
            ..times(22)
        },
    ] {
        assert!(map.observe_draw(1, 1, sample).is_err());
        assert!(
            map.draws.is_empty(),
            "rejected observation must not advance schedule"
        );
    }
    map.observe_draw(1, 1, times(22)).unwrap();
}

fn example() -> MapCaptureProfile {
    serde_json::from_str(crate::test_fixture::text(
        "tools/map_observation.example.json",
    ))
    .unwrap()
}

/// Every example profile parses into the one Command schema and passes
/// admission, so retiring a Command variant cannot strand a profile.
#[test]
fn every_example_profile_parses_and_validates() {
    let tools = crate::test_fixture::checkout_root().join("tools");
    let mut checked = 0;
    for entry in std::fs::read_dir(&tools).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !(name.starts_with("map_observation") && name.ends_with(".example.json")) {
            continue;
        }
        let profile: MapCaptureProfile =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
                .unwrap_or_else(|error| panic!("{name}: {error}"));
        profile
            .validate()
            .unwrap_or_else(|error| panic!("{name}: {error:#}"));
        checked += 1;
    }
    assert!(checked > 1, "found {checked} example profiles");
}

/// The presentation block takes only values the Options page can save.
#[test]
fn presentation_takes_only_options_values() {
    let mut profile = example();
    profile.cursor_position = None;
    profile.presentation = Some(MapPresentationProfile {
        zoom: 0,
        sidebar: 150,
        hd_radar: true,
        smooth_sidebar: true,
    });
    profile.validate().unwrap();
    for (zoom, sidebar) in [(125, 100), (100, 120)] {
        profile.presentation = Some(MapPresentationProfile {
            zoom,
            sidebar,
            hd_radar: false,
            smooth_sidebar: false,
        });
        assert!(profile.validate().is_err(), "{zoom} {sidebar}");
    }
}

#[test]
fn ordinary_capture_admission_uses_canonical_color_and_source_roster() {
    let mut profile = example();
    profile.launch.use_live_house_rows();
    profile.launch.local_slot_mut().color_index = 6;
    profile.validate().unwrap();
    let baseline = profile;
    let mut profile = baseline.clone();
    profile.launch.humans[0].raw_slot_index = Some(2);
    assert!(profile.validate().is_err());
    let mut profile = baseline.clone();
    profile.launch.humans[0].role = crate::skirmish_launch::LaunchHumanRole::Observer;
    assert!(profile.validate().is_err());
    let mut profile = baseline.clone();
    profile.launch.opponents[0].raw_slot_index = 3;
    assert!(profile.validate().is_err());
    let mut profile = baseline;
    let mut remote = profile.launch.humans[0].clone();
    remote.raw_slot_index = Some(2);
    profile.launch.humans.push(remote);
    assert!(profile.validate().is_err());
}

#[test]
fn bomb_and_fire_snapshots_read_retained_owners_without_advancing_state() {
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::{timer::CdTimer, world::Simulation};

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nIvanTimedDelay=450\n[InfantryTypes]\n0=IVAN\n\
         [VehicleTypes]\n0=HTNK\n[IVAN]\nStrength=125\n[HTNK]\nStrength=400\n",
    ))
    .unwrap();
    let mut sim = Simulation::new();
    let planter = sim
        .spawn_object_at_height("IVAN", "Player", 10, 10, 0, 0, &rules)
        .unwrap();
    let carrier = sim
        .spawn_object_at_height("HTNK", "Computer1", 11, 10, 0, 0, &rules)
        .unwrap();
    sim.session_mut_for_test().set_binary_frame_for_test(17);
    sim.bomb_attach_for_test(planter, Some(carrier), &rules);
    let entity = sim.entity_mut_for_test(planter).unwrap();
    entity.last_fire_frame.set_for_test(16);
    entity.body_frame_counter.set_for_test(u32::MAX);
    entity
        .seldom
        .rearm_timer
        .set_for_test(CdTimer::from_raw(16, 50));
    let hash = sim.state_hash();
    let rng = sim.rng_state();
    let snapshot = sim.bomb_observation(MAX_OBSERVATION_SAMPLES + 1);
    assert_eq!(snapshot.carriers, vec![carrier]);
    assert_eq!(snapshot.records.len(), 1);
    assert_eq!(snapshot.records[0].carrier_id, carrier);
    assert_eq!(snapshot.records[0].planter_id, Some(planter));
    assert_eq!(snapshot.records[0].planter_house, "Player");
    assert_eq!(snapshot.records[0].start_frame, 17);
    assert_eq!(snapshot.records[0].end_frame, 467);
    assert_eq!(bomb_sample_count(&snapshot), 3);
    assert_eq!(
        fire_observation(sim.entities().get(planter).unwrap(), 17),
        json!({"last_fire_frame": 16, "body_counter": u32::MAX, "stage_value": 0,
            "rearm": {"start_frame": 16, "duration": 50, "remaining": 49}})
    );
    assert_eq!(sim.state_hash(), hash);
    assert_eq!(sim.rng_state(), rng);
    sim.bomb_defuse_for_test(carrier);
    let retired = sim.bomb_observation(MAX_OBSERVATION_SAMPLES + 1);
    assert!(retired.carriers.is_empty());
    assert!(retired.records.is_empty());
}

#[test]
fn radiation_observer_preserves_composite_progress_geometry_hash_and_rng() {
    use crate::app::presentation::radiation_beams::RadiationBeams;
    use crate::render::surface_line::{SurfaceDraw, SurfaceLineBlend, SurfaceLineViewport};
    use crate::sim::{
        combat::radiation_beam::RadiationBeamBirth, projectile::ProjectileCoord, world::Simulation,
    };
    use crate::util::native_x87::NativeF64Bits;

    let sim = Simulation::new();
    let hash = sim.state_hash();
    let rng = sim.rng_state();
    let main = crate::sim::rng::MainRng::new(1).draws();
    let mut beams = RadiationBeams::default();
    beams.create(
        RadiationBeamBirth {
            frame: 7,
            from: ProjectileCoord::new(256, 256, 0),
            to: ProjectileCoord::new(768, 256, 0),
            z_adjust: -32,
            color: [0, 255, 0],
            house: None,
            house_color: crate::sim::combat::radiation_beam::BeamHouseColor::None,
            duration: 15,
            amplitude: NativeF64Bits::from_bits(40.0_f64.to_bits()),
            eruption: false,
        },
        |_, _| [0; 3],
    );
    let initial =
        serde_json::to_value(MapRadiationBeamObservation::capture(&beams).unwrap()).unwrap();
    assert_eq!(initial["live"][0]["progress"], 0);
    assert!(initial["live"][0]["length_bits"].is_null());
    let viewport = SurfaceLineViewport {
        camera: [0, 0],
        clip: [0, 0, 2048, 2048],
        z_origin_y: 0,
        zoom: 1.0,
    };
    // Actual presentation composites advance this owner without any Logic
    // tick. Numeric/native goldens belong to the renderer's own tests.
    for _ in 0..14 {
        beams.composite(60, viewport, &main, |_| true);
    }
    let draws = beams.draws().to_vec();
    assert!(!draws.is_empty());
    let retained = serde_json::to_value(beams.observation()).unwrap();
    let snapshot =
        serde_json::to_value(MapRadiationBeamObservation::capture(&beams).unwrap()).unwrap();
    assert_eq!(snapshot["live"][0]["progress"], 14);
    assert_eq!(snapshot["composites"][0]["progress_after"], 14);
    let SurfaceDraw::Line(line) = draws[0] else {
        panic!("expected actual beam line")
    };
    let SurfaceLineBlend::Replace(word) = line.blend else {
        panic!("expected packed beam color")
    };
    assert_eq!(
        snapshot["segments"][0],
        json!({
        "from": line.from, "to": line.to, "z_adjust": line.z_adjust, "rgb565": word})
    );
    assert_eq!(
        serde_json::to_value(MapRadiationBeamObservation::capture(&beams).unwrap()).unwrap(),
        snapshot
    );
    assert_eq!(serde_json::to_value(beams.observation()).unwrap(), retained);
    assert_eq!(beams.draws(), draws.as_slice());
    beams.composite(60, viewport, &main, |_| true);
    let expired =
        serde_json::to_value(MapRadiationBeamObservation::capture(&beams).unwrap()).unwrap();
    assert!(expired["live"].as_array().unwrap().is_empty());
    assert_eq!(expired["composites"][0]["progress_before"], 14);
    assert_eq!(expired["composites"][0]["progress_after"], 15);
    assert_eq!(expired["composites"][0]["expired"], true);
    assert!(!expired["segments"].as_array().unwrap().is_empty());
    assert_eq!(sim.state_hash(), hash);
    assert_eq!(sim.rng_state(), rng);
}

#[test]
fn cloak_runtime_snapshot_reads_timers_and_membership_without_advancing_state() {
    use crate::rules::retail_ini_fixture::retail_battle_rules;
    use crate::sim::cloak_disguise::CloakTickFacts;

    let Some(retail) = retail_battle_rules() else {
        return;
    };
    let mut sim = crate::sim::arena_fixture::rival_water_arena(&retail.rules, 0xC10A_0028, 64);
    let sub = sim
        .spawn_object("SUB", "Americans", 24, 24, 64, &retail.rules)
        .unwrap();
    // Exercise a running owner timer without modelling the world admission
    // predicate here. World gameplay tests cover that independent input.
    let mut rng = sim.scenario_rng_mut_for_test().clone();
    let cloak = sim
        .entity_mut_for_test(sub)
        .unwrap()
        .cloak
        .as_mut()
        .unwrap();
    cloak.tick(
        CloakTickFacts::new(17, true, true, false, true, 5, 18, 9, false, true, false),
        &mut rng,
    );
    *sim.scenario_rng_mut_for_test() = rng;
    let hash = sim.state_hash();
    let rng = sim.rng_state();
    let actor = sim.entities().get(sub).unwrap();
    let cloak = actor.cloak.as_deref().unwrap();
    let retained = *cloak;
    assert!(actor.in_playfield());
    assert_eq!(
        cloak_runtime_observation(cloak, actor.in_playfield(), 18),
        json!({"in_playfield": true, "step_delta_i32": 1, "step_speed_i32": 5,
            "step_timer": {"start_frame": 17, "duration": 5, "remaining": 4},
            "recloak_delay": {"start_frame": 0, "duration": 0, "remaining": 0}})
    );
    assert_eq!(*cloak, retained);
    assert_eq!(sim.state_hash(), hash);
    assert_eq!(sim.rng_state(), rng);
}

#[test]
fn temporal_snapshots_read_public_owners_without_advancing_state() {
    use crate::rules::retail_ini_fixture::retail_battle_rules;
    use crate::sim::{temporal::TemporalState, world::Simulation};

    let Some(retail) = retail_battle_rules() else {
        return;
    };
    let mut sim = Simulation::new();
    let fv = sim
        .spawn_object_at_height("FV", "Player", 10, 10, 0, 0, &retail.rules)
        .unwrap();
    let cleg = sim
        .spawn_object_at_height("CLEG", "Player", 10, 9, 0, 0, &retail.rules)
        .unwrap();
    let other = sim
        .spawn_object_at_height("E1", "Player", 9, 10, 0, 0, &retail.rules)
        .unwrap();
    let victim = sim
        .spawn_object_at_height("HTNK", "Computer1", 11, 10, 0, 0, &retail.rules)
        .unwrap();
    // A diagnostic fixture exercises independent cargo order and the
    // existing target-head fixture, without modelling gunner callbacks.
    let cargo = sim
        .entity_mut_for_test(fv)
        .unwrap()
        .passenger_role_mut_for_test()
        .cargo_mut()
        .unwrap();
    cargo.board_forced_chain(&[(cleg, 1.0)]);
    cargo.board_forced_chain(&[(other, 1.0)]);
    sim.entity_mut_for_test(victim)
        .unwrap()
        .set_temporal_for_test(TemporalState::warped_by_for_test(cleg));
    let hash = sim.state_hash();
    let rng = sim.rng_state();
    let snapshot = temporal_observation(sim.entities().get(fv).unwrap());
    assert_eq!(
        snapshot,
        json!({"has_link": false, "warp_target": null, "chain_head": null,
            "inside_transport": null,
            "cargo": {"count": 2, "passengers": [other, cleg]}})
    );
    assert_eq!(temporal_sample_count(&snapshot), 3);
    assert_eq!(temporal_sample_count(&Value::Null), 0);
    assert_eq!(
        temporal_observation(sim.entities().get(cleg).unwrap()),
        json!({"has_link": true, "warp_target": null, "chain_head": null,
            "inside_transport": null, "cargo": null})
    );
    let target = temporal_observation(sim.entities().get(victim).unwrap());
    assert_eq!(target["has_link"], false);
    assert_eq!(target["chain_head"], cleg);
    assert_eq!(sim.state_hash(), hash);
    assert_eq!(sim.rng_state(), rng);
}

#[test]
fn spawn_snapshots_preserve_current_type_and_ordered_pool_without_advancing_state() {
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::{
        combat::{Health, TargetKind},
        game_entity::GameEntity,
        spawn_manager::{SpawnManagerState, SpawnSlot, SpawnSlotState},
        timer::CdTimer,
        world::Simulation,
    };

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=CARRIER\n[CARRIER]\nStrength=800\nSpawnsNumber=3\n",
    ))
    .unwrap();
    let mut sim = Simulation::new();
    let owner = sim.intern("Player");
    let carrier_type = sim.intern("CARRIER");
    let hornet_type = sim.intern("HORNET");
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        1,
        10,
        10,
        0,
        0,
        owner,
        Health::new(800),
        carrier_type,
        crate::map::entities::EntityCategory::Unit,
        0,
        5,
        true,
    );
    assert_eq!(
        spawn_observation(&sim, &entity, &rules).unwrap(),
        Value::Null
    );
    assert_eq!(spawn_sample_count(&Value::Null), 0);
    let states = [
        SpawnSlotState::ReadyDocked,
        SpawnSlotState::Reloading,
        SpawnSlotState::InFlight,
        SpawnSlotState::KamikazeWait,
        SpawnSlotState::Attacking,
        SpawnSlotState::ComingHome,
        SpawnSlotState::Regenerating,
    ];
    // Supplied consumer priors: type capacity and retained node count are
    // deliberately different, and handles preserve the vector's order.
    let slots = states
        .iter()
        .enumerate()
        .map(|(index, &state)| {
            SpawnSlot::for_test(
                (state != SpawnSlotState::Regenerating).then_some(100 - index as u64),
                state,
                CdTimer::from_raw(-1, index as i32),
                state == SpawnSlotState::KamikazeWait,
            )
        })
        .collect();
    let mut manager = SpawnManagerState::for_test(hornet_type, slots);
    manager.set_rates_for_test(600, 150);
    manager.set_update_timer_for_test(CdTimer::from_raw(i32::MIN, 10));
    manager.set_target(Some(TargetKind::Entity(42)));
    entity.spawn_manager = Some(Box::new(manager.clone()));
    let hash = sim.state_hash();
    let rng = sim.rng_state();
    let snapshot = spawn_observation(&sim, &entity, &rules).unwrap();
    assert_eq!(snapshot["type_spawns_number"], 3);
    assert_eq!(snapshot["spawn_type"], "HORNET");
    assert_eq!(snapshot["mode"], "Idle");
    assert_eq!(snapshot["count_docked"], 2);
    assert_eq!(snapshot["count_alive"], 6);
    assert_eq!(snapshot["regen_rate"], 600);
    assert_eq!(snapshot["reload_rate"], 150);
    assert_eq!(snapshot["current_target"], Value::Null);
    assert_eq!(snapshot["queued_target"], json!({"Entity": 42}));
    assert_eq!(
        snapshot["update_timer"],
        json!({"start_frame": i32::MIN, "duration": 10})
    );
    assert_eq!(
        snapshot["spawn_timer"],
        json!({"start_frame": -1, "duration": 0})
    );
    assert_eq!(snapshot["slots"][0]["child"], 100);
    assert_eq!(snapshot["slots"][1]["child"], 99);
    assert_eq!(snapshot["slots"][6]["child"], Value::Null);
    for (index, state) in states.into_iter().enumerate() {
        assert_eq!(snapshot["slots"][index]["state"], json!(state));
        assert_eq!(
            snapshot["slots"][index]["timer"],
            json!({"start_frame": -1, "duration": index})
        );
        assert_eq!(snapshot["slots"][index]["is_missile"], index == 3);
    }
    assert_eq!(spawn_sample_count(&snapshot), 8);
    assert_eq!(entity.spawn_manager.as_deref(), Some(&manager));
    assert_eq!(sim.state_hash(), hash);
    assert_eq!(sim.rng_state(), rng);
}

#[test]
fn spawn_snapshots_charge_manager_and_slots_and_commit_only_within_budget() {
    for available in [4, 5] {
        let mut map = MapObservation {
            sample_count: MAX_OBSERVATION_SAMPLES - available,
            ..Default::default()
        };
        let result = map.observe_frame(
            MapFrameObservation {
                completed_steps: 0,
                simulation_tick: 0,
                binary_frame: 0,
                total_simulation_ms: 0,
                actors: vec![json!({"spawns": {"slots": [{}, {}, {}]}})],
                houses: vec![],
                missing_actor_ids: vec![],
                terrain: vec![],
                effects: None,
                lasers: None,
                electric_bolts: None,
                radiation_beams: None,
                bombs: None,
                crates: None,
                input: None,
                audio_state: None,
            },
            BTreeSet::new(),
        );
        assert_eq!(result.is_ok(), available == 5);
        assert_eq!(map.frames.len(), usize::from(available == 5));
        assert_eq!(
            map.sample_count,
            if available == 5 {
                MAX_OBSERVATION_SAMPLES
            } else {
                MAX_OBSERVATION_SAMPLES - available
            }
        );
    }
}

#[test]
fn wave_snapshot_reads_owner_and_geometry_without_advancing_state() {
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::{projectile::ProjectileCoord, wave::Wave, world::Simulation};
    let rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    let mut sim = Simulation::new();
    let first = ProjectileCoord {
        x: 1024,
        y: 1280,
        z: 0,
    };
    let second = ProjectileCoord {
        x: 2048,
        y: 1280,
        z: 0,
    };
    let wave = Wave::new(0, first, second);
    sim.waves_mut_for_test().spawn(41, wave);
    let mut profile = example();
    profile.schema_version = PROFILE_V2.to_owned();
    profile.observe_waves = Some(true);
    let hash = sim.state_hash();
    let rng = sim.rng_state();
    let snapshot = MapEffectObservation::capture(&sim, &rules, &profile).unwrap();
    assert_eq!(snapshot.sample_count(), 2);
    let rows = snapshot.waves.as_ref().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["stable_id"], 41);
    assert_eq!(rows[0]["source_leptons"], json!([1024, 1280, 0]));
    assert_eq!(rows[0]["lifetime"], Wave::DEFAULT_LIFETIME);
    assert_eq!(sim.state_hash(), hash);
    assert_eq!(sim.rng_state(), rng);
    profile.observe_waves = None;
    let absent = MapEffectObservation::capture(&sim, &rules, &profile).unwrap();
    assert!(serde_json::to_value(absent).unwrap().get("waves").is_none());
}

#[test]
fn effect_snapshot_reads_filtered_live_owner_without_advancing_state() {
    use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::{
        anim_class::AnimWorldCoord, components::AnimClassSpawnDescriptor, world::Simulation,
    };
    use crate::util::fixed_math::SimFixed;

    let mut rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    let mut art =
        ArtRegistry::from_ini(&IniFile::from_str("[TRAIL]\nRate=450\n[OTHER]\nRate=300\n"));
    art.bind_anim_frame_count_for_test("TRAIL", 15);
    art.bind_anim_frame_count_for_test("OTHER", 8);
    rules.replace_art_registry_for_test(art);
    let mut sim = Simulation::new();
    let at = AnimWorldCoord {
        x: 913,
        y: 1237,
        z: -17,
    };
    let mut selected = 0;
    for name in ["TRAIL", "OTHER"] {
        let kind = sim.intern(name);
        let mut descriptor =
            AnimClassSpawnDescriptor::new(kind, 0, 0, SimFixed::ZERO, SimFixed::ZERO, 0);
        descriptor.delay = 1;
        let id = sim.spawn_anim_at_world(&rules, descriptor, at).unwrap();
        if name == "TRAIL" {
            selected = id;
        }
    }
    let mut profile = example();
    profile.schema_version = PROFILE_V2.to_owned();
    profile.observe_projectiles = Some(true);
    profile.observe_anim_types = Some(vec!["trail".into()]);
    let hash = sim.state_hash();
    let rng = sim.rng_state();
    let snapshot = MapEffectObservation::capture(&sim, &rules, &profile).unwrap();
    assert_eq!(snapshot.sample_count(), 2);
    let rows = snapshot.animations.as_ref().unwrap();
    assert!(snapshot.projectiles.as_ref().unwrap().is_empty());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["stable_id"], selected);
    assert_eq!(rows[0]["stored_leptons"], json!([at.x, at.y, at.z]));
    assert_eq!(rows[0]["physical_leptons"], json!([at.x, at.y, at.z]));
    assert_eq!(rows[0]["owner_entity"], Value::Null);
    let live = sim.anims().find(|(id, _)| **id == selected).unwrap().1;
    assert_eq!(
        rows[0]["runtime"],
        serde_json::to_value(&live.runtime()).unwrap()
    );
    assert_eq!(sim.state_hash(), hash);
    assert_eq!(sim.rng_state(), rng);

    let mut observation = MapObservation {
        sample_count: MAX_OBSERVATION_SAMPLES - 1,
        ..Default::default()
    };
    assert!(
        observation
            .observe_frame(
                MapFrameObservation {
                    completed_steps: 0,
                    simulation_tick: 0,
                    binary_frame: 0,
                    total_simulation_ms: 0,
                    actors: vec![],
                    houses: vec![],
                    missing_actor_ids: vec![],
                    terrain: vec![],
                    effects: Some(snapshot),
                    input: None,
                    lasers: None,
                    electric_bolts: None,
                    radiation_beams: None,
                    bombs: None,
                    crates: None,
                    audio_state: None,
                },
                BTreeSet::new()
            )
            .is_err()
    );
    assert_eq!(observation.sample_count, MAX_OBSERVATION_SAMPLES - 1);
    assert!(observation.frames.is_empty());
}

#[test]
fn building_placement_projection_reads_actual_issuer_and_independent_saved_target() {
    // Observer serialization inputs only; these are not native placement
    // goldens or a replacement for a production loader witness.
    let mut input = crate::app::input::state::MatchInputState::new(
        crate::app::input::hotkeys::HotkeyBindings::default(),
    );
    let mut interner = crate::sim::intern::StringInterner::new();
    interner.intern("UnrelatedFirstEntry");
    let issuer = interner.intern("ActualIssuer");
    let empty = json!({
        "current": null, "saved_retry": null,
        "current_preview_present": false, "saved_preview_present": false
    });
    let observed = MapBuildingPlacementObservation::capture(&input, &interner);
    assert_eq!(observed.sample_count(), 1);
    assert_eq!(serde_json::to_value(observed).unwrap(), empty);

    assert!(input.begin_building_placement("GAPOWR", 20, issuer));
    input.submit_building_placement(
        &mut crate::app::sidebar_projection::SidebarProjectionState::default(),
    );
    assert!(input.begin_building_placement("GAPILL", 40, issuer));
    let observed = MapBuildingPlacementObservation::capture(&input, &interner);
    assert_eq!(observed.sample_count(), 3);
    let expected = json!({
        "current": {"type_id": "GAPILL", "product_id": 40, "issuer": "ActualIssuer"},
        "saved_retry": {"type_id": "GAPOWR", "product_id": 20, "issuer": "ActualIssuer"},
        "current_preview_present": false, "saved_preview_present": false
    });
    assert_eq!(serde_json::to_value(observed).unwrap(), expected);
    assert_eq!(
        serde_json::to_value(MapBuildingPlacementObservation::capture(&input, &interner)).unwrap(),
        expected,
        "observing does not consume the saved retry"
    );

    input.building_placement_preview = Some(crate::sim::production::BuildingPlacementPreview {
        rx: 106,
        ry: 34,
        width: 1,
        height: 1,
        valid: false,
        in_build_area: true,
        foundation_visible: true,
        reason: Some(crate::sim::production::BuildingPlacementError::BlockedTerrain),
        cells: vec![crate::sim::production::BuildingPlacementCell {
            coord: (106, 34),
            valid: false,
            level: -128,
            slope: 1,
        }],
        wall_autofill_cells: vec![],
    });
    let observed = MapBuildingPlacementObservation::capture(&input, &interner);
    assert_eq!(
        observed.sample_count(),
        5,
        "preview and cell consume bounded observation samples"
    );
    let value = serde_json::to_value(observed).unwrap();
    assert_eq!(
        value["current_preview"]["cells"],
        json!([{ "coord": [106,34], "valid": false, "level": -128, "slope": 1 }])
    );
    assert_eq!(value["current_preview"]["in_build_area"], true);
    assert_eq!(value["current_preview"]["foundation_visible"], true);
    assert!(
        input.building_placement_preview.is_some(),
        "observation retains the actual preview"
    );
    input.building_placement_preview = None;

    input.building_placement_pointer_expired(40);
    let orphan = MapBuildingPlacementObservation::capture(&input, &interner);
    assert_eq!(orphan.current.as_ref().unwrap().product_id, None);
    assert_eq!(orphan.current.as_ref().unwrap().type_id, "GAPILL");
    assert_eq!(orphan.saved_retry.as_ref().unwrap().product_id, Some(20));

    input.reset_targeting_for_new_world();
    let reset = MapBuildingPlacementObservation::capture(&input, &interner);
    assert_eq!(reset.sample_count(), 1);
    assert_eq!(serde_json::to_value(reset).unwrap(), empty);
}

#[test]
fn selection_scope_projection_and_hud_rows_serialize_and_charge_samples() {
    let mut scope = crate::app::types::TypeSelectInputState::default();
    let navigation =
        crate::app::input::dispatch::selection_navigation::CategoryNavigation::default();
    assert_eq!(
        serde_json::to_value(MapSelectionScopeObservation::capture(&scope, &navigation)).unwrap(),
        json!({"mode": "ordinary", "across_map": false, "last_outcome_key": null,
            "retained_navigation": [], "health_category": -1, "veterancy_category": -1})
    );
    scope.finish_tap(crate::app::types::TypeSelectOutcome::Map, true);
    let mut observed_scope = MapSelectionScopeObservation::capture(&scope, &navigation);
    // The sample limit counts every retained entry, including duplicates
    // that native733160 erases one at a time.
    observed_scope.retained_navigation = vec![7, 9, 7];
    let input = MapInputObservation {
        selected_ids: vec![7, 9],
        selection_pending: true,
        target_line_remaining: 0,
        target_line_active: false,
        planning_active: false,
        planning_paths: Vec::new(),
        local_input: Some(MapLocalInputObservation {
            camera_top_left: [0.0, 0.0],
            camera_zoom: 1.0,
            follow_target: None,
            repair_mode: false,
            sell_mode: false,
            targeting: None,
            building_placement: None,
            main_rng_cursor: [0, 103],
            selection_voice_enabled: true,
            selection_voice_requests: vec![MapSelectionVoiceRequest {
                speaker_id: 7,
                sound_id: "SelectVoice".into(),
            }],
            selection_scope: observed_scope,
            hud_messages: vec![
                "Units selected across SCREEN.".into(),
                "Units selected across MAP.".into(),
            ],
        }),
    };
    assert_eq!(
        input.sample_count(),
        9,
        "two selections, one voice, one scope, three retained entries and two HUD rows"
    );
    let observed = serde_json::to_value(&input).unwrap();
    assert!(observed["local_input"].get("building_placement").is_none());
    assert_eq!(
        observed["local_input"]["selection_scope"],
        json!({"mode": "type", "across_map": true, "last_outcome_key": "MSG:SelAcrossMap",
            "retained_navigation": [7, 9, 7], "health_category": -1, "veterancy_category": -1})
    );
    assert_eq!(
        observed["local_input"]["hud_messages"],
        json!([
            "Units selected across SCREEN.",
            "Units selected across MAP."
        ])
    );
    assert_eq!(
        scope.selection_scope_view(),
        ("type", true, Some("MSG:SelAcrossMap")),
        "read-only observation preserves the shared owner"
    );
}

#[test]
fn effect_step_whitelist_preserves_selected_boundaries_and_rejects_bad_schedules() {
    let mut profile = example();
    profile.schema_version = PROFILE_V2.to_owned();
    profile.ticks = 3;
    profile.observe_electric_bolts = Some(true);
    profile.validate().unwrap();
    assert!((0..=3).all(|step| profile.observes_electric_step(step)));
    profile.observe_electric_bolt_steps = Some(vec![0, 2]);
    profile.validate().unwrap();
    assert!(profile.observes_electric_step(0));
    assert!(!profile.observes_electric_step(1));
    assert!(profile.observes_electric_step(2));
    assert!(!profile.observes_electric_step(3));
    for steps in [vec![], vec![0, 0], vec![2, 1], vec![4]] {
        profile.observe_electric_bolt_steps = Some(steps);
        assert!(profile.validate().is_err());
    }
    profile.observe_electric_bolt_steps = Some(vec![0]);
    profile.observe_electric_bolts = Some(false);
    assert!(profile.validate().is_err());
    profile.observe_electric_bolts = None;
    assert!(profile.validate().is_err());
}

#[test]
fn gestures_require_sealed_neutral_cursor_and_strict_tactical_pixel_arguments() {
    let mut profile = example();
    profile.cursor_position = None;
    profile.gestures = Some(Vec::new());
    assert!(profile.validate().is_err());
    profile.cursor_position = Some([720, 556]);
    profile.validate().unwrap();
    let (width, height) =
        crate::ui::sidebar::SidebarFrame::native(profile.width, profile.height).tactical_size();
    profile.gestures = Some(vec![MapScheduledGesture {
        issue_after_step: 0,
        gesture: MapGesture::Click {
            position: [width - 2, height - 2],
            button: None,
            modifiers: None,
        },
    }]);
    profile.validate().unwrap();
    for position in [[0, 1], [1, 0], [width - 1, 1], [1, height - 1], [720, 556]] {
        profile.gestures.as_mut().unwrap()[0].gesture = MapGesture::Click {
            position,
            button: None,
            modifiers: None,
        };
        assert!(profile.validate().is_err(), "{position:?}");
    }
    profile.gestures.as_mut().unwrap()[0].gesture = MapGesture::Drag {
        from: [10, 10],
        to: [10, 10],
    };
    assert!(profile.validate().is_err());
    profile.gestures.as_mut().unwrap()[0].gesture = MapGesture::Drag {
        from: [10, 10],
        to: [100, 100],
    };
    profile.validate().unwrap();
    // A drag may end over the sidebar, but it starts on the battlefield.
    for (from, to, ok) in [
        ([10, 10], [720, 300], true),
        ([720, 300], [10, 10], false),
        ([10, 10], [profile.width - 1, 300], false),
    ] {
        profile.gestures.as_mut().unwrap()[0].gesture = MapGesture::Drag { from, to };
        assert_eq!(profile.validate().is_ok(), ok, "{from:?} -> {to:?}");
    }
    // A radar gesture uses the same ordinary mouse edges, but its live
    // production admission is outside the tactical rectangle.
    profile.gestures.as_mut().unwrap()[0].gesture = MapGesture::Minimap {
        position: [profile.width - 20, 100],
    };
    profile.validate().unwrap();
    for position in [[0, 1], [profile.width - 1, 100], [20, profile.height - 1]] {
        profile.gestures.as_mut().unwrap()[0].gesture = MapGesture::Minimap { position };
        assert!(profile.validate().is_err());
    }
    profile.gestures.as_mut().unwrap()[0].gesture = MapGesture::Drag {
        from: [10, 10],
        to: [100, 100],
    };
    let valid = serde_json::to_value(profile).unwrap();
    for gesture in [
        json!({"kind": "click", "position": [true, 1]}),
        json!({"kind": "click", "position": [1.0, 1]}),
        json!({"kind": "click", "position": [1, 1], "button": "middle"}),
        json!({"kind": "click", "position": [1, 1, 1]}),
        json!({"kind": "drag", "from": [1, 1]}),
        json!({"kind": "move", "position": [1, 1]}),
        Value::Null,
    ] {
        let mut invalid = valid.clone();
        invalid["gestures"][0]["gesture"] = gesture;
        assert!(serde_json::from_value::<MapCaptureProfile>(invalid).is_err());
    }
}

#[test]
fn sidebar_sampling_uses_completed_steps_across_quickload_rewind() {
    let mut profile = example();
    profile.schema_version = PROFILE_V2.to_owned();
    profile.ticks = 24;
    profile.allow_load_segments = Some(true);
    profile.observe_sidebar_steps = Some(vec![0, 1, 2, 4, 8, 9, 16, 17, 24]);
    profile.validate().unwrap();
    let mut map = initialized_map();
    map.allow_load_segments = true;
    map.observe_load_segment(MapLoadSegment {
        after_step: 16,
        gesture_ordinal: 3,
        before: MapClock {
            simulation_tick: 16,
            binary_frame: 16,
            total_simulation_ms: 352,
        },
        after: MapClock {
            simulation_tick: 4,
            binary_frame: 4,
            total_simulation_ms: 88,
        },
        restored_audio_state: None,
    })
    .unwrap();
    let mut sampled = Vec::new();
    for completed_steps in 0..=u64::from(profile.ticks) {
        let (simulation_tick, binary_frame) = map.expected_clock(completed_steps, false);
        let frame = MapFrameObservation {
            completed_steps,
            simulation_tick,
            binary_frame,
            total_simulation_ms: simulation_tick * MAP_PRESENTATION_INTERVAL_MS,
            actors: vec![],
            houses: vec![],
            missing_actor_ids: vec![],
            terrain: vec![],
            effects: None,
            input: None,
            lasers: None,
            electric_bolts: None,
            radiation_beams: None,
            bombs: None,
            crates: None,
            audio_state: None,
        };
        if profile.observes_sidebar_step(&frame) {
            sampled.push(completed_steps as u32);
        }
        map.observe_frame(frame, BTreeSet::new()).unwrap();
    }
    assert_eq!(map.frames[17].simulation_tick, 5);
    assert_eq!(map.frames[20].simulation_tick, 8);
    assert_eq!(
        sampled,
        profile.observe_sidebar_steps.unwrap(),
        "capture step17 must sample restored tick5; repeated tick8 at step20 must not sample again"
    );
}

#[test]
fn sidebar_target_resolution_reads_current_visible_slots_and_rejects_absent_controls() {
    let mut view = MapSidebarObservation {
        active_tab: "building",
        scroll_rows: 0,
        max_scroll_rows: 0,
        tabs: vec![MapSidebarTab {
            tab: "building",
            rect: [660.0, 170.0, 30.0, 20.0],
            active: true,
            disabled: false,
        }],
        items: vec![MapSidebarItem {
            slot: 3,
            type_id: "GAPOWR".to_owned(),
            super_weapon_section: None,
            display_name: "Grizzly Battle Tank".into(),
            rect: [720.0, 220.0, 60.0, 48.0],
            cost: Some(800),
            queue_category: "Building".to_owned(),
            enabled: false,
            progress: 0.0,
            queued_count: 0,
            is_building_this_type: false,
            is_ready: false,
            is_on_hold: false,
            is_armed: false,
            is_superweapon: false,
        }],
        scroll_up: MapSidebarControl {
            rect: [660.0, 570.0, 20.0, 20.0],
            disabled: true,
        },
        scroll_down: MapSidebarControl {
            rect: [690.0, 570.0, 20.0, 20.0],
            disabled: true,
        },
        mode_buttons: [
            MapSidebarToggle {
                rect: [660.0, 158.0, 20.0, 20.0],
                disabled: false,
                active: false,
            },
            MapSidebarToggle {
                rect: [700.0, 158.0, 20.0, 20.0],
                disabled: false,
                active: false,
            },
        ],
    };
    let target = MapSidebarTarget::Cameo {
        type_id: "GAPOWR".to_owned(),
    };
    // Build-limit grey is still a live cameo; its current visible slot is authoritative.
    assert_eq!(view.resolve(&target, [800, 600]).unwrap(), ([750, 244], 3));
    view.items[0].slot = 1;
    view.items[0].rect[1] = 180.0;
    assert_eq!(view.resolve(&target, [800, 600]).unwrap(), ([750, 204], 1));
    view.items[0].type_id = "GAPILE".to_owned();
    assert!(view.resolve(&target, [800, 600]).is_err());
    assert!(
        view.resolve(&MapSidebarTarget::ScrollDown {}, [800, 600])
            .is_err()
    );
    view.scroll_down.disabled = false;
    assert_eq!(
        view.resolve(&MapSidebarTarget::ScrollDown {}, [800, 600])
            .unwrap()
            .0,
        [700, 580]
    );
    assert!(
        view.resolve(&MapSidebarTarget::ScrollDown {}, [640, 480])
            .is_err()
    );
    assert_eq!(
        view.resolve(&MapSidebarTarget::Repair {}, [800, 600])
            .unwrap()
            .0,
        [670, 168]
    );
    assert_eq!(
        view.resolve(&MapSidebarTarget::Sell {}, [800, 600])
            .unwrap()
            .0,
        [710, 168]
    );
    view.mode_buttons[1].disabled = true;
    assert!(
        view.resolve(&MapSidebarTarget::Sell {}, [800, 600])
            .is_err()
    );
}

fn command_bar_fixture() -> (
    crate::ui::sidebar::command_bar::CommandBarLayout,
    Vec<Option<usize>>,
    crate::app::input::gadget_input::InGameGadgets,
) {
    use crate::ui::gadget::{
        GadgetRect,
        list::{GadgetSpec, ToggleKind},
    };
    let layout = crate::ui::sidebar::command_bar::CommandBarLayout::new(
        [800, 600],
        168,
        [28, 32],
        [52, 32],
        [28, 32],
        true,
    )
    .unwrap();
    // Prepared layout data for the diagnostic, not a native order golden.
    let index = command_bar_index("Guard").unwrap();
    let slots = vec![None, Some(index)];
    let rect = layout.slot(1).unwrap();
    let mut gadgets = crate::app::input::gadget_input::InGameGadgets::new();
    gadgets.command_bar = Some(std::array::from_fn(|i| {
        let handle = gadgets.list.add_tail(GadgetSpec::button(
            if i == index {
                GadgetRect::new(rect.x as i32, rect.y as i32, rect.w as i32, rect.h as i32)
            } else if i == 11 {
                let rect = layout.left_cap;
                GadgetRect::new(rect.x as i32, rect.y as i32, rect.w as i32, rect.h as i32)
            } else {
                GadgetRect::new(0, 0, 0, 0)
            },
            if i == 11 { 0xF0 } else { 0xD6 + i as u16 },
            ToggleKind::Plain,
        ));
        gadgets.list.get_mut(handle).unwrap().is_disabled = i != index && i != 11;
        handle
    }));
    (layout, slots, gadgets)
}

#[test]
fn command_bar_gesture_resolves_current_layout_and_retained_control() {
    let (layout, slots, mut gadgets) = command_bar_fixture();
    let (receipt, handle) =
        MapCommandBarGestureReceipt::resolve("Guard", layout, &slots, &gadgets, [800, 600])
            .unwrap();
    assert_eq!(receipt.slot, Some(1));
    assert_eq!(
        handle,
        gadgets.command_bar.unwrap()[command_bar_index("Guard").unwrap()]
    );
    let gadget = gadgets.list.get_mut(handle).unwrap();
    assert!(gadget.rect.contains(
        receipt.resolved_position[0] as i32,
        receipt.resolved_position[1] as i32
    ));
    assert_eq!(receipt.gadget_id, gadget.id);
    // Observed identity comes from the retained control, not a copied ID formula.
    gadget.id = 777;
    assert_eq!(
        MapCommandBarGestureReceipt::resolve("Guard", layout, &slots, &gadgets, [800, 600])
            .unwrap()
            .0
            .gadget_id,
        777
    );
    gadgets.list.get_mut(handle).unwrap().rect.x += 1;
    assert!(
        MapCommandBarGestureReceipt::resolve("Guard", layout, &slots, &gadgets, [800, 600])
            .is_err()
    );
}

#[test]
fn command_bar_thumb_uses_current_cap_without_inventing_a_button_list_slot() {
    use crate::ui::gadget::GadgetRect;
    for open in [true, false] {
        let (_, slots, mut gadgets) = command_bar_fixture();
        let layout = crate::ui::sidebar::command_bar::CommandBarLayout::new(
            [800, 600],
            168,
            [28, 32],
            [52, 32],
            [28, 32],
            open,
        )
        .unwrap();
        let handle = gadgets.command_bar.unwrap()[11];
        let gadget = gadgets.list.get_mut(handle).unwrap();
        let rect = layout.left_cap;
        gadget.rect = GadgetRect::new(rect.x as i32, rect.y as i32, rect.w as i32, rect.h as i32);
        gadget.id = if open { 0xF0 } else { 0xF1 };
        let (receipt, observed_handle) =
            MapCommandBarGestureReceipt::resolve("Thumb", layout, &slots, &gadgets, [800, 600])
                .unwrap();
        assert_eq!(observed_handle, handle);
        assert_eq!(receipt.slot, None);
        assert_eq!(receipt.gadget_id, if open { 0xF0 } else { 0xF1 });
        assert_eq!(
            receipt.resolved_position,
            if open { [18, 584] } else { [590, 584] }
        );
        let value = serde_json::to_value(receipt).unwrap();
        assert!(value.get("slot").is_none());
        assert!(
            value.get("thumb").is_none(),
            "pure resolution supplies no House observation"
        );
        gadgets.list.get_mut(handle).unwrap().is_disabled = true;
        assert!(
            MapCommandBarGestureReceipt::resolve("Thumb", layout, &slots, &gadgets, [800, 600],)
                .is_err()
        );
    }
    assert_eq!(crate::ui::sidebar::command_bar::COMMAND_NAMES.len(), 11);
    assert!(command_bar_index("thumb").is_err());
}

#[test]
fn actor_retask_snapshot_preserves_tagged_suspended_references() {
    use crate::sim::{combat::TargetKind, components::NavTargetRef, game_entity::GameEntity};
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 10, 20);
    entity
        .seldom
        .suspended_attack_target
        .set_for_test(Some(TargetKind::Cell(7, 8)));
    entity
        .navigation
        .set_suspended_nav_com_for_test(Some(NavTargetRef::object(42)));
    assert_eq!(
        retask_observation(&entity),
        json!({
            "suspended_target":{"Cell":[7,8]}, "suspended_nav":{"Object":{"id":42}}
        })
    );
    entity
        .seldom
        .suspended_attack_target
        .set_for_test(Some(TargetKind::Entity(43)));
    entity
        .navigation
        .set_suspended_nav_com_for_test(Some(NavTargetRef::cell(9, 10)));
    assert_eq!(
        retask_observation(&entity),
        json!({
            "suspended_target":{"Entity":43}, "suspended_nav":{"Cell":{"rx":9,"ry":10}}
        })
    );
    entity.seldom.suspended_attack_target.set_for_test(None);
    entity.navigation.set_suspended_nav_com_for_test(None);
    assert_eq!(
        retask_observation(&entity),
        json!({"suspended_target":null,"suspended_nav":null})
    );
}

#[test]
fn gesture_schedule_retains_equal_step_order_and_conditional_receipt_presence() {
    let mut profile = example();
    let mut map = initialized_map();
    assert!(map.transcript(&profile).get("gesture_input").is_none());
    profile.schema_version = PROFILE_V2.to_owned();
    profile.cursor_position = Some([720, 556]);
    profile.ticks = 3;
    profile.gestures = Some(vec![
        MapScheduledGesture {
            issue_after_step: 0,
            gesture: MapGesture::Click {
                position: [10, 10],
                button: None,
                modifiers: None,
            },
        },
        MapScheduledGesture {
            issue_after_step: 0,
            gesture: MapGesture::Drag {
                from: [20, 20],
                to: [100, 100],
            },
        },
        MapScheduledGesture {
            issue_after_step: 2,
            gesture: MapGesture::Click {
                position: [30, 30],
                button: None,
                modifiers: None,
            },
        },
    ]);
    profile.validate().unwrap();
    assert!(map.pending_gesture(&profile, 1).is_err());
    let observed = || MapInputObservation {
        selected_ids: Vec::new(),
        selection_pending: false,
        target_line_remaining: 0,
        target_line_active: false,
        planning_active: false,
        planning_paths: Vec::new(),
        local_input: None,
    };
    for ordinal in 0..2 {
        let scheduled = map.pending_gesture(&profile, 0).unwrap().unwrap().clone();
        assert_eq!(
            serde_json::to_value(&scheduled.gesture).unwrap(),
            serde_json::to_value(&profile.gestures()[ordinal].gesture).unwrap()
        );
        map.gestures.push(MapGestureReceipt {
            ordinal,
            issue_after_step: 0,
            issued_simulation_tick: 0,
            issued_binary_frame: 0,
            band_box_before_release: matches!(scheduled.gesture, MapGesture::Drag { .. }),
            gesture: scheduled.gesture,
            before: observed(),
            after: observed(),
            left_press_captured: true,
            right_press_captured: None,
            neutral_input_restored: true,
            queued_commands: Vec::new(),
            sidebar: None,
            keyboard: None,
            command_bar: None,
            minimap: None,
            held_cursor: None,
        });
    }
    assert!(map.pending_gesture(&profile, 1).unwrap().is_none());
    assert_eq!(
        map.pending_gesture(&profile, 2)
            .unwrap()
            .unwrap()
            .issue_after_step,
        2
    );
    assert!(map.pending_gesture(&profile, 3).is_err());
    let transcript = map.transcript(&profile);
    assert_eq!(transcript["gesture_input"]["policy"], GESTURE_POLICY);
    assert_eq!(
        transcript["gesture_input"]["equal_step_order"],
        "commands_then_gestures"
    );
    assert!(
        transcript["gesture_input"]["receipts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|receipt| receipt.get("right_press_captured").is_none())
    );
    assert_eq!(
        transcript["gesture_input"]["receipts"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    profile.gestures.as_mut().unwrap()[2].issue_after_step = 3;
    assert!(profile.validate().is_err());
    profile.gestures.as_mut().unwrap()[0].issue_after_step = 1;
    assert!(profile.validate().is_err());
    let row = profile.gestures()[1].clone();
    profile.gestures = Some(vec![row; MAX_GESTURES + 1]);
    assert!(profile.validate().is_err());
}

#[test]
fn type_discovery_preserves_retained_identity_and_optional_transcript_presence() {
    let mut profile = example();
    profile.schema_version = PROFILE_V2.to_owned();
    profile.observe_owners = Some(vec!["Computer1".to_owned()]);
    profile.observe_types = Some(vec!["MTNK".to_owned(), "CLEG".to_owned()]);
    let mut map = initialized_map();
    assert!(map.observes_actor(&profile, 7, "Computer1", "CLEG"));
    assert!(!map.observes_actor(&profile, 8, "Computer2", "CLEG"));
    assert!(!map.observes_actor(&profile, 9, "Computer1", "GAPOWR"));
    assert!(!map.observes_actor(&profile, 10, "Computer1", "cleg"));
    map.observe_frame(
        MapFrameObservation {
            completed_steps: 0,
            simulation_tick: 0,
            binary_frame: 0,
            total_simulation_ms: 0,
            actors: vec![json!({"stable_id": 7, "owner": "Computer1", "type_id": "CLEG"})],
            houses: Vec::new(),
            missing_actor_ids: Vec::new(),
            terrain: Vec::new(),
            effects: None,
            input: None,
            lasers: None,
            electric_bolts: None,
            radiation_beams: None,
            bombs: None,
            crates: None,
            audio_state: None,
        },
        BTreeSet::from([7]),
    )
    .unwrap();
    assert!(map.observes_actor(&profile, 7, "Computer2", "GAPOWR"));
    map.observe_frame(
        MapFrameObservation {
            completed_steps: 1,
            simulation_tick: 1,
            binary_frame: 1,
            total_simulation_ms: 22,
            actors: Vec::new(),
            houses: Vec::new(),
            missing_actor_ids: vec![7],
            terrain: Vec::new(),
            effects: None,
            input: None,
            lasers: None,
            electric_bolts: None,
            radiation_beams: None,
            bombs: None,
            crates: None,
            audio_state: None,
        },
        BTreeSet::from([7]),
    )
    .unwrap();
    assert!(map.observes_actor(&profile, 7, "Computer2", "GAPOWR"));
    let transcript = map.transcript(&profile);
    assert_eq!(transcript["type_filter"], json!(["MTNK", "CLEG"]));
    assert_eq!(transcript["frames"][1]["missing_actor_ids"], json!([7]));
    profile.observe_types = None;
    assert!(map.observes_actor(&profile, 9, "Computer1", "GAPOWR"));
    assert!(!map.observes_actor(&profile, 8, "Computer2", "CLEG"));
    assert!(map.transcript(&profile).get("type_filter").is_none());
}

#[test]
fn command_schedule_keeps_equal_step_input_order_and_refuses_missed_or_final_steps() {
    let mut profile = example();
    profile.schema_version = PROFILE_V2.to_owned();
    profile.ticks = 3;
    profile.commands = Some(vec![
        MapScheduledCommand {
            issue_after_step: 0,
            owner: "Computer1".to_owned(),
            payload: Command::Stop { entity_id: 1 },
            symbolic: None,
        },
        MapScheduledCommand {
            issue_after_step: 0,
            owner: "Computer1".to_owned(),
            payload: Command::DeployMcv { entity_id: 2 },
            symbolic: None,
        },
        MapScheduledCommand {
            issue_after_step: 2,
            owner: "Computer1".to_owned(),
            payload: Command::ForceAttackCell {
                attacker_id: 3,
                target_rx: 87,
                target_ry: 53,
            },
            symbolic: None,
        },
    ]);
    profile.validate().unwrap();
    let mut map = initialized_map();
    assert!(map.pending_command(&profile, 1).is_err());
    for ordinal in 0..2 {
        let command = map.pending_command(&profile, 0).unwrap().unwrap().clone();
        assert_eq!(command.payload, profile.commands()[ordinal].payload);
        map.commands.push(MapCommandReceipt {
            ordinal,
            issue_after_step: 0,
            issued_simulation_tick: 0,
            envelope_execute_tick: 0,
            owner: command.owner,
            payload: command.payload,
            bindings: None,
        });
    }
    assert!(map.pending_command(&profile, 1).unwrap().is_none());
    assert_eq!(
        map.pending_command(&profile, 2)
            .unwrap()
            .unwrap()
            .issue_after_step,
        2
    );
    assert!(map.pending_command(&profile, 3).is_err());
    profile.commands.as_mut().unwrap()[2].issue_after_step = 3;
    assert!(profile.validate().is_err());
    profile.commands.as_mut().unwrap()[0].issue_after_step = 1;
    assert!(profile.validate().is_err());
}

/// Every profile kept under `tools/` loads through the production reader,
/// so a change to a command's wire shape has to migrate them. Each one
/// names its schema in its first key.
#[test]
fn stored_profiles_load() {
    fn walk(dir: &Path, profiles: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, profiles);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let mut head = [0; 128];
                let read = std::io::Read::read(&mut std::fs::File::open(&path).unwrap(), &mut head)
                    .unwrap();
                let tag = b"\"schema_version\": \"vera20k.map-observation-profile.";
                if head[..read].windows(tag.len()).any(|window| window == tag) {
                    profiles.push(path);
                }
            }
        }
    }
    let mut profiles = Vec::new();
    // The reader refuses linked ancestors; a checkout may sit behind one.
    let tools = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("tools"));
    walk(&tools.unwrap(), &mut profiles);
    assert!(!profiles.is_empty());
    for path in profiles {
        let profile = MapCaptureProfile::load(&path)
            .unwrap_or_else(|error| panic!("{}: {error:#}", path.display()));
        let source: Value = serde_json::from_slice(&profile.bytes).unwrap();
        // These maintained examples use the canonical serializer shape;
        // legacy inputs remain supported only at the decoder boundary.
        assert_eq!(
            source["launch"],
            serde_json::to_value(&profile.value.launch).unwrap(),
            "{}",
            path.display()
        );
    }
}

#[test]
fn observations_require_l0_then_every_committed_frame_and_bound_retained_samples() {
    let frame = |step, tick| MapFrameObservation {
        completed_steps: step,
        simulation_tick: tick,
        binary_frame: tick as u32,
        total_simulation_ms: tick * 22,
        actors: Vec::new(),
        houses: Vec::new(),
        missing_actor_ids: Vec::new(),
        terrain: Vec::new(),
        effects: None,
        input: None,
        lasers: None,
        electric_bolts: None,
        radiation_beams: None,
        bombs: None,
        crates: None,
        audio_state: None,
    };
    let mut map = initialized_map();
    assert!(map.observe_frame(frame(1, 1), BTreeSet::new()).is_err());
    map.observe_frame(frame(0, 0), BTreeSet::new()).unwrap();
    assert!(map.observe_frame(frame(0, 0), BTreeSet::new()).is_err());
    assert!(map.observe_frame(frame(1, 2), BTreeSet::new()).is_err());
    map.observe_frame(frame(1, 1), BTreeSet::new()).unwrap();
    let mut large = frame(2, 2);
    large.terrain = vec![Value::Null; MAX_OBSERVATION_SAMPLES + 1];
    assert!(map.observe_frame(large, BTreeSet::new()).is_err());
    let mut large = frame(2, 2);
    large.input = Some(MapInputObservation {
        selected_ids: (1..=MAX_OBSERVATION_SAMPLES as u64 + 1).collect(),
        selection_pending: false,
        target_line_remaining: 0,
        target_line_active: false,
        planning_active: false,
        planning_paths: Vec::new(),
        local_input: None,
    });
    assert!(map.observe_frame(large, BTreeSet::new()).is_err());
    assert_eq!(map.frames.len(), 2);
    assert_eq!(map.sample_count, 0);
}
