//! Physical retail bridge input decisions composed with the existing complete
//! Engineer repair witness. Uses the production cursor/selection/action owners,
//! normal queue ingress/drain, bound runtime, Walk/PerCell entry and publication.
//! Target identity is supplied by the fixture: no window events, screen picking,
//! AppState finish_order/audio/target-line effects or native event-byte codec are
//! executed. Capture passes unchanged through the existing ordinary-Move codec
//! adapter, as it does in finish_order. The fixture retains its headless frame
//! duration; neither native whole-world timing nor native pixels are certified.

use super::*;
use crate::app::input::cursor::{
    ActionDistanceTarget, capability_cursor_for_hover, cursor_id_for_feedback,
    select_best_for_action,
};
use crate::app::types::{CursorFeedbackKind, CursorId};
use crate::headless_scenario::HeadlessScenario;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;
use crate::sim::world::bridge_test_evidence;
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};

fn packet() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/engineer_bridge_cursor_caller.json",
    ))
    .unwrap()
}

/// Supplied target identity, classified by the same loaded friendship owner
/// as hover_target_at_point. No separate bridge admission or geometric logic.
fn hut_hover(sim: &Simulation, engineer: u64, hut: u64) -> HoverTargetKindWithId {
    let actor = sim.entities().get(engineer).unwrap();
    let target = sim.entities().get(hut).unwrap();
    let friendly = crate::map::houses::are_houses_friendly(
        &sim.house_alliances,
        sim.resolve(actor.owner()),
        sim.resolve(target.owner()),
    );
    HoverTargetKindWithId {
        kind: if friendly {
            HoverTargetKind::FriendlyStructure
        } else {
            HoverTargetKind::EnemyStructure
        },
        stable_id: hut,
    }
}

fn resolve_hut_orders(
    sim: &Simulation,
    rules: &RuleSet,
    engineer: u64,
    hut: u64,
    repairable_span: bool,
    receipts: &RefCell<Vec<Value>>,
) -> Vec<CommandEnvelope> {
    let native = packet();
    let hover = hut_hover(sim, engineer, hut);
    let original_selection = [engineer];
    let before_hash = sim.state_hash();
    let before_actor = serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap();
    let before_rng = (
        sim.scenario_rng.logical_state(),
        sim.main_rng.logical_state(),
        sim.mapgen_rng.logical_state(),
    );
    let best = select_best_for_action(
        sim,
        &original_selection,
        ActionDistanceTarget::Object(hut),
        Some(rules),
    );
    assert_eq!(best, Some(engineer));
    assert_eq!(
        sim.engineer_building_action(engineer, hut, rules),
        Some(crate::sim::world::EngineerBuildingAction::Repair(
            repairable_span
        ))
    );
    let feedback =
        capability_cursor_for_hover(sim, &original_selection, best, &hover, Some(rules), None);
    // The executed caller proves identical action for self/allied/hostile.
    // Check every relation golden, avoiding an invented expected-action formula.
    let mut action = None;
    for relation in ["self", "allied", "hostile"] {
        let name = format!(
            "{relation}_{}",
            if repairable_span { "True" } else { "False" }
        );
        let row = native["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["route"] == "object" && row["input"]["name"] == name)
            .unwrap();
        let expected_action = row["output"]["action"].as_u64().unwrap();
        let expected_feedback = match expected_action {
            29 => CursorFeedbackKind::RepairAction(true),
            32 => CursorFeedbackKind::RepairAction(false),
            other => panic!("unrepresented native hut action {other}"),
        };
        assert_eq!(feedback, expected_feedback, "{name}");
        if let Some(action) = action {
            assert_eq!(expected_action, action);
        }
        action = Some(expected_action);
    }
    let action = action.unwrap();
    let display = native["display_cursors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["action"] == action && row["input"]["minimap"] == false)
        .unwrap();
    let native_cursor_row = display["output"]["cursor_row"].as_u64().unwrap();
    let expected_cursor = match native_cursor_row {
        33 => CursorId::Repair,
        35 => CursorId::NoRepair,
        other => panic!("unrepresented native cursor row {other}"),
    };
    assert_eq!(cursor_id_for_feedback(feedback), Some(expected_cursor));

    let mut remaining = original_selection.to_vec();
    let commands = engineer_capture_orders(sim, rules, &mut remaining, &hover)
        .expect("the real local Engineer/hut must reach the terminal hut action");
    assert!(remaining.is_empty());
    for relation in ["self", "allied", "hostile"] {
        let name = format!(
            "{relation}_{}",
            if repairable_span { "True" } else { "False" }
        );
        let row = native["object_clicks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == name && row["input"]["emp_blocked"] == false)
            .unwrap();
        assert_eq!(
            commands.len() as u64,
            row["output"]["queue_calls"].as_u64().unwrap(),
            "{name}"
        );
        for command in &commands {
            assert_eq!(
                command,
                &Command::CaptureBuilding {
                    engineer_id: engineer,
                    target_building_id: hut,
                }
            );
            let queued = row["output"]["trace"]
                .as_array()
                .unwrap()
                .iter()
                .find(|item| item["kind"] == "queue_boundary")
                .unwrap();
            assert_eq!(queued["mission"], 8);
            assert!(queued["target"].is_null());
            assert_eq!(queued["destination"], "hut");
        }
    }
    let owner = sim.entities().get(engineer).unwrap().owner();
    // This is the current-tick stamp chosen by the actual context input owner,
    // not the fixture's default supplied Capture(next tick) envelope.
    let mut envelopes: Vec<_> = commands
        .into_iter()
        .map(|payload| CommandEnvelope::new(owner, sim.session.tick, payload))
        .map(|envelope| {
            crate::app::input::commands::roundtrip_ordinary_local_megamission(sim, envelope)
                .unwrap()
        })
        .collect();
    restore_selection_dispatch_order(&mut envelopes, &original_selection);
    assert_eq!(sim.state_hash(), before_hash);
    assert_eq!(
        serde_json::to_value(sim.entities().get(engineer).unwrap()).unwrap(),
        before_actor
    );
    assert_eq!(
        (
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        ),
        before_rng
    );
    receipts.borrow_mut().push(json!({
        "tick": sim.session.tick, "binary_frame": sim.session.binary_frame,
        "engineer": engineer, "hut": hut,
        "hut_cell": [sim.entities().get(hut).unwrap().position.rx, sim.entities().get(hut).unwrap().position.ry],
        "native_action": action, "native_cursor_row": native_cursor_row,
        "envelopes": envelopes, "input_state_hash": format!("{before_hash:016X}"),
        "input_rng": format!("{before_rng:?}"),
        "input_preserved_actor_hash_and_rng": true,
    }));
    envelopes
}

fn probe_healthy_hut(
    scene: &mut HeadlessScenario,
    hut_coord: (u16, u16),
    start: (u16, u16),
    receipts: &RefCell<Vec<Value>>,
) {
    let runtime = &mut scene.runtime;
    let owner = runtime.simulation.session.current_house.unwrap();
    let owner_name = runtime.simulation.resolve(owner).to_owned();
    let hut = runtime
        .simulation
        .entities()
        .values()
        .find_map(|entity| {
            (runtime.simulation.resolve(entity.type_ref()) == "CABHUT"
                && (entity.position.rx, entity.position.ry) == hut_coord)
                .then_some(entity.stable_id())
        })
        .unwrap();
    // The positive repair consumed its actor. Use the normal constructor for
    // another real Engineer, rather than resurrecting or inventing actor state.
    let engineer = runtime
        .simulation
        .spawn_object(
            "ENGINEER",
            &owner_name,
            start.0,
            start.1,
            0,
            &runtime.resources.rules,
        )
        .unwrap();
    assert!(
        !crate::sim::world::bridge_orchestrator::bridge_hut_can_repair(
            &runtime.simulation,
            hut_coord
        )
    );
    let before_pending = runtime.simulation.pending_command_snapshot();
    let before_hash = runtime.simulation.state_hash();
    let envelopes = resolve_hut_orders(
        &runtime.simulation,
        &runtime.resources.rules,
        engineer,
        hut,
        false,
        receipts,
    );
    assert!(envelopes.is_empty());
    runtime.simulation.queue_commands(envelopes);
    assert_eq!(
        runtime.simulation.pending_command_snapshot(),
        before_pending
    );
    assert_eq!(runtime.simulation.state_hash(), before_hash);
    assert!(
        runtime
            .simulation
            .entities()
            .get(engineer)
            .unwrap()
            .navigation
            .nav_com
            .is_none()
    );
}

fn record_receipt(map: &str, stages: Vec<Value>, orders: Vec<Value>) {
    let receipt = json!({
        "schema": "vera20k.retail-hut-input-composition.v1", "map": map,
        "native_packet_sha256": crate::util::sha256::sha256_hex(crate::test_fixture::bytes("tools/spatial_oracle/engineer_bridge_cursor_caller.json")),
        "fixture_tick_ms": crate::headless_scenario::SIM_TICK_MS,
        "stages": stages, "orders": orders,
        "limits": [
            "Physical retail asset/rules load and production input decision owners composed with ordinary queue/drain and full bound-runtime Engineer repair.",
            "Fixture supplies target identity; window delivery, screen picking and AppState finish_order side effects are excluded.",
            "Capture passes through the existing Move-only codec adapter; native Capture event-byte codec is not established.",
            "Fixture retains existing headless frame duration. No native whole-world timing or pixel parity is claimed.",
        ],
    });
    eprintln!(
        "RETAIL_HUT_INPUT {}",
        serde_json::to_string(&receipt).unwrap()
    );
    if let Some(directory) = std::env::var_os("VERA20K_BRIDGE_INPUT_EXPORT") {
        let directory = std::path::PathBuf::from(directory);
        assert!(
            directory.is_dir(),
            "input export directory must already exist"
        );
        let path = directory.join(format!("{map}-hut-input.json"));
        let text = serde_json::to_string_pretty(&receipt).unwrap() + "\n";
        if path.exists() {
            assert_eq!(
                std::fs::read_to_string(path).unwrap(),
                text,
                "preserve prior evidence"
            );
        } else {
            std::fs::write(path, text).unwrap();
        }
    }
}

#[test]
#[ignore = "requires physical Anytown and retail TEMPERATE assets"]
fn retail_concrete_hut_input_reaches_engineer_entry_and_publication() {
    let orders = RefCell::new(Vec::new());
    let stages = RefCell::new(Vec::new());
    let repairs = Cell::new(0);
    bridge_test_evidence::visit_anytown_concrete_stages_with_repair_orders(
        |phase, scene| {
            stages.borrow_mut().push(json!({
                "phase": phase, "map_hash": scene.map.ini.content_hash(),
                "tick": scene.sim().session.tick, "frame": scene.sim().session.binary_frame,
                "state_hash": format!("{:016X}", scene.sim().state_hash()),
            }));
            if phase == "repaired" {
                probe_healthy_hut(scene, (89, 51), (89, 50), &orders);
            }
        },
        |sim, rules, engineer, hut| {
            repairs.set(repairs.get() + 1);
            resolve_hut_orders(sim, rules, engineer, hut, true, &orders)
        },
    );
    assert_eq!(repairs.get(), 1);
    record_receipt("anytown-concrete", stages.into_inner(), orders.into_inner());
}

#[test]
#[ignore = "requires physical Shrapnel and retail SNOW assets"]
fn retail_wood_hut_input_reaches_engineer_entry_and_publication() {
    let orders = RefCell::new(Vec::new());
    let stages = RefCell::new(Vec::new());
    let repairs = Cell::new(0);
    bridge_test_evidence::visit_shrapnel_wood_stages_with_repair_orders(
        |phase, scene| {
            stages.borrow_mut().push(json!({
                "phase": phase, "map_hash": scene.map.ini.content_hash(),
                "tick": scene.sim().session.tick, "frame": scene.sim().session.binary_frame,
                "state_hash": format!("{:016X}", scene.sim().state_hash()),
            }));
            if phase == "repaired" {
                probe_healthy_hut(scene, (117, 56), (117, 55), &orders);
            }
        },
        |sim, rules, engineer, hut| {
            repairs.set(repairs.get() + 1);
            resolve_hut_orders(sim, rules, engineer, hut, true, &orders)
        },
    );
    assert_eq!(repairs.get(), 2);
    record_receipt("shrapnel-wood", stages.into_inner(), orders.into_inner());
}
