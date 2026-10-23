//! Physical retail rules/ART/map load and the ordinary Engineer input route.
//!
//! The already admitted Building's damaged prior comes from the native joined
//! witness. Its constructor/placement, full native world clock, mouse picking,
//! native event-byte codec and audio device output are outside this test.
//! Decisions, queue/drain, Walk arrival, repair/art/sound request, House update
//! and deferred Engineer retirement run through the bound production runtime.
//! The CAMOV02 case instead damages an original map building with stock tank
//! fire before repair; its whole-world timing is a Rust composition check.

use super::*;
use crate::app::input::cursor::{
    ActionDistanceTarget, capability_cursor_for_hover, cursor_id_for_feedback,
    select_best_for_action,
};
use crate::app::types::{CursorFeedbackKind, CursorId};
use crate::rules::ruleset::RuleSet;
use crate::sim::world::{EngineerBuildingAction, SimSoundEvent, Simulation, TickLane};
use serde_json::{Value, json};

fn building_orders(
    sim: &Simulation,
    rules: &RuleSet,
    engineer: u64,
    building: u64,
    repairable: bool,
) -> Vec<CommandEnvelope> {
    building_orders_for_hover(
        sim,
        rules,
        engineer,
        building,
        repairable,
        HoverTargetKind::FriendlyStructure,
    )
}

fn building_orders_for_hover(
    sim: &Simulation,
    rules: &RuleSet,
    engineer: u64,
    building: u64,
    repairable: bool,
    hover_kind: HoverTargetKind,
) -> Vec<CommandEnvelope> {
    let hover = HoverTargetKindWithId {
        kind: hover_kind,
        stable_id: building,
    };
    let original_selection = [engineer];
    let prior = sim.state_hash();
    let best = select_best_for_action(
        sim,
        &original_selection,
        ActionDistanceTarget::Object(building),
        Some(rules),
    );
    assert_eq!(best, Some(engineer));
    assert_eq!(
        sim.engineer_building_action(engineer, building, rules),
        Some(EngineerBuildingAction::Repair(repairable))
    );
    let feedback = capability_cursor_for_hover(
        sim,
        &original_selection,
        best,
        &hover,
        Some(rules),
        None,
        crate::app::input::context_order::OrderModifier::Normal,
        false,
        false,
    );
    assert_eq!(feedback, CursorFeedbackKind::RepairAction(repairable));
    assert_eq!(
        cursor_id_for_feedback(feedback),
        Some(if repairable {
            CursorId::Repair
        } else {
            CursorId::NoRepair
        })
    );
    let mut selected = original_selection.to_vec();
    let commands = engineer_capture_orders(sim, rules, &mut selected, &hover).unwrap();
    assert!(selected.is_empty());
    assert_eq!(commands.len(), usize::from(repairable));
    let owner = sim.entities().get(engineer).unwrap().owner();
    let envelopes = commands
        .into_iter()
        .map(|payload| CommandEnvelope::new(owner, sim.session().tick(), payload))
        .map(|envelope| {
            crate::app::input::commands::roundtrip_ordinary_local_megamission(sim, envelope)
                .unwrap()
        })
        .collect();
    assert_eq!(
        sim.state_hash(),
        prior,
        "input decision must preserve state/RNG"
    );
    envelopes
}

fn advance_retail_frame(runtime: &mut crate::sim::runtime::SimRuntime) -> Vec<SimSoundEvent> {
    let due = runtime.simulation.take_due_commands();
    runtime
        .advance_frame(
            &due,
            crate::app::types::SIM_TICK_MS,
            TickLane::Ordinary,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap()
        .sound_events
}

fn assert_retail_anim_advances(runtime: &mut crate::sim::runtime::SimRuntime, anim: u64) {
    let first_frame = runtime
        .simulation
        .anim(anim)
        .unwrap()
        .runtime()
        .current_frame;
    for _ in 0..60 {
        advance_retail_frame(runtime);
        if runtime
            .simulation
            .anim(anim)
            .unwrap()
            .runtime()
            .current_frame
            != first_frame
        {
            return;
        }
    }
    panic!("stock animation {anim} did not advance through ordinary frames");
}

#[test]
#[ignore = "physical XInvasion, Urban ART binding, tank damage and Engineer Walk repair"]
fn retail_camov02_alias_binds_damaged_art_and_engineer_repair_restores_normal() {
    let (retail, _) = crate::rules::retail_ini_fixture::retail_assets()
        .expect("run with RA2_DIR and VERA20K_REQUIRE_RETAIL_ASSETS=1");
    let mut launch = crate::headless_scenario::battle_session("XInvasion.map", &[]);
    launch.local_slot_mut().start_position =
        crate::skirmish_launch::LaunchStartPosition::Position(0);
    launch.options.unit_count = 0;
    launch.options.bases = true;
    launch.options.crates = false;
    launch.options.short_game = false;
    launch.options.multi_engineer = false;
    launch.options.shroud = false;
    launch.options.fog_of_war = false;
    // Resolve the unchanged retail map from archives. Its Neutral roster,
    // terrain and ambient triggers remain; the lobby supplies the human AMCV.
    let mut scene =
        crate::headless_scenario::load_session(&retail, "XInvasion.map", 0x427A_80, launch)
            .expect("physical Urban XInvasion loads through production owners");
    let runtime = &mut scene.runtime;
    let rules = &runtime.resources.rules;
    assert_eq!(rules.object("CAMOV02").unwrap().strength, 1000);
    assert!(rules.object("CAMOV02").unwrap().can_be_occupied);
    assert!(rules.object("CAMOV02").unwrap().repairable);
    assert_eq!(rules.general.condition_yellow, 0.5);
    // Original AnimType ReadINI427D00 and theater postpass427A80 bind both
    // sections to CUMOV02_A.SHP (8 raw frames). AD inherits NewTheater from
    // Image=CAMOV02_A while retaining its own damaged animation bounds.
    for (name, start, loop_start, loop_end) in [("CAMOV02_A", 0, 0, 2), ("CAMOV02_AD", 2, 2, 4)] {
        let config = rules.art().anim_runtime_config(name).unwrap();
        assert_eq!(config.raw_shp_frame_count, Some(8), "{name} must bind");
        assert_eq!(
            (
                config.start,
                config.end,
                config.loop_start,
                config.loop_end,
                config.loop_count
            ),
            (start, 4, loop_start, loop_end, -1),
            "{name} keeps its own timing"
        );
        let image = rules.art().resolve_anim_image_id(name);
        assert_eq!(
            crate::rules::art_data::effect_shp_candidates(
                crate::rules::art_data::EffectImageKind::Animation,
                Some(rules.art()),
                name,
                &image,
                "urb",
                "URBAN"
            )[0],
            "CUMOV02_A.SHP"
        );
    }
    let building = runtime
        .simulation
        .entities()
        .iter_sorted()
        .find_map(|(id, entity)| {
            (runtime.simulation.resolve(entity.type_ref()) == "CAMOV02"
                && (entity.position.rx(), entity.position.ry()) == (35, 61))
                .then_some(id)
        })
        .expect("original XInvasion structure record1025810");
    let target = runtime.simulation.entities().get(building).unwrap();
    let neutral = target.owner();
    assert_eq!(runtime.simulation.resolve(neutral), "Neutral");
    assert!(runtime.simulation.houses()[&neutral].multiplay_passive());
    assert_eq!(target.health.current(), 1000);
    assert!(target.is_object_alive() && !target.in_limbo());
    let normal_slot = target.building_art.anim_slots()[3].unwrap();
    assert_eq!(
        runtime
            .simulation
            .resolve(runtime.simulation.anim(normal_slot).unwrap().type_id()),
        "CAMOV02_A"
    );
    let owner = runtime.simulation.session().current_house().unwrap();
    assert!(runtime.simulation.houses()[&owner].is_human());
    let owner_name = runtime.simulation.resolve(owner).to_owned();
    // Only helper placement is supplied. Damage, animation replacement, Walk
    // arrival and repair use their ordinary production transactions.
    let tank = runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "MTNK",
            &owner_name,
            31,
            60,
            0,
            rules,
            &runtime.resources.overlay_registry,
        )
        .unwrap();
    let engineer = runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "ENGINEER",
            &owner_name,
            32,
            61,
            0,
            rules,
            &runtime.resources.overlay_registry,
        )
        .unwrap();
    for (id, cell) in [(tank, (31, 60)), (engineer, (32, 61))] {
        let entity = runtime.simulation.entities().get(id).unwrap();
        assert!(entity.is_object_alive() && !entity.in_limbo());
        assert_eq!((entity.position.rx(), entity.position.ry()), cell);
    }
    assert_retail_anim_advances(runtime, normal_slot);
    let attack = crate::app::input::commands::roundtrip_ordinary_local_megamission(
        &runtime.simulation,
        CommandEnvelope::new(
            owner,
            runtime.simulation.session().tick(),
            Command::Attack {
                attacker_id: tank,
                target_id: building,
            },
        ),
    )
    .unwrap();
    runtime.simulation.queue_commands(vec![attack]);
    let mut crossed_yellow = false;
    for _ in 0..1200 {
        advance_retail_frame(runtime);
        let target = runtime.simulation.entities().get(building).unwrap();
        assert!(
            target.health.current() > 0,
            "stop fire before destroying the building"
        );
        if target.health.current() <= 500 {
            crossed_yellow = true;
            break;
        }
    }
    assert!(
        crossed_yellow,
        "ordinary stock MTNK fire must damage CAMOV02"
    );
    runtime.simulation.queue_commands(vec![CommandEnvelope::new(
        owner,
        runtime.simulation.session().tick(),
        Command::Stop { entity_id: tank },
    )]);
    let mut quiescent = false;
    for _ in 0..240 {
        advance_retail_frame(runtime);
        if runtime.simulation.projectiles().is_empty()
            && runtime
                .simulation
                .entities()
                .get(tank)
                .unwrap()
                .attack_target
                .target()
                .is_none()
        {
            quiescent = true;
            break;
        }
    }
    assert!(
        quiescent,
        "Stop must clear the attack and launched projectiles"
    );
    let target = runtime.simulation.entities().get(building).unwrap();
    let damaged_hp = target.health.current();
    assert!(damaged_hp > 0 && damaged_hp <= 500);
    assert!(target.building_art.damage_state_active());
    let damaged_slot =
        target.building_art.anim_slots()[3].expect("bound damaged slot survives allocation");
    assert_ne!(damaged_slot, normal_slot);
    assert!(runtime.simulation.anim(normal_slot).is_none());
    assert_eq!(
        runtime
            .simulation
            .resolve(runtime.simulation.anim(damaged_slot).unwrap().type_id()),
        "CAMOV02_AD"
    );
    assert_retail_anim_advances(runtime, damaged_slot);
    assert_eq!(
        runtime
            .simulation
            .entities()
            .get(building)
            .unwrap()
            .health
            .current(),
        damaged_hp
    );
    let engineer_start = crate::sim::movement::ground_pose::position_world_xy(
        &runtime
            .simulation
            .entities()
            .get(engineer)
            .unwrap()
            .position,
    );
    let orders = building_orders_for_hover(
        &runtime.simulation,
        &runtime.resources.rules,
        engineer,
        building,
        true,
        HoverTargetKind::EnemyStructure,
    );
    runtime.simulation.queue_commands(orders);
    let mut walked = false;
    let mut repaired_sound = 0;
    for _ in 0..600 {
        let sounds = advance_retail_frame(runtime);
        repaired_sound += sounds.iter().filter(|event| {
            matches!(event, SimSoundEvent::VocAt { sound_id, .. } if sound_id == "BuildingRepaired")
        }).count();
        if let Some(entity) = runtime.simulation.entities().get(engineer) {
            walked |= crate::sim::movement::ground_pose::position_world_xy(&entity.position)
                != engineer_start;
        }
        if !runtime.simulation.entities().contains(engineer) {
            break;
        }
    }
    assert!(
        walked,
        "Engineer must reach the map building through ordinary Walk"
    );
    assert!(
        !runtime.simulation.entities().contains(engineer),
        "repair retires Engineer"
    );
    assert_eq!(repaired_sound, 1);
    let target = runtime.simulation.entities().get(building).unwrap();
    assert_eq!(
        target.owner(),
        neutral,
        "passive occupation repair is not capture"
    );
    assert_eq!(target.health.current(), 1000);
    assert_eq!(target.estimated_health.get(), 1000);
    assert!(!target.repairing.is_on());
    assert!(!target.building_art.damage_state_active());
    let repaired_slot = target.building_art.anim_slots()[3].unwrap();
    assert_ne!(repaired_slot, damaged_slot);
    assert!(runtime.simulation.anim(damaged_slot).is_none());
    assert_eq!(
        runtime
            .simulation
            .resolve(runtime.simulation.anim(repaired_slot).unwrap().type_id()),
        "CAMOV02_A"
    );
    assert_retail_anim_advances(runtime, repaired_slot);
}

#[test]
#[ignore = "physical AnyTown, retail rules/ART/audio binding and Walk repair composition"]
fn retail_building_repair_input_walk_art_sound_house_and_retirement() {
    let retail = std::path::PathBuf::from(std::env::var_os("RA2_DIR").unwrap());
    let profile: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/map_observation.building-opening.example.json",
    ))
    .unwrap();
    let launch = serde_json::from_value(profile["launch"].clone()).unwrap();
    let descriptor =
        crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(launch).unwrap();
    let mut scene =
        crate::headless_scenario::load_with_launch(&retail, "XMP03T4.MAP", 0x1234_5678, descriptor)
            .unwrap();
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/engineer_repair_joined.json",
    ))
    .unwrap();
    let row = native["routes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "own_damaged")
        .unwrap();
    let before = &row["arrivals"][0]["before"]["building"];
    let after = &row["arrivals"][0]["after"]["building"];
    let runtime = &mut scene.runtime;
    let owner = runtime.simulation.session().current_house().unwrap();
    let owner_name = runtime.simulation.resolve(owner).to_owned();
    let rules = &runtime.resources.rules;
    assert_eq!(rules.object("GAPOWR").unwrap().strength, 750);
    assert_eq!(rules.object("GAPOWR").unwrap().power, 200);
    assert!(rules.object("ENGINEER").unwrap().engineer);
    assert_eq!(
        rules.general.building_repaired_sound.as_deref(),
        Some("BuildingRepaired")
    );
    let plant = runtime
        .simulation
        .spawn_object("GAPOWR", &owner_name, 31, 89, 0, rules)
        .expect("retained ordinary construction production placement site");
    runtime
        .simulation
        .entities_mut_for_test()
        .get_mut(plant)
        .unwrap()
        .finish_building_construction_for_test();
    runtime.simulation.grand_opening(
        plant,
        false,
        false,
        rules,
        Some(&runtime.resources.overlay_registry),
        crate::sim::world::FrameEffects::default(),
    );
    let plant_entity = runtime
        .simulation
        .entities_mut_for_test()
        .get_mut(plant)
        .unwrap();
    plant_entity
        .health
        .set_for_test(before["actual_hp"].as_i64().unwrap() as i32);
    plant_entity
        .estimated_health
        .reset(before["estimated_hp"].as_i64().unwrap() as i32);
    plant_entity.sample_building_health_for_power();
    runtime
        .simulation
        .refresh_building_damage_state(plant, rules);
    let damaged_slot = runtime
        .simulation
        .entities()
        .get(plant)
        .unwrap()
        .building_art
        .anim_slots()[3]
        .expect("stock GAPOWR active slot");
    assert_eq!(
        runtime
            .simulation
            .resolve(runtime.simulation.anim(damaged_slot).unwrap().type_id()),
        before["slots"][0]["type_name"].as_str().unwrap()
    );
    let engineer = runtime
        .simulation
        .spawn_object("ENGINEER", &owner_name, 30, 89, 0, rules)
        .unwrap();
    let mut envelopes = vec![CommandEnvelope::new(
        owner,
        runtime.simulation.session().tick(),
        Command::ToggleRepair { entity_id: plant },
    )];
    envelopes.extend(building_orders(
        &runtime.simulation,
        rules,
        engineer,
        plant,
        true,
    ));
    runtime.simulation.queue_commands(envelopes);
    let mut repaired_sound = 0;
    let mut healed_frame = None;
    let trace_path =
        std::env::var_os("VERA20K_ENGINEER_INPUT_TRACE_EXPORT").map(std::path::PathBuf::from);
    let mut trace = Vec::new();
    for _ in 0..80 {
        // The ordinary app tick drains the input queue before submitting the
        // frame transaction (match_runtime::sim_tick). Follow that same edge.
        let due = runtime.simulation.take_due_commands();
        let output = runtime
            .advance_frame(
                &due,
                crate::app::types::SIM_TICK_MS,
                TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        for event in output.sound_events {
            if matches!(event, SimSoundEvent::VocAt { ref sound_id, .. } if sound_id == "BuildingRepaired")
            {
                repaired_sound += 1;
            }
        }
        let plant_entity = runtime.simulation.entities().get(plant).unwrap();
        if trace_path.is_some() {
            trace.push(json!({
                "frame": runtime.simulation.session().binary_frame(),
                "plant": plant_entity,
                "engineer": runtime.simulation.entities().get(engineer),
                "power": runtime.simulation.power_states().get(&owner),
            }));
        }
        if plant_entity.health.current() == after["actual_hp"].as_i64().unwrap() as i32 {
            healed_frame.get_or_insert(runtime.simulation.session().binary_frame());
        }
        if healed_frame.is_some() && !runtime.simulation.entities().contains(engineer) {
            break;
        }
    }
    if let Some(path) = trace_path {
        assert!(!path.exists(), "preserve earlier movement evidence");
        std::fs::write(path, serde_json::to_vec_pretty(&trace).unwrap()).unwrap();
    }
    let healed_frame = healed_frame.expect("ordinary bound Walk/PerCell repair must arrive");
    assert!(
        !runtime.simulation.entities().contains(engineer),
        "deferred delete drained"
    );
    assert_eq!(repaired_sound, 1);
    runtime
        .advance_frame(
            &[],
            crate::app::types::SIM_TICK_MS,
            TickLane::Ordinary,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    let plant_entity = runtime.simulation.entities().get(plant).unwrap();
    assert_eq!(
        plant_entity.estimated_health.get(),
        after["estimated_hp"].as_i64().unwrap() as i32
    );
    assert!(!plant_entity.repairing.is_on());
    assert!(!plant_entity.building_art.damage_state_active());
    assert_eq!(plant_entity.building_power_health_sample(), Some(750));
    let healthy_slot = plant_entity.building_art.anim_slots()[3].unwrap();
    assert_ne!(healthy_slot, damaged_slot);
    assert!(runtime.simulation.anim(damaged_slot).is_none());
    assert_eq!(
        runtime
            .simulation
            .resolve(runtime.simulation.anim(healthy_slot).unwrap().type_id()),
        after["slots"][0]["type_name"].as_str().unwrap()
    );
    assert!(runtime.simulation.power_states()[&owner].total_output() >= 200);
    let second = runtime
        .simulation
        .spawn_object("ENGINEER", &owner_name, 30, 90, 0, &runtime.resources.rules)
        .unwrap();
    assert!(
        building_orders(
            &runtime.simulation,
            &runtime.resources.rules,
            second,
            plant,
            false,
        )
        .is_empty()
    );
    let receipt = json!({
        "schema": "vera20k.retail-engineer-building-input.v1",
        "map": "XMP03T4.MAP", "plant": plant, "engineer": engineer,
        "repair_frame": healed_frame, "sound_requests": repaired_sound,
        "damaged_slot": damaged_slot, "healthy_slot": healthy_slot,
        "actual_hp": 750, "estimated_hp": 750, "sampled_hp": 750,
        "state_hash": runtime.simulation.state_hash(),
        "limits": ["supplied admitted damaged Building prior", "target identity supplied",
            "production input decision, queue, Walk, repair and deferred cleanup",
            "audio device, native whole-world clock and pixel parity excluded"],
    });
    eprintln!("RETAIL_ENGINEER_BUILDING_INPUT {receipt}");
    if let Some(path) = std::env::var_os("VERA20K_ENGINEER_INPUT_EXPORT") {
        let path = std::path::PathBuf::from(path);
        assert!(!path.exists(), "preserve earlier production evidence");
        std::fs::write(path, serde_json::to_string_pretty(&receipt).unwrap() + "\n").unwrap();
    }
}
