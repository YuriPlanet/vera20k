//! Consumed original708C30 controls, including its signed64/low32 ftol trap.
use super::{GameEntity, rounded_pip_quantity};
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::retail_ini_fixture::{
    retail_battle_rules_for_map, retail_battle_rules_for_map_with_patch,
};
use crate::rules::ruleset::RuleSet;
use crate::sim::capture_manager::CaptureManagerState;
use crate::sim::miner::{CargoBale, Miner, MinerConfig, MinerKind, ResourceType};
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use crate::sim::slave_manager::SlaveLink;

fn native() -> &'static serde_json::Value {
    crate::test_fixture::json("tools/procedural_drawing_oracle/ammo_pips.json")
}

fn int(value: &serde_json::Value) -> i32 {
    value.as_i64().unwrap() as i32
}

#[test]
fn rounded_quantities_match_original_ftol_before_the_positive_minimum() {
    assert_eq!(native()["original_code_unchanged"], true);
    let mut checked = 0;
    for row in native()["fill_controls"].as_array().unwrap() {
        let arithmetic = &row["arithmetic"];
        let Some(amount) = arithmetic["amount"].as_f64() else {
            assert!(row["ftol"].as_array().unwrap().is_empty());
            continue;
        };
        let denominator = arithmetic["denominator"].as_f64().unwrap();
        assert_eq!(amount, f64::from(amount as i32), "{row}");
        assert_eq!(denominator, f64::from(denominator as i32), "{row}");
        let conversion = row["ftol"].as_array().unwrap().last().unwrap();
        assert_eq!(
            rounded_pip_quantity(
                amount as i32,
                denominator as i32,
                int(&arithmetic["maximum"])
            ),
            conversion["eax"].as_u64().unwrap() as u32 as i32,
            "{row}"
        );
        checked += 1;
    }
    assert!(checked >= 24);
}

fn check_fill(row: &serde_json::Value, rules: &RuleSet) {
    let input = &row["input"];
    let name = input["type_name"].as_str().unwrap();
    let object = rules.object(name).unwrap();
    let category = EntityCategory::from(object.category);
    let mut entity = GameEntity::test_default_of_category(1, name, "Americans", 0, 0, category);
    entity.initialize_ammo_from_type(object);
    if let Some(current) = input["current_ammo"].as_i64() {
        entity
            .ammo_mut()
            .unwrap()
            .set_current_for_test(current as i32);
    }
    if let Some(count) = input["capture_count"].as_u64() {
        let victims: Vec<_> = (10..10 + count).collect();
        entity.capture_manager = Some(CaptureManagerState::with_victims_for_test(
            object.pip_max(rules),
            true,
            &victims,
        ));
    }
    if let Some(count) = input["cargo_count"].as_u64() {
        let mut cargo = PassengerCargo::new(object.passengers as u32, object.size_limit);
        for id in 10..10 + count {
            cargo.board_forced_chain(&[(id, 1.0)]);
        }
        entity.passenger_role = PassengerRole::transport(cargo);
    }
    if let Some(slots) = input["storage"].as_array() {
        let total = int(&row["arithmetic"]["storage_total"]);
        assert!(
            total >= 0,
            "raw signed/fractional storage is outside the bale owner"
        );
        assert!(slots.iter().all(|slot| {
            let amount = slot.as_f64().unwrap();
            amount >= 0.0 && amount.fract() == 0.0
        }));
        assert_eq!(
            slots.iter().map(|slot| slot.as_f64().unwrap()).sum::<f64>(),
            f64::from(total)
        );
        let bales = vec![
            CargoBale {
                tiberium_type: crate::rules::tiberium_type::TiberiumTypeId(0),
                resource_type: ResourceType::Ore,
                value: 25
            };
            total as usize
        ];
        let mut miner = Miner::new(MinerKind::War, &MinerConfig::default(), 0);
        miner.set_cargo_for_test(bales.clone());
        entity.miner = Some(miner);
        // The same native integer Storage total is already represented by
        // the slave owner's bales. Test its fallback without claiming a
        // complete SLAV draw/lifecycle comparison or inventing new goldens.
        let mut slave_storage = entity.clone();
        slave_storage.miner = None;
        slave_storage.slave = SlaveLink::for_test(None, bales);
        assert_eq!(
            slave_storage.techno_pip_fill_level(object, rules, None),
            int(&row["result"]),
            "slave storage: {row}"
        );
    }
    assert_eq!(object.ammo, int(&row["type_fields"]["ammo"]), "{row}");
    assert_eq!(object.storage, int(&row["type_fields"]["storage"]), "{row}");
    assert_eq!(
        object.pip_scale as i32,
        int(&row["type_fields"]["pip_scale"]),
        "{row}"
    );
    assert_eq!(object.pip_max(rules), int(&row["pip_max"]), "{row}");
    assert_eq!(
        entity.techno_pip_fill_level(object, rules, None),
        int(&row["result"]),
        "{row}"
    );
    assert_eq!(row["actor_unchanged"], true);
    assert!(row["rng_entries"].as_array().unwrap().is_empty());
}

#[test]
fn base_fill_query_matches_original_on_existing_live_state_owners() {
    let Some(retail) = retail_battle_rules_for_map("XMP03T4.MAP") else {
        return;
    };
    let controls = native()["fill_controls"].as_array().unwrap();
    assert!(controls.len() >= 26);
    let mut configured = std::collections::BTreeMap::<String, Vec<&serde_json::Value>>::new();
    for row in controls {
        if let Some(configuration) = row["input"]["configuration"].as_object() {
            let name = row["input"]["type_name"].as_str().unwrap();
            let mut patch = format!("[{name}]\n");
            for (key, value) in configuration {
                patch.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
            }
            configured.entry(patch).or_default().push(row);
        } else {
            check_fill(row, &retail.rules);
        }
    }
    for (patch, rows) in configured {
        let fixture =
            retail_battle_rules_for_map_with_patch("XMP03T4.MAP", &IniFile::from_str(&patch))
                .unwrap();
        for row in rows {
            check_fill(row, &fixture.rules);
        }
    }
}
