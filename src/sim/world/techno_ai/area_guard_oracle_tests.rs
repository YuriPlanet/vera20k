//! Original G-issued Event -> Commence -> in-range AreaGuard histories.
//!
//! The existing Foot fixture owns supplied constructor ordinals, Cell crop,
//! Logic/Display/occupancy order and retail readers. This adapter supplies the
//! observed event-boundary state and transports pointers/Abstract IDs only.
//! It neither constructs expected gameplay nor runs missing intervening ticks.
//! Native network scheduling, locomotor Process and pursuit are excluded.

use super::*;
use crate::sim::command::{
    Command, CommandRecord, MegaMissionOrder, MegaMissionRecord, MegaMissionTarget,
};
use crate::sim::snapshot::GameSnapshot;

fn corpus() -> &'static Value {
    static CORPUS: OnceLock<Value> = OnceLock::new();
    CORPUS.get_or_init(|| {
        let value: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/area_guard.json",
        ))
        .unwrap();
        assert_eq!(value["native_sha256"], oracle()["native_sha256"]);
        value
    })
}

fn streams(row: &Value, state: &Value) -> Value {
    let mut value = json!({});
    for name in ["main", "scenario", "mapgen"] {
        let reference = state["rng"][name].as_str().unwrap();
        let bytes = row["complete_rng_states"][reference]["bytes"]
            .as_str()
            .unwrap();
        value[name] = serde_json::to_value(SimRng::from_native_state_hex_for_test(bytes)).unwrap();
    }
    value
}

fn pointer(value: &Value) -> Value {
    Value::String(format!("0x{:x}", value.as_u64().unwrap()))
}

fn fixture(row: &Value, state: &Value, rules: &RuleSet) -> SuppliedFootFixture {
    let native = oracle();
    let mut supplied = native["rows"][0].clone();
    supplied["input"]["name"] = row["name"].clone();
    supplied["input"]["candidate_live"] = json!(true);
    supplied["rng_before"] = streams(row, state);
    let mut f = SuppliedFootFixture::new(&supplied, rules);
    let roles = [
        ("MTNK", "source"),
        ("E1", "e1"),
        ("allied_MTNK", "victim"),
        ("enemy_MTNK", "candidate"),
    ];
    let bindings: Vec<_> = roles
        .iter()
        .map(|(name, prior)| {
            (
                &corpus()["histories"]["actor_pointers"][name],
                &native["setup"][prior],
            )
        })
        .collect();
    f.remap_native_objects(&bindings);
    install_recorded_cell_coordinates(&mut f);
    // Extend the shared fixture only with independent, pre-case Cell input
    // observations. A destination result never supplies its own height input.
    let mut terrain = f.sim.resolved_terrain.as_ref().unwrap().clone();
    for receipt in corpus()["histories"]["post_cells"].as_array().unwrap() {
        let before = &receipt["before"];
        let (x, y) = f.cells[before["pointer"].as_str().unwrap()];
        assert_eq!(json!([x, y]), before["cell"]);
        assert_eq!(receipt["getter"], "0x486840");
        assert_eq!(before, &receipt["after"]);
        assert_eq!(receipt["rng_before"], receipt["rng_after"]);
        assert_eq!(before["slope"], 0, "represented flat post cells");
        *terrain.cell_mut(x, y).unwrap() =
            crate::sim::world::lifecycle_tests::common_raw_terrain_cell(
                x,
                y,
                u8::try_from(signed(&before["level"])).unwrap(),
                false,
            );
        terrain.cell_mut(x, y).unwrap().yr_cell_land_type =
            u8::try_from(signed(&before["land"])).unwrap();
        let identity = terrain.native_cell_identity((x as i16, y as i16));
        terrain.write_native_cell_flags(identity, before["flags"].as_u64().unwrap() as u32);
        let coordinate = crate::sim::movement::nav_target_coordinate(
            NavTargetRef::cell(x, y),
            None,
            &f.sim.substrate.entities,
            Some(&terrain),
            Some((rules, &f.sim.interner)),
        )
        .unwrap();
        assert_eq!(
            [coordinate.x, coordinate.y, coordinate.z],
            xyz(&receipt["returned_xyz"])
        );
    }
    f.sim.install_resolved_terrain_for_new_map(terrain);
    f.sim.session.binary_frame = state["frame"].as_u64().unwrap() as u32;
    f.sim.session.tick = state["frame"].as_u64().unwrap();
    f.sim.session.game_mode_nonzero = state["game_mode"] != 0;
    let friendly = f.sim.substrate.entities.get(f.actor).unwrap().owner;
    let enemy = f
        .sim
        .substrate
        .entities
        .get(
            f.id(&corpus()["histories"]["actor_pointers"]["enemy_MTNK"])
                .unwrap(),
        )
        .unwrap()
        .owner;
    f.sim.session.current_house = Some(friendly);
    f.sim.session.house_order = vec![friendly, enemy];
    for (name, _) in roles {
        let before = &state["actors"][name];
        let id = f
            .id(&corpus()["histories"]["actor_pointers"][name])
            .unwrap();
        let archive = f.target(&before["archive"]);
        let target = f.target(&before["target"]);
        let nav = f.nav(&before["nav"]);
        let suspended_target = f.target(&pointer(&before["suspended_target"]));
        let suspended_nav = f.nav(&pointer(&before["suspended_nav"]));
        let aux = f.nav(&pointer(&before["navigation"]["aux"]));
        let actor = f.sim.substrate.entities.get_mut(id).unwrap();
        // Pose and active-list/cell registration come from the existing
        // original Unlimbo observations; these histories do not relocate them.
        let coord = crate::sim::movement::ground_pose::position_world_coord(&actor.position);
        assert_eq!([coord.x, coord.y, coord.z], xyz(&before["position"]));
        actor.lifecycle.in_limbo = before["limbo"] != 0;
        actor.lifecycle.object_alive = before["alive"] != 0;
        actor.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(signed(&before["mission"])),
            suspended: MissionId::from_raw(signed(&before["suspended_mission"])),
            queued: MissionId::from_raw(signed(&before["queued"])),
            movement_bypass_latch: signed(&before["mission_flag_b8"]) as u8,
            handler_state: signed(&before["status"]) as u32,
            mission_start_frame: 0,
            ai_counter: signed(&before["visit"]) as u32,
            dispatch_timer: MissionDispatchTimer::from_raw(
                signed(&before["dispatch"][0]),
                signed(&before["dispatch"][1]),
            ),
        });
        actor.set_archive_target(archive);
        actor.attack_target = target.map(|target| AttackTarget { target });
        actor.suspended_attack_target = suspended_target;
        actor.navigation.suspended_nav_com = suspended_nav;
        actor.navigation.nav_com = nav;
        actor.navigation.nav_com_aux = aux;
        assert_eq!(before["navigation"]["queue_count"], 0);
        actor.navigation.nav_queue.clear();
        actor.navigation.path_replay.directions = before["navigation"]["path"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| signed(value) as u8)
            .collect();
        actor.navigation.path_replay.reference_cell = Some((
            signed(&before["navigation"]["reference_cell"][0]) as i16,
            signed(&before["navigation"]["reference_cell"][1]) as i16,
        ));
        let path = &mut actor.navigation.path_runtime;
        path.movement_timer = crate::sim::timer::CdTimer::from_raw(
            signed(&before["navigation"]["movement_timer"][0]),
            signed(&before["navigation"]["movement_timer"][2]),
        );
        path.blocked_timer = crate::sim::timer::CdTimer::from_raw(
            signed(&before["navigation"]["blocked_timer"][0]),
            signed(&before["navigation"]["blocked_timer"][2]),
        );
        path.path_blocked = before["navigation"]["blocked"] != 0;
        path.retries_left = signed(&before["navigation"]["retry"]) as u32;
        assert_eq!(before["loco_head"], json!([0, 0, 0]));
        assert_eq!(before["navigation"]["destination"], json!([0, 0, 0]));
        actor.passive_scan_timer = MissionTimer::armed(
            signed(&before["targeting_timer"][0]) as u32,
            signed(&before["targeting_timer"][2]) as u32,
        );
        assert_eq!(before["scan"], 0);
        actor.on_bridge = before["on_bridge"] != 0;
        actor
            .mission_leaf
            .set_foot_firing_sequence(signed(&before["firing"]) as u8);
        if let Some(infantry) = actor.infantry.as_mut() {
            infantry.idle_action_timer = MissionTimer::armed(
                signed(&before["idle_timer"][0]) as u32,
                signed(&before["idle_timer"][2]) as u32,
            );
            actor
                .mission_leaf
                .set_infantry_doing_verified(signed(&before["doing"]))
                .unwrap();
        }
    }
    f
}

fn assert_state(f: &mut SuppliedFootFixture, row: &Value, state: &Value) {
    for (name, expected) in state["actors"].as_object().unwrap() {
        f.actor = f
            .id(&corpus()["histories"]["actor_pointers"][name])
            .unwrap();
        assert_foot_projection(
            f,
            &json!({"input": {"name": format!("{}:{name}", row["name"])}, "after": expected, "rng_after": streams(row, state)}),
        );
        let actor = f.sim.substrate.entities.get(f.actor).unwrap();
        assert_eq!(
            actor.mission.suspended().raw(),
            signed(&expected["suspended_mission"]),
            "{name}: retained suspended selector"
        );
        assert_eq!(
            actor.suspended_attack_target,
            f.target(&pointer(&expected["suspended_target"])),
            "{name}: suspended Target"
        );
        assert_eq!(
            actor.navigation.suspended_nav_com,
            f.nav(&pointer(&expected["suspended_nav"])),
            "{name}: suspended NavCom"
        );
        assert_eq!(
            actor.navigation.nav_com_aux,
            f.nav(&pointer(&expected["navigation"]["aux"])),
            "{name}: NavComAux"
        );
        assert_eq!(
            actor.navigation.nav_queue.len(),
            expected["navigation"]["queue_count"].as_u64().unwrap() as usize,
            "{name}: navigation vector"
        );
        assert_eq!(
            actor.navigation.path_replay.directions,
            expected["navigation"]["path"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| signed(value) as u8)
                .collect::<Vec<_>>(),
            "{name}: retained path suffix"
        );
        assert_eq!(
            actor.navigation.path_replay.reference_cell,
            Some((
                signed(&expected["navigation"]["reference_cell"][0]) as i16,
                signed(&expected["navigation"]["reference_cell"][1]) as i16
            )),
            "{name}: reference Cell"
        );
        let path = &actor.navigation.path_runtime;
        for (timer, field) in [
            (path.movement_timer, "movement_timer"),
            (path.blocked_timer, "blocked_timer"),
        ] {
            assert_eq!(
                [timer.start_frame(), timer.duration()],
                [
                    signed(&expected["navigation"][field][0]),
                    signed(&expected["navigation"][field][2])
                ],
                "{name}: {field}"
            );
        }
        assert_eq!(
            path.retries_left,
            signed(&expected["navigation"]["retry"]) as u32,
            "{name}: retry"
        );
        assert_eq!(
            path.path_blocked,
            expected["navigation"]["blocked"] != 0,
            "{name}: blocked"
        );
        let loco = actor.locomotor.as_ref().unwrap();
        let (destination, head) = if let Some(drive) = loco
            .selected_drive_runtime()
            .and_then(|drive| drive.retained())
        {
            (drive.destination(), drive.head_to())
        } else {
            (loco.walk_destination(), loco.step_head())
        };
        let coord = |coord: Option<DriveCoord>| coord.map_or([0, 0, 0], |c| [c.x, c.y, c.z]);
        assert_eq!(
            coord(destination),
            xyz(&expected["navigation"]["destination"]),
            "{name}: destination"
        );
        assert_eq!(coord(head), xyz(&expected["loco_head"]), "{name}: Head_To");
    }
}

fn delivered_command(f: &SuppliedFootFixture, row: &Value, boundary: &Value) -> Command {
    let record_pointer = boundary["this"].as_u64().unwrap();
    let slot = (record_pointer - 0xA802D4) / 0x6F;
    let event = boundary["before"]["rings"]["outlist"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["slot"] == slot)
        .unwrap();
    let bytes: Vec<_> = event["bytes"]
        .as_str()
        .unwrap()
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    let record = MegaMissionRecord::decode(&CommandRecord::decode_exact(&bytes).unwrap()).unwrap();
    let id = |native_id: i32| {
        let (name, _) = boundary["before"]["actors"]
            .as_object()
            .unwrap()
            .iter()
            .find(|(_, actor)| actor["id"] == native_id)
            .unwrap_or_else(|| panic!("{}: missing native Abstract ID{native_id}", row["name"]));
        f.id(&corpus()["histories"]["actor_pointers"][name])
            .unwrap()
    };
    match record.order {
        MegaMissionOrder::AreaGuard { post } => Command::Guard {
            entity_id: id(record.source_id),
            target: match post {
                MegaMissionTarget::Null => None,
                MegaMissionTarget::Cell { x, y } => Some(TargetKind::Cell(x as u16, y as u16)),
                MegaMissionTarget::Object { id: native_id } => {
                    Some(TargetKind::Entity(id(native_id)))
                }
            },
        },
        MegaMissionOrder::Move { target_x, target_y } => Command::Move {
            entity_id: id(record.source_id),
            target_rx: target_x as u16,
            target_ry: target_y as u16,
            queue: false,
        },
    }
}

#[test]
fn area_guard_events_commence_and_in_range_dispatch_match_original_histories() {
    let Some(rules) = retail_rules() else {
        return;
    };
    assert_eq!(
        rules_receipt(&rules),
        corpus()["histories"]["handler_rules"]
    );
    for family in ["MTNK", "E1"] {
        assert_weapon_reader_projection(
            &rules,
            &corpus()["histories"]["weapon_reader_projection"][family],
            family,
        );
    }
    let mut event_count = 0;
    let mut dispatch_count = 0;
    for row in corpus()["histories"]["rows"].as_array().unwrap() {
        if row["input"]["dispatch"] != true {
            continue;
        }
        let boundaries = row["boundaries"].as_array().unwrap();
        let first = boundaries
            .iter()
            .position(|b| {
                b["label"]
                    .as_str()
                    .unwrap()
                    .starts_with("delivered_original_Event_")
            })
            .unwrap();
        let mut f = fixture(row, &boundaries[first]["before"], &rules);
        for boundary in &boundaries[first..] {
            let label = boundary["label"].as_str().unwrap();
            // A subsequent Player_Send_Command only queues another raw record.
            // Its own ingress/voice/RNG effects have separate app comparisons.
            if label == "original_Move_retask" {
                continue;
            }
            f.sim.session.binary_frame = boundary["before"]["frame"].as_u64().unwrap() as u32;
            f.sim.session.tick = boundary["before"]["frame"].as_u64().unwrap();
            assert_state(&mut f, row, &boundary["before"]);
            if label.starts_with("delivered_original_") {
                let command = delivered_command(&f, row, boundary);
                let owner = f.sim.substrate.entities.get(1).unwrap().owner;
                let owner = f.sim.interner.resolve(owner).to_owned();
                assert!(
                    f.sim.apply_command(&owner, &command, Some(&rules)),
                    "{}: {label}",
                    row["name"]
                );
                event_count += 1;
            } else if label == "original_Commence" {
                let actor = f
                    .id(&Value::String(format!(
                        "0x{:x}",
                        boundary["this"].as_u64().unwrap()
                    )))
                    .unwrap();
                assert_eq!(
                    f.sim
                        .mission_commence_exact(actor, f.sim.session.binary_frame)
                        .unwrap(),
                    boundary["returned_eax"] != 0
                );
            } else {
                assert!(
                    label.contains("Mission_dispatch"),
                    "unexpected boundary{label}"
                );
                let actor = f
                    .id(&Value::String(format!(
                        "0x{:x}",
                        boundary["this"].as_u64().unwrap()
                    )))
                    .unwrap();
                super::super::dispatch_foot_mission(
                    &mut f.sim,
                    actor,
                    &rules,
                    crate::sim::world::ObjectAiCtx::default(),
                );
                dispatch_count += 1;
            }
            assert_state(&mut f, row, &boundary["after"]);
        }
    }
    assert_eq!(event_count, 13);
    assert_eq!(dispatch_count, 6);
}

#[test]
fn area_guard_snapshot_retains_pending_order_post_and_retask_references() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let row = corpus()["histories"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "guard_suspended_cleanup")
        .unwrap();
    let boundary = row["boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|boundary| boundary["label"] == "delivered_original_Event_0")
        .unwrap();
    let mut f = fixture(row, &boundary["before"], &rules);
    let command = delivered_command(&f, row, boundary);
    let owner = f.sim.substrate.entities.get(1).unwrap().owner;
    f.sim
        .queue_command(crate::sim::command::CommandEnvelope::new(
            owner,
            f.sim.session.tick + 1,
            command,
        ));
    let saved = GameSnapshot::save(&f.sim, 0, 0, "Area Guard pending retask", 0);
    let mut restored = GameSnapshot::load(&saved).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        restored.pending_command_snapshot(),
        f.sim.pending_command_snapshot()
    );
    // The Scenario reset and process RNG policy belong to the load owner.
    // Compare persisted references here; neither a no-load RNG continuation
    // nor native raw-save wire equivalence is inferred from this roundtrip.
    let actor = restored.substrate.entities.get(1).unwrap();
    let original = f.sim.substrate.entities.get(1).unwrap();
    assert_eq!(actor.mission, original.mission);
    assert_eq!(
        actor.suspended_attack_target,
        original.suspended_attack_target
    );
    assert_eq!(
        actor.navigation.suspended_nav_com,
        original.navigation.suspended_nav_com
    );
    assert_eq!(actor.archive_target(), original.archive_target());

    let owner = f.sim.interner.resolve(owner).to_owned();
    let pending = f.sim.take_due_commands();
    assert_eq!(pending.len(), 1);
    assert!(
        f.sim
            .apply_command(&owner, &pending[0].payload, Some(&rules))
    );
    let saved = GameSnapshot::save(&f.sim, 0, 0, "Area Guard committed post", 0);
    let mut restored = GameSnapshot::load(&saved).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    let actor = restored.substrate.entities.get(1).unwrap();
    let original = f.sim.substrate.entities.get(1).unwrap();
    assert_eq!(actor.mission, original.mission);
    assert_eq!(actor.archive_target(), original.archive_target());
    assert_eq!(actor.navigation.nav_com, original.navigation.nav_com);
    assert!(actor.archive_target().is_some());
    assert!(actor.suspended_attack_target.is_none());
    assert!(actor.navigation.suspended_nav_com.is_none());
    assert!(restored.pending_command_snapshot().is_empty());
}
