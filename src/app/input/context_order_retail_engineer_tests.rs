//! Physical retail rules/ART/map load and the ordinary Engineer input route.
//!
//! The already admitted Building's damaged prior comes from the native joined
//! witness. Its constructor/placement, full native world clock, mouse picking,
//! native event-byte codec and audio device output are outside this test.
//! Decisions, queue/drain, Walk arrival, repair/art/sound request, House update
//! and deferred Engineer retirement run through the bound production runtime.

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
    let hover = HoverTargetKindWithId {
        kind: HoverTargetKind::FriendlyStructure,
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
    let feedback =
        capability_cursor_for_hover(sim, &original_selection, best, &hover, Some(rules), None);
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
        .map(|payload| CommandEnvelope::new(owner, sim.session.tick, payload))
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
    let owner = runtime.simulation.session.current_house.unwrap();
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
        .substrate
        .entities
        .get_mut(plant)
        .unwrap()
        .finish_building_construction_for_test();
    runtime.simulation.grand_opening(
        plant,
        false,
        false,
        rules,
        Some(&runtime.resources.overlay_registry),
    );
    let plant_entity = runtime
        .simulation
        .substrate
        .entities
        .get_mut(plant)
        .unwrap();
    plant_entity.health.current = before["actual_hp"].as_i64().unwrap() as i32;
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
        .building_anim_slots[3]
        .expect("stock GAPOWR active slot");
    assert_eq!(
        runtime
            .simulation
            .resolve(runtime.simulation.anim(damaged_slot).unwrap().type_id),
        before["slots"][0]["type_name"].as_str().unwrap()
    );
    let engineer = runtime
        .simulation
        .spawn_object("ENGINEER", &owner_name, 30, 89, 0, rules)
        .unwrap();
    let mut envelopes = vec![CommandEnvelope::new(
        owner,
        runtime.simulation.session.tick,
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
            .advance_frame(&due, crate::app::types::SIM_TICK_MS, TickLane::Ordinary)
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
                "frame": runtime.simulation.session.binary_frame,
                "plant": plant_entity,
                "engineer": runtime.simulation.entities().get(engineer),
                "power": runtime.simulation.power_states.get(&owner),
            }));
        }
        if plant_entity.health.current == after["actual_hp"].as_i64().unwrap() as i32 {
            healed_frame.get_or_insert(runtime.simulation.session.binary_frame);
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
        .advance_frame(&[], crate::app::types::SIM_TICK_MS, TickLane::Ordinary)
        .unwrap();
    let plant_entity = runtime.simulation.entities().get(plant).unwrap();
    assert_eq!(
        plant_entity.estimated_health.get(),
        after["estimated_hp"].as_i64().unwrap() as i32
    );
    assert!(!plant_entity.repairing);
    assert!(!plant_entity.building_damage_state_active);
    assert_eq!(plant_entity.building_power_health_sample(), Some(750));
    let healthy_slot = plant_entity.building_anim_slots[3].unwrap();
    assert_ne!(healthy_slot, damaged_slot);
    assert!(runtime.simulation.anim(damaged_slot).is_none());
    assert_eq!(
        runtime
            .simulation
            .resolve(runtime.simulation.anim(healthy_slot).unwrap().type_id),
        after["slots"][0]["type_name"].as_str().unwrap()
    );
    assert!(runtime.simulation.power_states[&owner].total_output >= 200);
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
