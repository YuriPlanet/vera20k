//! Building44E8F0→451B40 and actual-SHP Anim423AC0 comparisons.
//! Supplied scene/type controls are inputs; expected slots, constructors,
//! frame/timer histories and RNG bytes come only from original execution.

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::art_data::{ArtRegistry, BuildingAnimVariantConfig};
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::sim::mission::leaf::MissionLeafState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::sim::rng::SimRng;
use crate::sim::stage::StageClass;
use serde_json::{Value, json};

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_slot_replacement.expiry.json",
    ))
    .unwrap()
}

fn integer(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn sections(sections: &Value) -> String {
    let mut text = String::new();
    for (section, keys) in sections.as_object().unwrap() {
        text.push_str(&format!("[{section}]\n"));
        for (key, raw) in keys.as_object().unwrap() {
            text.push_str(&format!("{key}={}\n", raw.as_str().unwrap()));
        }
    }
    text
}

fn stock_rules(golden: &Value, case: &Value) -> RuleSet {
    let mut registry = String::from("[BuildingTypes]\n0=NADEPT\n1=GADEPT\n[Animations]\n");
    for (i, anim) in golden["inputs"]["native_anim_types"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        registry.push_str(&format!("{i}={}\n", anim["name"].as_str().unwrap()));
    }
    let layers = golden["inputs"]["layers"].as_array().unwrap();
    let mut stack = RulesLayerStack::new(IniFile::from_str(&format!(
        "{registry}{}",
        sections(&layers[0]["selected"])
    )));
    for layer in &layers[1..] {
        if layer["absent"] == true {
            continue;
        }
        let kind = match layer["file"].as_str().unwrap() {
            "LANGRULE.INI" => RulesLayerKind::LangRule,
            "MPBattleMD.ini" => RulesLayerKind::GameMode,
            "XMP03T4.MAP" => RulesLayerKind::Scenario,
            other => panic!("unknown layer {other}"),
        };
        stack.push(kind, IniFile::from_str(&sections(&layer["selected"])));
    }
    let building = case["building"].as_str().unwrap();
    let mut controls = format!("[{building}]\n");
    for (field, key) in [("unit_repair", "UnitRepair"), ("grinding", "Grinding")] {
        if let Some(value) = case[field].as_i64() {
            controls.push_str(&format!(
                "{key}={}\n",
                if value != 0 { "yes" } else { "no" }
            ));
        }
    }
    stack.push(RulesLayerKind::Scenario, IniFile::from_str(&controls));
    let art_ini = IniFile::from_str(&sections(&golden["inputs"]["art"]["selected"]));
    let mut rules =
        RuleSet::from_processed_rules(&stack.process_with_fixed_art(&art_ini).unwrap()).unwrap();
    let mut art = ArtRegistry::from_ini(&art_ini);
    for anim in golden["inputs"]["native_anim_types"].as_array().unwrap() {
        art.bind_anim_frame_count_for_test(
            anim["name"].as_str().unwrap(),
            integer(&anim["raw_shp_frame_count"]),
        );
    }
    // The extra controls supply raw native type fields and variant strings.
    // They are separate from the physical retail reader assertions above.
    let entry = art.get_mut(building).unwrap();
    if let Some(value) = case["is_anim_delayed_fire"].as_i64() {
        entry.is_anim_delayed_fire = value != 0;
    }
    let template = entry
        .building_anims
        .iter()
        .find(|c| c.native_slot == 10)
        .unwrap()
        .clone();
    let mut supplied = case["slot_names"].as_object().cloned().unwrap_or_default();
    if let Some(name) = case["initial_type"].as_str() {
        supplied
            .entry(integer(&case["slot"]).to_string())
            .or_insert_with(|| json!({}));
        let names = supplied
            .get_mut(&integer(&case["slot"]).to_string())
            .unwrap();
        names["normal"] = json!(name);
        names["damaged"] = json!(name);
    }
    for (slot, variants) in supplied {
        let slot: u8 = slot.parse().unwrap();
        if !entry
            .building_anims
            .iter()
            .any(|config| config.native_slot == slot)
        {
            let mut config = template.clone();
            config.native_slot = slot;
            config.anim_type.clear();
            config.damaged_variant = None;
            config.garrisoned_variant = None;
            config.z_adjust = 0;
            entry.building_anims.push(config);
        }
        let config = entry
            .building_anims
            .iter_mut()
            .find(|c| c.native_slot == slot)
            .unwrap();
        for (variant, name) in variants.as_object().unwrap() {
            let name = name.as_str().unwrap();
            let variant_config = || BuildingAnimVariantConfig {
                anim_type: name.to_owned(),
                loop_start: 0,
                loop_end: 0,
                loop_count: 0,
                rate: 0,
                start_frame: 0,
                ping_pong: false,
            };
            match variant.as_str() {
                "normal" => config.anim_type = name.to_owned(),
                "damaged" => config.damaged_variant = Some(variant_config()),
                "garrisoned" => config.garrisoned_variant = Some(variant_config()),
                other => panic!("unknown native variant {other}"),
            }
        }
    }
    rules.install_art_data(art);
    rules
}

fn scene(golden: &Value, row: &Value) -> (Simulation, RuleSet, u64, u64) {
    let case = &row["input"];
    let rules = stock_rules(golden, case);
    let mut sim = Simulation::new();
    sim.session.binary_frame = 200;
    sim.session.game_options.game_speed = integer(&row["game_speed"]);
    sim.native_unique_ids =
        Some(crate::sim::native_identity::NativeUniqueIdCursor::test_at_current_value(0));
    sim.main_rng =
        SimRng::from_native_state_hex_for_test(row["rng_before"]["main"].as_str().unwrap());
    sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(row["rng_before"]["scenario"].as_str().unwrap());
    let building = case["building"].as_str().unwrap();
    let id = sim.allocate_stable_id();
    let contact = sim.allocate_stable_id();
    let mut actor =
        GameEntity::test_default_of_category(contact, "HTNK", "A", 10, 10, EntityCategory::Unit);
    actor.type_ref = sim.interner.intern("HTNK");
    actor.owner = sim.interner.intern("A");
    sim.substrate.entities.insert(actor);
    let mut entity =
        GameEntity::test_default_of_category(id, building, "A", 6, 9, EntityCategory::Structure);
    entity.type_ref = sim.interner.intern(building);
    entity.owner = sim.interner.intern("A");
    entity.health.current = case["health"].as_i64().unwrap_or(1200) as i32;
    entity.install_native_stage_fixture(StageClass::constructed(200));
    entity.mission_leaf = MissionLeafState::constructed(EntityCategory::Structure, 200);
    entity
        .mission_leaf
        .set_building_ready_latch(case["ready"].as_u64().unwrap_or(0) as u8);
    entity.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(case["mission"].as_i64().unwrap_or(20) as i32),
        queued: MissionId::from_raw(case["queued"].as_i64().unwrap_or(-1) as i32),
        suspended: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::from_raw(200, 0),
    });
    let contacts = &row["before"]["contacts"];
    let count = case["contact_count"]
        .as_u64()
        .unwrap_or(contacts.as_array().unwrap().len() as u64) as usize;
    entity.radio_contacts = crate::sim::radio::contacts::Contacts::with_capacity(count);
    for (i, pointer) in contacts.as_array().unwrap().iter().take(count).enumerate() {
        if pointer.as_u64().unwrap() != 0 {
            entity.radio_contacts.set_slot(i, contact);
        }
    }
    // Negative native vector counts have no equivalent valid Rust vector;
    // this control supplies its empty predicate, without certifying malformed state.
    if case["garrison_count"].as_i64().unwrap_or(0) > 0 {
        let mut cargo = crate::sim::passenger::PassengerCargo::new(1, 0);
        assert!(cargo.board(contact, 1));
        entity.passenger_role = crate::sim::passenger::PassengerRole::Transport { cargo };
    }
    sim.substrate.entities.insert(entity);
    let slot = integer(&case["slot"]) as u8;
    let expired = sim
        .set_building_anim_slot(
            id,
            slot,
            case["health"].as_i64().unwrap_or(1200) <= 600,
            false,
            0,
            &rules,
        )
        .unwrap();
    assert!(!sim.anim(expired).unwrap().completed());
    sim.substrate
        .anims
        .get_mut(expired)
        .unwrap()
        .install_building_expiry_fixture(
            case["slot_marker"].as_i64().unwrap_or(1) != 0,
            case["completion"].as_i64().unwrap_or(0) != 0,
        );
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    entity.lifecycle.object_alive = case["alive"].as_i64().unwrap_or(1) != 0;
    if let Some(health) = case["callback_health"].as_i64() {
        entity.health.current = health as i32;
    }
    if case["unmatched"] == true {
        entity.building_anim_slots[usize::from(slot)] = None;
        sim.rebuild_building_anim_slot_indices();
    }
    (sim, rules, id, expired)
}

fn anim_state(sim: &Simulation, anim: &crate::sim::anim_class::AnimObject) -> Value {
    let runtime = &anim.runtime;
    json!({
        "type_name": sim.interner.resolve(anim.type_id), "native_id": anim.native_unique_id,
        "location": [anim.world_coord.x, anim.world_coord.y, anim.world_coord.z],
        "z_adjust": anim.z_adjust, "draw_flags": anim.draw_flags,
        "completion_marker": u8::from(anim.completed()), "slot_marker": u8::from(anim.is_building_anim()),
        "owner_object": anim.owner_entity.unwrap_or(0),
        "runtime": {
            "current_frame": runtime.current_frame, "frame_step": runtime.frame_step,
            "delay_remaining": runtime.delay_remaining, "rate_reload": runtime.rate_reload,
            "frame_timer": [runtime.frame_timer.start_frame(), runtime.frame_timer.duration()],
            "loop_remaining": runtime.loop_remaining, "first_ai_guard": u8::from(runtime.first_ai_guard),
            "constructor_reverse": u8::from(runtime.constructor_reverse),
            "inactive": u8::from(runtime.inactive), "paused": u8::from(runtime.paused),
        }
    })
}

fn assert_boundary(sim: &Simulation, id: u64, native: &Value, name: &str) {
    let expected_anims = native["anims"].as_array().unwrap();
    let rust_anims: Vec<_> = sim.substrate.anims.iter().map(|(_, anim)| anim).collect();
    assert_eq!(
        rust_anims.len(),
        expected_anims.len(),
        "{name}: live/deferred store"
    );
    for anim in rust_anims {
        let expected = expected_anims
            .iter()
            .find(|a| integer(&a["native_id"]) == anim.native_unique_id)
            .unwrap();
        let observed = anim_state(sim, anim);
        for (key, value) in observed.as_object().unwrap() {
            assert_eq!(
                value, &expected[key],
                "{name}: Anim {} {key}",
                anim.native_unique_id
            );
        }
    }
    let expected_ids: Vec<_> = native["slots"]
        .as_array()
        .unwrap()
        .iter()
        .map(|ptr| {
            if ptr == 0 {
                None
            } else {
                Some(integer(
                    &expected_anims
                        .iter()
                        .find(|a| a["pointer"] == *ptr)
                        .unwrap()["native_id"],
                ))
            }
        })
        .collect();
    let observed_ids: Vec<_> = sim
        .entities()
        .get(id)
        .unwrap()
        .building_anim_slots
        .iter()
        .map(|id| id.map(|id| sim.anim(id).unwrap().native_unique_id))
        .collect();
    assert_eq!(observed_ids, expected_ids, "{name}: all21 slots");
    let observed_logic: Vec<_> = sim
        .logic_order()
        .iter()
        .map(|id| sim.anim(*id).unwrap().native_unique_id)
        .collect();
    let expected_logic: Vec<_> = native["logic"]
        .as_array()
        .unwrap()
        .iter()
        .map(|ptr| {
            integer(
                &expected_anims
                    .iter()
                    .find(|a| a["pointer"] == *ptr)
                    .unwrap()["native_id"],
            )
        })
        .collect();
    assert_eq!(observed_logic, expected_logic, "{name}: Logic order");
}

#[test]
fn original_64_slot_expiry_controls_and_actual_sprite_frame_histories() {
    let golden = corpus();
    let rows = golden["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 64);
    for row in rows {
        let case = &row["input"];
        let name = case["name"].as_str().unwrap();
        let (mut sim, rules, id, expired) = scene(&golden, row);
        assert_boundary(&sim, id, &row["before"], name);
        let before = sim.entities().get(id).unwrap();
        let clocks = (
            *before.native_stage(),
            *before.mission_leaf.as_building().unwrap().repair_progress(),
            before.mission_leaf.as_building().unwrap().ready_latch(),
        );
        if case["natural"] == true || case["cancel_ai"] == true {
            if case["cancel_ai"] == true {
                sim.substrate
                    .anims
                    .get_mut(expired)
                    .unwrap()
                    .runtime
                    .inactive = true;
            }
            for frame in row["frames"].as_array().unwrap() {
                sim.session.binary_frame = integer(&frame["frame"]) as u32;
                sim.visit_anim(expired, &rules, None);
                let anim = sim.anim(expired).unwrap();
                assert_eq!(
                    anim.runtime.current_frame,
                    integer(&frame["current"]),
                    "{name}: {frame}"
                );
                assert_eq!(
                    [
                        anim.runtime.frame_timer.start_frame(),
                        anim.runtime.frame_timer.duration()
                    ],
                    [integer(&frame["timer"][0]), integer(&frame["timer"][2])],
                    "{name}: {frame}"
                );
                assert_eq!(
                    anim.runtime.loop_remaining,
                    integer(&frame["loop"]) as u8,
                    "{name}: {frame}"
                );
                assert_eq!(
                    anim.completed(),
                    frame["completion_marker"] == 1,
                    "{name}: {frame}"
                );
                assert_eq!(
                    anim.runtime.first_ai_guard,
                    frame["first_guard"] == 1,
                    "{name}: {frame}"
                );
                assert_eq!(
                    !sim.substrate.pending_delete.contains(&expired),
                    frame["alive"] == 1,
                    "{name}: {frame}"
                );
            }
        } else if case["scalar_clear"] == true {
            sim.clear_building_anim_slot(id, integer(&case["slot"]) as u8);
        } else {
            sim.building_anim_pointer_expired(expired, Some(&rules));
        }
        assert_boundary(&sim, id, &row["after"], name);
        let after = sim.entities().get(id).unwrap();
        assert_eq!(
            (
                *after.native_stage(),
                *after.mission_leaf.as_building().unwrap().repair_progress(),
                after.mission_leaf.as_building().unwrap().ready_latch()
            ),
            clocks,
            "{name}: independent body/service clocks"
        );
        assert_eq!(
            sim.main_rng.native_state_hex(),
            row["rng_after"]["main"].as_str().unwrap(),
            "{name}: Main RNG"
        );
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"]["scenario"].as_str().unwrap(),
            "{name}: Scenario RNG"
        );
    }
}

#[test]
fn original_stock_idle_body_publishes_ready_without_waiting_for_slot_completion() {
    let golden = corpus();
    for row in golden["stock_body_readiness"].as_array().unwrap() {
        let fixture = &golden["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["input"]["building"] == row["building"])
            .unwrap();
        let (mut sim, rules, id, _) = scene(&golden, fixture);
        sim.clear_all_building_anim_slots(id);
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.begin_building_body(
            crate::sim::building_construction::BuildingBodyMode::Idle,
            200,
        );
        entity
            .mission_leaf
            .set_building_ready_latch(integer(&row["before"]["building"]["done"]) as u8);
        entity.advance_building_body(
            200,
            false,
            rules
                .object(row["building"].as_str().unwrap())
                .unwrap()
                .has_turret,
            &sim.session.game_options,
        );
        assert_eq!(
            entity.building_ready_latch(),
            integer(&row["after"]["building"]["done"]) as u8
        );
        assert_eq!(*entity.native_stage(), StageClass::constructed(200));
    }
}

#[test]
fn snapshot_restores_completed_retraction_and_independent_building_marker() {
    let golden = corpus();
    for name in ["retract_completed", "slot_marker_false"] {
        let row = golden["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["input"]["name"] == name)
            .unwrap();
        let (sim, rules, id, expired) = scene(&golden, row);
        let bytes = bincode::serialize(&sim).unwrap();
        let mut restored: Simulation = bincode::deserialize(&bytes).unwrap();
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(
            restored.anim(expired).unwrap().completed(),
            sim.anim(expired).unwrap().completed()
        );
        assert_eq!(
            restored.anim(expired).unwrap().is_building_anim(),
            sim.anim(expired).unwrap().is_building_anim()
        );
        restored.building_anim_pointer_expired(expired, Some(&rules));
        assert_boundary(&restored, id, &row["after"], name);
    }
}
