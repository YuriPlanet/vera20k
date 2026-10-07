//! Original selector, charge arithmetic and FireAt rearm-copy comparisons.
//! Passenger/cargo caller integration belongs to the passenger-owner tests.
use super::*;
use crate::rules::ini_parser::IniSection;
use crate::rules::object_type::ObjectCategory;
use crate::sim::combat::world_receiver::fireat_rearm_frames;
use crate::sim::timer::CdTimer;
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/ifv_turret_switching.json",
    ))
    .unwrap()
}

fn int(row: &Value, key: &str) -> i32 {
    row[key].as_i64().unwrap() as i32
}

fn object(native: &Value, charge: bool, count: i32, gunner: bool, gattling: bool) -> ObjectType {
    let root = native["physical"]["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["file"] == "rulesmd.ini")
        .unwrap();
    let mut section = IniSection::new("FV".into());
    // Read the physical authored values; native output table entries remain
    // expectations, never fixture inputs to another implementation.
    for (key, value) in root["sections"]["FV"].as_object().unwrap() {
        section.set(key, value.as_str().unwrap());
    }
    section.set("IsChargeTurret", if charge { "yes" } else { "no" });
    section.set("TurretCount", &count.to_string());
    section.set("Gunner", if gunner { "yes" } else { "no" });
    section.set("IsGattling", if gattling { "yes" } else { "no" });
    ObjectType::from_ini_section("FV", &section, ObjectCategory::Vehicle)
}

fn assert_selection(entity: &GameEntity, expected: &Value, context: &str) {
    assert_eq!(
        (
            entity.current_weapon_number(),
            entity.current_turret_index()
        ),
        (int(expected, "weapon"), int(expected, "turret")),
        "{context}"
    );
}

fn replay_selector_events(entity: &mut GameEntity, object: &ObjectType, events: &Value) {
    let before_timer = entity.rearm_timer;
    let before_delay = entity.charge_turret_delay;
    for event in events.as_array().unwrap() {
        assert_eq!(event["event"], "set_gunner", "unexpected native event");
        entity.set_gunner_weapon(int(event, "mode"), object);
    }
    assert_eq!(entity.rearm_timer, before_timer);
    assert_eq!(entity.charge_turret_delay, before_delay);
}

#[test]
fn gunner_constructor_and_selector_match_original_components() {
    let native = corpus();
    for row in native["lifecycle"]["rows"].as_array().unwrap() {
        let object = object(&native, row["charge"].as_bool().unwrap(), 4, true, false);
        let mut entity = GameEntity::test_default(1, "FV", "Americans", 5, 5);
        assert_selection(
            &entity,
            &row["initialization"]["base_constructor"],
            "Techno constructor",
        );
        assert_eq!(
            entity.charge_turret_delay,
            native["charge"]["constructor_delay"].as_i64().unwrap() as i32,
        );
        replay_selector_events(&mut entity, &object, &row["initialization"]["events"]);
        assert_selection(&entity, &row["initialization"]["after"], "Unit gunner init");
        for phase in ["receive", "remove", "retry"] {
            let context = format!("mode={} charge={} {phase}", row["mode"], row["charge"]);
            assert_selection(&entity, &row[phase]["before"], &context);
            replay_selector_events(&mut entity, &object, &row[phase]["events"]);
            assert_selection(&entity, &row[phase]["after"], &context);
        }
    }
}

#[test]
fn native_selector_ignores_turret_count_and_gattling_gates() {
    let native = corpus();
    for row in native["lifecycle"]["initialization_controls"]
        .as_array()
        .unwrap()
    {
        let object = object(
            &native,
            false,
            int(row, "count"),
            row["gunner"].as_bool().unwrap(),
            false,
        );
        let mut entity = GameEntity::test_default(1, "FV", "Americans", 5, 5);
        replay_selector_events(&mut entity, &object, &row["initialization"]["events"]);
        assert_selection(&entity, &row["initialization"]["after"], "constructor gate");
        replay_selector_events(&mut entity, &object, &row["events"]);
        assert_selection(&entity, &row["init_from_type"], "InitFromType gate");
        replay_selector_events(&mut entity, &object, &row["remove_null"]["events"]);
        assert_selection(&entity, &row["remove_null"]["after"], "null gunner removal");
    }
    let row = &native["lifecycle"]["gattling_selector"];
    let object = object(&native, false, 4, true, true);
    let mut entity = GameEntity::test_default(1, "FV", "Americans", 5, 5);
    replay_selector_events(&mut entity, &object, &row["initialization"]["events"]);
    replay_selector_events(&mut entity, &object, &row["select"]["events"]);
    assert_selection(&entity, &row["select"]["after"], "gattling selector");
}

#[test]
fn charge_turret_ai_and_restored_owner_match_original_arithmetic() {
    let native = corpus();
    for row in native["charge"]["ai_rows"].as_array().unwrap() {
        let input = &row["inputs"];
        let context = input["name"].as_str().unwrap();
        let object = object(
            &native,
            input["charge"].as_bool().unwrap(),
            int(input, "count"),
            true,
            input["gattling"].as_bool().unwrap(),
        );
        let mut entity = GameEntity::test_default(1, "FV", "Americans", 5, 5);
        entity.current_weapon_number = int(input, "initial_weapon");
        entity.current_turret_index = int(input, "initial_turret");
        entity.charge_turret_delay = int(input, "charge_delay");
        entity.rearm_timer = CdTimer::from_raw(int(input, "start"), int(input, "duration"));
        let before_timer = entity.rearm_timer;
        let before_delay = entity.charge_turret_delay;
        let encoded = bincode::serialize(&entity).unwrap();
        let mut restored: GameEntity = bincode::deserialize(&encoded).unwrap();
        for entity in [&mut entity, &mut restored] {
            entity.update_charge_turret(int(input, "frame"), &object);
            assert_selection(entity, &row["after"], context);
            assert_eq!(entity.rearm_timer, before_timer, "timer: {context}");
            assert_eq!(entity.charge_turret_delay, before_delay, "delay: {context}");
        }
    }
}

#[test]
fn fireat_rearm_copy_matches_original_normal_and_disk_writers() {
    let native = corpus();
    for row in native["charge"]["fire_writers"].as_array().unwrap() {
        let mut entity = GameEntity::test_default(1, "FV", "Americans", 5, 5);
        entity.berserk.active = row["berserk"].as_bool().unwrap();
        let selection = (
            entity.current_weapon_number(),
            entity.current_turret_index(),
        );
        // GetROF itself is the oracle's explicit supplied boundary. The Disk
        // writer receives it directly; ordinary FireAt applies its shared
        // berserk adjustment before both writers enter the retained owner.
        let supplied_get_rof = int(row, "supplied_get_rof");
        let duration = if row["disk_laser"].as_bool().unwrap() {
            supplied_get_rof
        } else {
            fireat_rearm_frames(supplied_get_rof, entity.berserk.active)
        };
        entity.rearm_after_fire(int(row, "frame"), duration);
        assert_eq!(
            entity.charge_turret_delay,
            int(row, "charge_delay"),
            "{row}"
        );
        assert_eq!(
            entity.rearm_timer.start_frame(),
            int(row, "timer_start"),
            "{row}"
        );
        assert_eq!(
            entity.rearm_timer.duration(),
            int(row, "timer_duration"),
            "{row}"
        );
        assert_eq!(
            (
                entity.current_weapon_number(),
                entity.current_turret_index()
            ),
            selection,
            "FireAt does not update the draw index: {row}"
        );
    }
}
