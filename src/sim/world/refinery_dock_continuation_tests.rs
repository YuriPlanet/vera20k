//! Retained original 4D9290/73E5E0/73D630/radio/per-cell histories.
//! Native outputs: tools/spatial_oracle/refinery_dock.{json,md}. Compare
//! every represented boundary, including full Scenario RNG, cadence, contacts,
//! cargo and payment. Arrivals supply coordinates/terminated Drive vectors;
//! path traversal, whole-world scheduling/destruction and repair math are
//! explicitly outside the executable packet. No Rust-generated goldens.

use super::harvest_field_oracle_tests::{registry, row_scene_with};
use super::lifecycle::PointerExpiryControl;
use super::refinery_dock_oracle_tests::{Scene, cell, sends};
use crate::sim::combat::TargetKind;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::docking::building_dock;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::sim::radio::{self, RadioMessage, RadioPayload};
use serde_json::{Value, json};

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/refinery_dock.json",
    ))
    .unwrap()
}

fn int(v: &Value) -> i32 {
    v.as_i64().unwrap() as i32
}

fn nav(s: &Scene, v: &Value) -> Option<NavTargetRef> {
    if v.is_null() {
        None
    } else if let Some(name) = v.as_str() {
        Some(NavTargetRef::Building { id: s.id(name) })
    } else {
        let (rx, ry) = cell(v);
        Some(NavTargetRef::cell(rx, ry))
    }
}

fn nav_name(s: &Scene, v: Option<NavTargetRef>) -> Value {
    match v {
        None => Value::Null,
        Some(NavTargetRef::Cell { rx, ry }) => json!([rx, ry]),
        Some(
            NavTargetRef::Building { id }
            | NavTargetRef::Entity { id }
            | NavTargetRef::Object { id },
        ) => s.name(id),
    }
}

fn coord(v: &Value) -> Option<DriveCoord> {
    let values = v.as_array()?;
    let c = DriveCoord {
        x: int(&values[0]),
        y: int(&values[1]),
        z: int(&values[2]),
    };
    (c != DriveCoord { x: 0, y: 0, z: 0 }).then_some(c)
}

fn coord_value(v: Option<DriveCoord>) -> Value {
    v.map_or_else(|| json!([0, 0, 0]), |c| json!([c.x, c.y, c.z]))
}

fn fixture(row: &Value) -> Scene {
    let case = &row["input"];
    let mut input = case.clone();
    // Cell-only legacy dressing is replaced by the declared native prestate.
    input.as_object_mut().unwrap().remove("nav");
    input.as_object_mut().unwrap().remove("archive");
    if input.get("ore").is_none() {
        input["ore"] = json!([]);
    }
    input["unlimbo_at_cell"] = false.into();
    let mut s = row_scene_with(&input, |text, art| {
        // This packet transfers the original retail-reader final bytes into
        // its sparse types. The separate retail test checks these inputs.
        *text = text.replacen(
            "[HARV]\nStrength=1000\n",
            &format!(
                "[HARV]\nStrength={}\nSpeedType=Wheel\n",
                case["strength"].as_i64().unwrap_or(1000)
            ),
            1,
        );
        text.push_str("[Move]\nRate=.016\n");
        if let Some(count) = case["type_number_of_docks"].as_i64() {
            *text = text.replacen("NumberOfDocks=1", &format!("NumberOfDocks={count}"), 1);
        }
        *text = text.replacen(
            "[GAREFN]\nStrength=900\nRefinery=yes\nDockUnload=yes\n",
            if case["unit_repair"] == true {
                "[GAREFN]\nStrength=1200\nUnitRepair=yes\nRefinery=no\nDockUnload=no\n"
            } else {
                "[GAREFN]\nStrength=1000\nRefinery=yes\nDockUnload=yes\n"
            },
            1,
        );
        if case["unit_repair"] == true {
            // The sparse native controls leave the offset array pointer NULL
            // and map [-1,0,0] at page0. This is a supplied fixture input,
            // not the stock NADEPT ART offset (refinery_dock.md).
            let offset = &row["supplied_dock_inputs"]["offsets"][0];
            assert!(offset.is_array(), "native dock input receipt required");
            art.merge(&crate::rules::ini_parser::IniFile::from_str(&format!(
                "[GAREFN]\nQueueingCell=0,0\nDockingOffset0={},{},{}\n",
                int(&offset[0]),
                int(&offset[1]),
                int(&offset[2]),
            )));
        }
        if case["hover"] == true {
            *text = text.replacen(
                "Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}",
                "Locomotor={4A582742-9839-11D1-B709-00A024DDAFD1}",
                1,
            );
        }
    });
    if let Some(rates) = row["supplied_rate_frames"].as_object() {
        for (name, frames) in rates {
            let mission = match name.as_str() {
                "enter" => crate::sim::mission::MissionType::Enter,
                "guard" => crate::sim::mission::MissionType::Guard,
                "harvest" => crate::sim::mission::MissionType::Harvest,
                "move" => crate::sim::mission::MissionType::Move,
                other => panic!("unrepresented control {other}"),
            };
            s.rules
                .mission_control
                .set_native_rate_frames_for_test(mission, int(frames));
        }
    }
    assert!(case["rates"].is_null() || row["supplied_rate_frames"].is_object());
    // The original sparse fixture supplies foundation object lists but leaves
    // Cell+124's ground Building bit zero. FNPC consumes the raw bit. Keep this
    // declared prestate here; production placement retains its Building bits.
    for y in 9..12 {
        for x in 6..10 {
            s.sim.substrate.raw_cell_occupation.clear_ground(x, y, 0x80);
        }
    }
    // Native OTHER is a supplied radio object, neither House-listed nor
    // foundation-placed. Use membership owners to reproduce that prestate.
    let owner = s.sim.substrate.entities.get(s.other).unwrap().owner();
    s.sim.update_house_tracking(
        s.other,
        crate::sim::house_tracking::HouseTracking::remove_tracking,
    );
    s.sim.leave_house_base_lists(s.other, owner);
    for y in 20..23 {
        for x in 20..24 {
            s.sim.substrate.occupancy.remove(x, y, s.other);
            s.sim.substrate.raw_cell_occupation.clear_ground(x, y, 0x80);
        }
    }
    if case["passable"] == json!([null]) {
        // FNPC's NULL answer is a declared external input in this one control.
        // Allocate the actor first, then deny every real candidate through the
        // shared raw occupation plane; this does not test a land-cost producer.
        let terrain = s.sim.resolved_terrain.as_ref().unwrap();
        let (width, height) = (terrain.width(), terrain.height());
        for y in 0..height {
            for x in 0..width {
                s.sim.substrate.raw_cell_occupation.mark_ground(x, y, 0xFF);
            }
        }
    }
    let before = &row["before"];
    let raw_nav = nav(&s, &before["miner_nav"]);
    let archive = nav(&s, &before["archive"]).map(|v| match v {
        NavTargetRef::Cell { rx, ry } => TargetKind::Cell(rx, ry),
        NavTargetRef::Building { id } => TargetKind::Entity(id),
        _ => unreachable!(),
    });
    for (id, contact, tether) in [
        (s.miner, "miner_contact", "miner_tether"),
        (s.refinery, "refinery_contact", "refinery_tether"),
        (s.other, "other_contact", "other_tether"),
    ] {
        let contact = before[contact].as_str().map(|n| s.id(n));
        let peer = if id == s.miner { s.refinery } else { s.miner };
        let e = s.sim.substrate.entities.get_mut(id).unwrap();
        e.radio_contacts.clear_all();
        if let Some(contact) = contact {
            e.radio_contacts.set_slot(0, contact);
        }
        e.dock_entered_with = (before[tether] == 1).then_some(peer);
    }
    if let Some(slots) = case["actual_slots"].as_array() {
        // These native controls supply an actual vector independently of the
        // type. Deserialize through the Contacts owner to preserve sparse,
        // duplicate and zero-slot prepared states. Building construction
        // normally installs at least one slot; it is checked separately.
        let slots: Vec<_> = slots
            .iter()
            .map(|slot| slot.as_str().map(|name| s.id(name)))
            .collect();
        s.sim
            .substrate
            .entities
            .get_mut(s.refinery)
            .unwrap()
            .radio_contacts = serde_json::from_value(json!({"slots": slots})).unwrap();
    }
    {
        let e = s.sim.substrate.entities.get_mut(s.miner).unwrap();
        e.health.current = int(&before["health"]);
        e.navigation.nav_com = raw_nav;
        e.set_archive_target(archive);
        e.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(int(&before["miner_mission"])),
            queued: MissionId::from_raw(int(&before["miner_queued"])),
            suspended: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: int(&before["miner_status"]) as u32,
            mission_start_frame: 200,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::from_raw(
                int(&before["dispatch_timer"][0]),
                int(&before["dispatch_timer"][1]),
            ),
        });
        e.install_native_stage_fixture(crate::sim::stage::StageClass::from_native_fixture(
            int(&before["stage"][0]),
            int(&before["stage_changed"]) as u8,
            crate::sim::timer::CdTimer::from_raw(
                int(&before["stage"][1]),
                int(&before["stage"][2]),
            ),
            int(&before["stage"][3]),
            int(&before["stage_step"]),
        ));
        if before["loco_destination"].is_array() {
            let drive = e.locomotor.as_mut().unwrap();
            assert!(drive.ensure_installed_track_state());
            assert!(drive.store_track_destination(
                crate::sim::movement::track_process::TrackFamily::Drive,
                coord(&before["loco_destination"])
            ));
            assert!(drive.store_track_head(
                crate::sim::movement::track_process::TrackFamily::Drive,
                coord(&before["loco_head"])
            ));
        }
    }
    {
        let e = s.sim.substrate.entities.get_mut(s.refinery).unwrap();
        e.health.current =
            case["refinery_health"]
                .as_i64()
                .unwrap_or(if case["unit_repair"] == true {
                    1200
                } else {
                    1000
                }) as i32;
        e.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(case["refinery_current"].as_i64().unwrap_or(5) as i32),
            queued: MissionId::from_raw(int(&before["refinery_queued"])),
            suspended: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 200,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::from_raw(0, 0),
        });
        e.mission_leaf
            .set_building_ready_latch(int(&before["building_repairing"]) as u8);
        // This packet supplies an already-idle building body, independently
        // of the spawned fixture's Unlimbo request. Import its measured
        // current/queued state through the existing body owner before the
        // selected continuation; the native prestate has no queued Idle.
        assert_eq!(before["refinery_bstate"][1], -1);
        e.initialize_building_idle_body(200);
        if before["refinery_bstate"][0] == 0 {
            e.begin_building_body(
                crate::sim::building_construction::BuildingBodyMode::Construction,
                200,
            );
        } else {
            assert_eq!(before["refinery_bstate"][0], 1);
        }
    }
    if let Some(name) = before["pending_entry"].as_str() {
        let pending = s.id(name);
        building_dock::set_pending_entry(&mut s.sim, s.miner, Some(pending));
        let e = s.sim.substrate.entities.get_mut(pending).unwrap();
        e.lifecycle.object_alive = case["pending_alive"] != false;
        if let Some(health) = case["pending_health"].as_i64() {
            e.health.current = health as i32;
        }
    }
    s.sim.scenario_rng = crate::sim::rng::SimRng::new(case["seed"].as_u64().unwrap_or(1));
    radio::take_transmit_log();
    s
}

fn compare(s: &Scene, native: &Value, context: &str) {
    let e = s.sim.substrate.entities.get(s.miner).unwrap();
    let b = s.sim.substrate.entities.get(s.refinery).unwrap();
    let at =
        crate::sim::movement::ground_pose::object_get_coords(e, s.sim.resolved_terrain.as_ref());
    let archive = e.archive_target().map(|v| match v {
        TargetKind::Cell(rx, ry) => NavTargetRef::cell(rx, ry),
        TargetKind::Entity(id) => NavTargetRef::Entity { id },
    });
    let pending = e
        .pending_entry()
        .map(|id| s.name(id))
        .unwrap_or(Value::Null);
    let storage = e.miner.as_ref().map_or_else(
        || json!([0.0, 0.0, 0.0, 0.0]),
        |m| {
            json!([
                m.cargo
                    .iter()
                    .filter(|b| b.resource_type == crate::sim::miner::ResourceType::Ore)
                    .count() as f64,
                m.cargo
                    .iter()
                    .filter(|b| b.resource_type == crate::sim::miner::ResourceType::Gem)
                    .count() as f64,
                0.0,
                0.0
            ])
        },
    );
    let stage = e.native_stage();
    let stage_raw = serde_json::to_value(stage).unwrap();
    let house = s.sim.houses.get(&e.owner()).unwrap();
    let view = s.sim.scenario_rng.logical_view();
    let fields = json!({
        "frame":s.sim.session.binary_frame, "cell":[at.x,at.y,at.z],
        "miner_mission":e.mission.current().raw(),"miner_queued":e.mission.queued().raw(),
        "miner_status":e.mission.handler_state(),
        "dispatch_timer":[e.mission.dispatch_timer().start_frame(),e.mission.dispatch_timer().delay()],
        "nav_queue_count":e.navigation.nav_queue.len(), "miner_nav":nav_name(s,e.navigation.nav_com),
        "archive":nav_name(s,archive), "pending_entry":pending,
        "target":e.attack_target.as_ref().map_or(Value::Null, |t| match t.target { TargetKind::Entity(id)=>s.name(id), TargetKind::Cell(rx,ry)=>json!([rx,ry]) }),
        "health":e.health.current,"strength":s.sim.object_type(e.type_ref(),&s.rules).unwrap().strength,
        "locomotor_powered":u8::from(e.locomotor.as_ref().unwrap().is_powered()),
        "locomotor_kind":if e.locomotor.as_ref().unwrap().active_kind()==crate::rules::locomotor_type::LocomotorKind::Hover {"hover"} else {"drive"},
        "refinery_bstate":[b.building_body_state().unwrap(),b.queued_building_body_state().unwrap()],
        "refinery_queued":b.mission.queued().raw(), "building_repairing":b.building_ready_latch(),
        "stage":[stage.value(),stage.timer().start_frame(),stage.timer().duration(),stage.rate()],
        "stage_changed":stage_raw["changed"],"stage_step":stage_raw["increment"],
        "facing":{"desired":e.body_facing.destination(),"start":e.body_facing.start_word(),"timer_start":e.body_facing.timer_start_frame().map_or(-1,|n|n as i32),"duration":e.body_facing.timer_duration()},
        "unloading":u8::from(e.miner.as_ref().is_some_and(|m|m.unload_active)),
        "harvesting":u8::from(e.miner.as_ref().is_some_and(|m|m.harvesting)),
        "storage":storage,"balance":house.economy.credits,"score":house.economy.harvested_credits,
        "scenario_rng":{"disabled":view.disabled,"index_a":view.index_a,"index_b":view.index_b,"state":view.words}
    });
    for (key, value) in fields.as_object().unwrap() {
        assert_eq!(value, &native[key], "{context}: {key}");
    }
    for (id, field) in [
        (s.miner, "miner_contact"),
        (s.refinery, "refinery_contact"),
        (s.other, "other_contact"),
    ] {
        assert_eq!(
            s.sim
                .substrate
                .entities
                .get(id)
                .unwrap()
                .radio_contacts
                .slot(0)
                .map_or(Value::Null, |id| s.name(id)),
            native[field],
            "{context}: {field}"
        );
    }
    for (id, field) in [(s.miner, "miner_tether"), (s.refinery, "refinery_tether")] {
        assert_eq!(
            u8::from(
                s.sim
                    .substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .dock_entered_with
                    .is_some()
            ),
            int(&native[field]) as u8,
            "{context}: {field}"
        );
    }
    if let Some(drive) = e
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_drive_runtime())
        .and_then(|r| r.retained())
    {
        assert_eq!(
            coord_value(drive.destination()),
            native["loco_destination"],
            "{context}: Drive destination"
        );
        assert_eq!(
            coord_value(drive.head_to()),
            native["loco_head"],
            "{context}: Drive head"
        );
    }
}

fn step(s: &mut Scene, step: &Value, context: &str) {
    let op = &step["input"];
    if let Some(frame) = op["frame"].as_u64() {
        s.sim.session.binary_frame = frame as u32;
    }
    let now = s.sim.session.binary_frame;
    radio::take_transmit_log();
    match op["op"].as_str().unwrap() {
        "has_free_or_own" => assert_eq!(
            s.sim
                .substrate
                .entities
                .get(s.refinery)
                .unwrap()
                .radio_contacts
                .has_free_or(s.miner),
            step["returned"]["admitted"].as_bool().unwrap(),
            "{context}: actual-slot admission"
        ),
        "tick" => {
            s.sim.mission_host_promote(s.miner, now, &s.rules);
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
            s.sim
                .substrate
                .entities
                .get_mut(s.miner)
                .unwrap()
                .tick_native_stage(now as i32);
            crate::sim::miner::miner_system::unit_ai_clear_harvesting(&mut s.sim, s.miner);
        }
        "enter" => assert_eq!(
            crate::sim::mission::enter::mission_enter(&mut s.sim, &s.rules, s.miner),
            int(&step["returned"]),
            "{context}: delay"
        ),
        "unload" => assert_eq!(
            crate::sim::miner::mission_unload(&mut s.sim, &s.rules, s.miner),
            int(&step["returned"]),
            "{context}: delay"
        ),
        "find_bay" => {
            let found = crate::sim::miner::miner_system::find_docking_bay(
                &mut s.sim,
                &s.rules,
                s.miner,
                op["wide"] == 1,
                op["bypass"] == 1,
            );
            assert_eq!(
                found.map_or(Value::Null, |id| s.name(id)),
                step["returned"],
                "{context}: bay"
            );
        }
        "radio" => {
            let message = match int(&op["message"]) {
                14 => RadioMessage::CanDock,
                15 => RadioMessage::CanEnter,
                21 => RadioMessage::DockNow,
                34 => RadioMessage::IsRepairing,
                n => panic!("unrepresented radio {n}"),
            };
            let (sender, target) = if op["from"] == "refinery" {
                (s.refinery, s.miner)
            } else {
                (s.miner, s.refinery)
            };
            assert_eq!(
                radio::transmit(
                    &mut s.sim,
                    sender,
                    target,
                    message,
                    RadioPayload::default(),
                    Some(&s.rules)
                )
                .code() as i32,
                int(&step["returned"]["reply"]),
                "{context}: reply"
            );
            // The Building pointer payload is unused by these production
            // callers; Rust carries only the MOVE_HERE Cell payload.
        }
        "per_cell_enter" => assert_eq!(
            s.sim.unit_dock_now(s.miner, &s.rules, Some(registry())),
            step["returned"]["early_return_before_unit_tail"]
                .as_bool()
                .unwrap(),
            "{context}: early return"
        ),
        "repair_release_power" => {
            // The packet supplies completed service, then executes only the
            // original PowerOn prefix. Production depot release is separately
            // checked through the Building repair mission, including exit movement.
            let e = s.sim.substrate.entities.get_mut(s.miner).unwrap();
            e.health.current = int(&step["after"]["strength"]);
            e.locomotor.as_mut().unwrap().power_on();
        }
        "pending_entry_try" => {
            building_dock::try_pending_entry(&mut s.sim, &s.rules, s.miner);
        }
        "break_other_contact" => {
            radio::transmit_to_contact(&mut s.sim, s.other, RadioMessage::Break, Some(&s.rules));
        }
        "arrival" => {
            let (rx, ry) = cell(&step["returned"]["supplied_arrival_cell"]);
            let e = s.sim.substrate.entities.get_mut(s.miner).unwrap();
            let old = (e.position.rx, e.position.ry);
            let layer = e.occupancy_list_layer().unwrap();
            e.position.rx = rx;
            e.position.ry = ry;
            e.position.sub_x = crate::util::fixed_math::SimFixed::from_num(128);
            e.position.sub_y = crate::util::fixed_math::SimFixed::from_num(128);
            let drive = e.locomotor.as_mut().unwrap();
            assert!(drive.store_track_destination(
                crate::sim::movement::track_process::TrackFamily::Drive,
                None
            ));
            assert!(drive.store_track_head(
                crate::sim::movement::track_process::TrackFamily::Drive,
                None
            ));
            s.sim.substrate.occupancy.move_entity(
                old.0,
                old.1,
                rx,
                ry,
                s.miner,
                layer,
                None,
                crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
            );
            s.sim
                .set_unit_null_destination(s.miner, Some(&s.rules), Some(registry()));
            s.sim
                .per_cell_process(
                    s.miner,
                    crate::sim::movement::PerCellReason::Arrival,
                    Some(&s.rules),
                    Some(registry()),
                )
                .unwrap();
        }
        "building_now_dead_contacts" => {
            let e = s.sim.substrate.entities.get_mut(s.refinery).unwrap();
            e.health.current = 0;
            e.lifecycle.object_alive = false;
            s.sim
                .building_now_dead_contacts(s.refinery, &[s.miner], Some(&s.rules));
        }
        "pointer_expiry" => {
            let e = s.sim.substrate.entities.get(s.refinery).unwrap();
            let at = crate::sim::movement::ground_pose::object_get_coords(
                e,
                s.sim.resolved_terrain.as_ref(),
            );
            let owner = e.owner();
            s.sim.notify_entity_pointer_expired(
                s.miner,
                s.refinery,
                Some(((at.x / 256) as u16, (at.y / 256) as u16)),
                false,
                0,
                false,
                Some(owner),
                PointerExpiryControl::Uninit,
                Some(&s.rules),
                None,
            );
        }
        other => panic!("{context}: operation {other}"),
    }
    let expected: Vec<_> = step["callback_events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e[0] == "send")
        .cloned()
        .collect();
    assert_eq!(sends(s), expected, "{context}: complete radio sequence");
}

fn compare_contact_vector(s: &Scene, native: &Value, context: &str) {
    let contacts = &s
        .sim
        .substrate
        .entities
        .get(s.refinery)
        .unwrap()
        .radio_contacts;
    let slots: Vec<_> = (0..contacts.capacity())
        .map(|index| contacts.slot(index).map_or(Value::Null, |id| s.name(id)))
        .collect();
    assert_eq!(
        json!(contacts.capacity()),
        native["count"],
        "{context}: count"
    );
    assert_eq!(json!(slots), native["slots"], "{context}: actual slots");
}

fn replay(row: &Value) {
    let name = row["input"]["name"].as_str().unwrap();
    let mut s = fixture(row);
    compare(&s, &row["before"], &format!("{name}: initial"));
    if row["supplied_contact_inputs"].is_object() {
        compare_contact_vector(&s, &row["supplied_contact_inputs"]["actual_vector"], name);
    }
    for (index, operation) in row["steps"].as_array().unwrap().iter().enumerate() {
        let context = format!("{name}: step {index} {}", operation["input"]["op"]);
        if operation["contact_vector_before"].is_object() {
            compare_contact_vector(&s, &operation["contact_vector_before"], &context);
        }
        step(&mut s, operation, &context);
        compare(&s, &operation["after"], &context);
        if operation["contact_vector_after"].is_object() {
            compare_contact_vector(&s, &operation["contact_vector_after"], &context);
        }
    }
}

#[test]
fn actual_contact_slots_match_original_admission_receiver_and_scanner() {
    let c = corpus();
    let rows = c["actual_slot_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 9);
    for row in rows {
        replay(row);
    }
}

#[test]
fn shared_enter_radio_and_depot_arrival_match_original_controls() {
    let c = corpus();
    let rows = c["continuation_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 57);
    let mut failures = Vec::new();
    for row in rows {
        // All GameEntity categories are Techno; a non-Techno pointer has no
        // represented counterpart. Its native row stays saved as a boundary.
        if row["input"]["name"] != "pending_not_techno" {
            if std::panic::catch_unwind(|| replay(row)).is_err() {
                failures.push(row["input"]["name"].as_str().unwrap());
            }
        }
    }
    assert!(failures.is_empty(), "failed native controls: {failures:?}");
}

#[test]
fn deposit_busy_retry_and_destroyed_refinery_match_original_histories() {
    let c = corpus();
    let rows = c["deposit_histories"].as_array().unwrap();
    assert_eq!(rows.len(), 3);
    for row in rows {
        replay(row);
    }
}

#[test]
fn independent_pending_entry_survives_save_load_and_changes_the_state_hash() {
    let c = corpus();
    let row = c["continuation_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["input"]["name"] == "repair_per_cell_enter_object")
        .unwrap();
    let mut s = fixture(row);
    s.sim
        .substrate
        .entities
        .get_mut(s.miner)
        .unwrap()
        .mark_live_contact_with(s.refinery);
    s.sim
        .substrate
        .entities
        .get_mut(s.refinery)
        .unwrap()
        .mark_live_contact_with(s.miner);
    s.sim
        .mission_assign_exact(
            s.refinery,
            MissionId::from_known(crate::sim::mission::MissionType::Repair),
            200,
        )
        .unwrap();
    let building = s.sim.substrate.entities.get_mut(s.refinery).unwrap();
    building.mission.set_handler_state(2);
    building.mission_leaf.start_building_repair_progress(200);
    let without_pending = s.sim.state_hash();
    building_dock::set_pending_entry(&mut s.sim, s.miner, Some(s.other));
    assert_ne!(
        s.sim.state_hash(),
        without_pending,
        "pending influences future admission"
    );
    // Save/load the actual retained state. Map reattachment and Scenario load
    // reset are separate contracts; this bincode roundtrip pins these owners.
    let bytes = crate::sim::snapshot::GameSnapshot::save(&s.sim, 0, 0, "independent pending", 0);
    let restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    let unit = restored.substrate.entities.get(s.miner).unwrap();
    assert_eq!(unit.pending_entry(), Some(s.other));
    assert!(unit.radio_contacts.contains(s.refinery));
    let building = restored.substrate.entities.get(s.refinery).unwrap();
    assert!(building.radio_contacts.contains(s.miner));
    assert_eq!(
        building.mission.current(),
        MissionId::from_known(crate::sim::mission::MissionType::Repair)
    );
    assert_eq!(building.mission.handler_state(), 2);
    assert_eq!(
        building
            .mission_leaf
            .as_building()
            .unwrap()
            .repair_progress()
            .value(),
        0
    );
}
