//! Original Unit Attack7447A0 -> Foot4D4DC0 -> idle738970 -> Harvest73E5E0.
//! Replays the saved original executable packet, not values derived from Rust.
//! Shared map/ore/Drive setup and state comparisons remain with harvest_field.
//! Commands, expiry, idle, Stage and promotion call their production owners.
//!
//! Native packet: tools/spatial_oracle/harvest_attack_return.{json,md}; the
//! live-target Approach is an explicit external return seam. The fixture's
//! unarmed targets and stationary source reproduce its no-navigation answer;
//! actual shooting/death is connected separately by harvest_field_cycle_tests.
//! Compare every represented step's mission/cursor, dispatch and passive timer,
//! Target, NavCom, archive, latch, Stage, cargo/ore, spread and full Scenario RNG.
//! Raw pointers/event allocation bytes and unused timer padding are not state
//! identities in Rust. Facing/refinery fields are pinned by existing dock tests.

use super::harvest_field_oracle_tests::{compare_state, registry, row_scene_with};
use super::lifecycle::PointerExpiryControl;
use super::refinery_dock_oracle_tests::{Scene, cell};
use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionTimer};
use serde_json::{Value, json};

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/harvest_attack_return.json",
    ))
    .unwrap()
}

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

/// The native fixture supplies these two registered Unit objects. Their pointer
/// tokens are translated to existing Rust stable ids, never used numerically.
struct Replay {
    scene: Scene,
    first: u64,
    second: u64,
}

impl Replay {
    fn new(row: &Value) -> Self {
        let mut input = row["input"].clone();
        input["mission"] = "guard".into();
        input["miner_cell"] = json!([15, 15]);
        let mut scene = row_scene_with(&input, |text, _| {
            // Original reader receipt supplies .016 for these committed slots.
            // Existing field fixture already reads Harvest/Guard and the stock
            // Harvester, Storage and scan values through their native parsers.
            text.push_str("[Attack]\nRate=.016\n[Move]\nRate=.016\n");
            if let Some(control) = input.get("idle_control") {
                if control["armed"] == true {
                    *text = text.replacen("[HARV]\n", "[HARV]\nPrimary=IdleWeapon\n", 1);
                    text.push_str("[IdleWeapon]\nDamage=10\nROF=10\nRange=4\n");
                }
                if control["deploys_into"] == true {
                    *text = text.replacen("[HARV]\n", "[HARV]\nDeploysInto=GAREFN\n", 1);
                }
                text.push_str("[Sleep]\nZombie=yes\n");
            }
        });
        let owner = scene.sim.interner.intern("Russians");
        scene
            .sim
            .houses
            .insert(owner, HouseState::new(owner, 1, None, true, 0, 10));
        let target_owner = if input["same_owner"] == true {
            "Americans"
        } else {
            "Russians"
        };
        let first = scene
            .sim
            .spawn_object("MTNK", target_owner, 16, 15, 0, &scene.rules)
            .unwrap();
        let second = scene
            .sim
            .spawn_object("MTNK", "Russians", 15, 16, 0, &scene.rules)
            .unwrap();
        for id in [first, second] {
            scene
                .sim
                .substrate
                .entities
                .get_mut(id)
                .unwrap()
                .health
                .current = 1000;
        }
        let before = &row["before"];
        let entity = scene.sim.substrate.entities.get_mut(scene.miner).unwrap();
        entity.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(int(&before["miner_mission"])),
            queued: MissionId::from_raw(int(&before["miner_queued"])),
            suspended: MissionId::from_raw(int(&before["suspended"])),
            movement_bypass_latch: 0,
            handler_state: int(&before["miner_status"]) as u32,
            mission_start_frame: 200,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::from_raw(
                int(&before["dispatch_words"][0]),
                int(&before["dispatch_words"][2]),
            ),
        });
        entity.lifecycle.object_alive = before["alive"] == 1;
        entity.health.current = int(&before["health"]);
        entity.passive_scan_timer = MissionTimer {
            start_frame: int(&before["targeting_words"][0]) as u32,
            duration: int(&before["targeting_words"][2]) as u32,
        };
        if input["target"] == true {
            scene
                .sim
                .assign_target_represented(
                    scene.miner,
                    Some(TargetKind::Entity(first)),
                    Some(&scene.rules),
                )
                .unwrap();
        }
        if input["sensor"] == 1 {
            let owner = scene.sim.interner.get("Americans").unwrap();
            scene.sim.fog.width = 33;
            scene.sim.fog.height = 33;
            scene.sim.fog.sensors_add_at(owner, (16, 15), 1);
        }
        if let Some(control) = input.get("idle_control") {
            let entity = scene.sim.substrate.entities.get_mut(scene.miner).unwrap();
            entity.weapon_burst =
                serde_json::from_value(json!({"index": control["burst"]})).unwrap();
            let object = scene.rules.object("HARV").unwrap();
            assert_eq!(
                crate::sim::combat::combat_weapon::is_armed(entity, object),
                int(&row["idle_control_receipt"]["is_armed_eax"]) & 255 != 0,
                "{}: native IsArmed return byte",
                input["name"]
            );
            assert_eq!(
                object.deploys_into.is_some(),
                control["deploys_into"] == true
            );
            let owner = entity.owner();
            scene.sim.houses.get_mut(&owner).unwrap().current_iq = int(&control["house_iq"]);
        }
        // Unit construction consumes shared stream work in Rust. All fixture
        // allocations precede the declared original Scenario seed/prestate.
        scene.sim.scenario_rng = crate::sim::rng::SimRng::new(input["seed"].as_u64().unwrap_or(1));
        Self {
            scene,
            first,
            second,
        }
    }

    fn compare(&self, native: &Value, input: &Value, context: &str) {
        let s = &self.scene;
        let e = s.sim.substrate.entities.get(s.miner).unwrap();
        assert_eq!(
            s.sim.session.binary_frame as i32,
            int(&native["frame"]),
            "{context}: frame"
        );
        assert_eq!(
            e.mission.current().raw(),
            int(&native["miner_mission"]),
            "{context}: current"
        );
        assert_eq!(
            e.mission.queued().raw(),
            int(&native["miner_queued"]),
            "{context}: queued"
        );
        assert_eq!(
            e.mission.suspended().raw(),
            int(&native["suspended"]),
            "{context}: suspended"
        );
        assert_eq!(
            e.mission.handler_state(),
            int(&native["miner_status"]) as u32,
            "{context}: cursor"
        );
        assert_eq!(
            json!([
                e.mission.dispatch_timer().start_frame(),
                e.mission.dispatch_timer().delay()
            ]),
            native["dispatch_timer"],
            "{context}: dispatch timer"
        );
        assert_eq!(
            json!([
                e.passive_scan_timer.start_frame as i32,
                e.passive_scan_timer.duration as i32
            ]),
            json!([native["targeting_words"][0], native["targeting_words"][2]]),
            "{context}: passive timer"
        );
        let target = e
            .attack_target
            .as_ref()
            .map(|t| match t.target {
                TargetKind::Entity(id) if id == self.first => "0x200c2000",
                TargetKind::Entity(id) if id == self.second => "0x200c2800",
                other => panic!("{context}: unexpected Target {other:?}"),
            })
            .unwrap_or("0x0");
        assert_eq!(
            target,
            native["target"].as_str().unwrap(),
            "{context}: Target"
        );
        if let Some(index) = native.get("burst_index") {
            assert_eq!(e.weapon_burst.index(), int(index), "{context}: burst index");
        }
        assert_eq!(
            e.navigation.nav_queue.len(),
            int(&native["nav_queue_count"]) as usize,
            "{context}: waypoint queue"
        );
        assert_eq!(
            u8::from(e.is_object_alive()),
            int(&native["alive"]) as u8,
            "{context}: alive"
        );
        assert_eq!(
            e.health.current,
            int(&native["health"]),
            "{context}: health"
        );
        let first = s.sim.substrate.entities.get(self.first).unwrap();
        assert_eq!(
            u8::from(first.is_object_alive()),
            int(&native["target_alive"]) as u8,
            "{context}: target alive"
        );
        assert_eq!(
            first.health.current,
            int(&native["target_health"]),
            "{context}: target health"
        );
        let view = s.sim.scenario_rng.logical_view();
        assert_eq!(
            json!({"disabled": view.disabled, "index_a": view.index_a, "index_b": view.index_b, "state": view.words}),
            native["scenario_rng"],
            "{context}: full Scenario RNG"
        );
        let stage = serde_json::to_value(e.native_stage()).unwrap();
        assert_eq!(
            stage["changed"], native["stage_changed"],
            "{context}: Stage changed"
        );
        assert_eq!(
            stage["increment"], native["stage_step"],
            "{context}: Stage increment"
        );
        for (id, field) in [(s.miner, "miner_contact"), (s.refinery, "refinery_contact")] {
            let contact = s
                .sim
                .substrate
                .entities
                .get(id)
                .unwrap()
                .radio_contacts
                .slot(0)
                .map_or(Value::Null, |id| s.name(id));
            assert_eq!(contact, native[field], "{context}: {field}");
        }
        compare_state(s, &json!({"input": input, "state": native}), context);
        if e.miner.is_some() {
            assert_eq!(
                u8::from(e.miner.as_ref().unwrap().unload_active),
                int(&native["unloading"]) as u8,
                "{context}: unload latch"
            );
        } else {
            assert_eq!(
                native["storage"],
                json!([0.0, 0.0, 0.0, 0.0]),
                "{context}: no harvester storage"
            );
        }
    }

    fn step(&mut self, step: &Value, context: &str) {
        let input = &step["input"];
        let s = &mut self.scene;
        let now = s.sim.session.binary_frame;
        match input["op"].as_str().unwrap() {
            "command" | "stop" => {
                let command = match input["op"].as_str().unwrap() {
                    "stop" => Command::Stop { entity_id: s.miner },
                    _ => match input["mission"].as_str().unwrap() {
                        "attack" => Command::Attack {
                            attacker_id: s.miner,
                            target_id: if input["target"] == "second" {
                                self.second
                            } else {
                                self.first
                            },
                        },
                        "move" => {
                            let (target_rx, target_ry) = cell(&input["destination"]);
                            Command::Move {
                                entity_id: s.miner,
                                target_rx,
                                target_ry,
                                queue: false,
                            }
                        }
                        other => panic!("{context}: command {other}"),
                    },
                };
                assert!(
                    s.sim.apply_command_with_overlays(
                        "Americans",
                        &command,
                        Some(&s.rules),
                        Some(registry())
                    ),
                    "{context}: command admission"
                );
            }
            "promote" => s.sim.mission_host_promote(s.miner, now, &s.rules),
            "dispatch" => {
                let config = crate::sim::miner::MinerConfig::from_rules(&s.rules);
                super::dispatch_foot_mission(
                    &mut s.sim,
                    s.miner,
                    &s.rules,
                    super::ObjectAiCtx {
                        miner_config: Some(&config),
                        overlay_registry: Some(registry()),
                        ..Default::default()
                    },
                );
            }
            "post_foot_ai" => {
                crate::sim::miner::miner_system::unit_ai_clear_harvesting(&mut s.sim, s.miner)
            }
            "break_contact" => {
                let response = crate::sim::radio::transmit_to_contact(
                    &mut s.sim,
                    s.miner,
                    crate::sim::radio::RadioMessage::Break,
                    Some(&s.rules),
                );
                assert_eq!(
                    response.code() as i32,
                    int(&step["returned"]),
                    "{context}: BREAK"
                );
            }
            "techno_stage_ai" => {
                s.sim
                    .substrate
                    .entities
                    .get_mut(s.miner)
                    .unwrap()
                    .tick_native_stage(now as i32);
            }
            "idle" => {
                let skip_land = input["args"].as_array().is_some_and(|a| a[0] != 0);
                let result = s
                    .sim
                    .unit_enter_idle_mode(s.miner, Some(&s.rules), skip_land);
                assert_eq!(
                    u8::from(result),
                    (int(&step["returned"]) & 255) as u8,
                    "{context}: saved Foot idle result"
                );
            }
            "expiry" => {
                let victim = if input["pointer"] == "second" {
                    self.second
                } else {
                    self.first
                };
                if input["dead"] == true {
                    let e = s.sim.substrate.entities.get_mut(victim).unwrap();
                    e.health.current = 0;
                    e.lifecycle.object_alive = false;
                }
                let e = s.sim.substrate.entities.get(victim).unwrap();
                let (at, alive, health, owner) = (
                    (e.position.rx, e.position.ry),
                    e.is_object_alive(),
                    e.health.current,
                    e.owner(),
                );
                s.sim.notify_entity_pointer_expired(
                    s.miner,
                    victim,
                    Some(at),
                    alive,
                    health,
                    false,
                    Some(owner),
                    if input["control"] == 0 {
                        PointerExpiryControl::DetachAll
                    } else {
                        PointerExpiryControl::Uninit
                    },
                    Some(&s.rules),
                    None,
                );
            }
            "deadline" => {
                // Transport the ORIGINAL previous timer's deadline supplied
                // by this corpus step; never calculate a Rust golden frame.
                s.sim.session.binary_frame =
                    int(&step["returned"]["supplied_frame_from_previous_native_timer"]) as u32;
            }
            other => panic!("{context}: operation {other}"),
        }
    }
}

#[test]
fn attack_expiry_idle_and_harvest_match_original_executable_steps() {
    let corpus = corpus();
    assert_eq!(corpus["row_count"], 53);
    let mut compared = 0;
    for row in corpus["rows"].as_array().unwrap() {
        let name = row["input"]["name"].as_str().unwrap();
        // Retained AttackMove is a separate mechanism lacking the native +5C4
        // owner; ordinary player Attack clears it. Preserve its original row
        // as a residual instead of supplying a fake Rust equivalence.
        if name == "saved_attack_move" {
            continue;
        }
        let mut replay = Replay::new(row);
        replay.compare(&row["before"], &row["input"], &format!("{name}: initial"));
        for (index, step) in row["steps"].as_array().unwrap().iter().enumerate() {
            let context = format!("{name}: step {index} {}", step["input"]["op"]);
            replay.step(step, &context);
            replay.compare(&step["after"], &row["input"], &context);
        }
        compared += 1;
    }
    assert_eq!(compared, 52);
}

#[test]
fn shared_unit_idle_setter_admission_matches_original_controls() {
    let corpus = corpus();
    let mut compared = 0;
    for row in corpus["rows"].as_array().unwrap() {
        if row["input"].get("idle_control").is_none() {
            continue;
        }
        let name = row["input"]["name"].as_str().unwrap();
        let mut replay = Replay::new(row);
        replay.compare(&row["before"], &row["input"], &format!("{name}: initial"));
        for step in row["steps"].as_array().unwrap() {
            replay.step(step, name);
            replay.compare(&step["after"], &row["input"], name);
        }
        compared += 1;
    }
    assert_eq!(compared, 10);
}

#[test]
fn retail_inputs_match_original_reader_receipts() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let corpus = corpus();
    let receipt = &corpus["reader_receipts"]["layers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()["after"];
    let harv = retail.rules.object("HARV").unwrap();
    assert_eq!(u8::from(harv.harvester), int(&receipt["harvester"]) as u8);
    assert_eq!(u8::from(harv.weeder), int(&receipt["weeder"]) as u8);
    assert_eq!(harv.storage, int(&receipt["storage"]));
    assert_eq!(harv.movement_zone as i32, int(&receipt["movement_zone"]));
    assert_eq!(harv.dock, vec!["NAREFN", "GAREFN"]);
    assert_eq!(
        retail.rules.general.harvester_load_rate,
        int(&receipt["load_rate"])
    );
    assert_eq!(
        retail.rules.general.tiberium_short_scan,
        int(&receipt["short_scan"])
    );
    assert_eq!(
        retail.rules.general.tiberium_long_scan,
        int(&receipt["long_scan"])
    );
    for mission in [
        crate::sim::mission::MissionType::Attack,
        crate::sim::mission::MissionType::Harvest,
    ] {
        assert_eq!(retail.rules.mission_control.rate_frames(mission), 14);
    }
}
