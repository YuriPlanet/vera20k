//! Consuming controls for native scenario House names and authored admission.
//! See tools/input_oracle/campaign_house_names.md for original-code bounds.

use super::{HouseDifficulty, NormalizedSkirmishSlot, populate_launch_houses};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::snapshot::{GameSnapshot, SnapshotRestoreError};
use crate::sim::world::Simulation;
use crate::skirmish_launch::{LaunchCountry, LaunchStartPosition, LaunchTeam};
use serde_json::Value;

fn corpus() -> Value {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/input_oracle/campaign_house_names.json",
    ))
    .unwrap();
    assert_eq!(
        native["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    native
}

fn generated_houses(rules: &RuleSet, native: &Value) -> Simulation {
    let mut sim = Simulation::with_seed(31);
    let slots: Vec<_> = native["houses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|house| {
            let slot = house["slot"].as_u64().unwrap() as usize;
            NormalizedSkirmishSlot {
                owner_name: if slot == 0 {
                    "Computer".to_owned()
                } else {
                    format!("Computer{slot}")
                },
                country: LaunchCountry::from_country_index(u32::from(
                    rules
                        .country_index("Russians")
                        .expect("fixture declares Russians")
                        .0,
                )),
                color_index: slot as u8,
                start_position: LaunchStartPosition::Position(slot as u8),
                team: LaunchTeam::None,
                is_human: house["generated_kind"] == "human",
                observer: false,
                source_human_index: None,
                difficulty: HouseDifficulty::Normal,
            }
        })
        .collect();
    populate_launch_houses(&mut sim, &slots, rules);
    sim
}

#[test]
fn generated_names_and_first_array_ordinal_match_original_controls() {
    let native = corpus();
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Russians\n[Russians]\nSide=Nod\n",
    ))
    .unwrap();
    let mut sim = generated_houses(&rules, &native);
    let registered = sim.houses.order().to_vec();
    for house in native["houses"].as_array().unwrap() {
        let slot = house["slot"].as_u64().unwrap() as usize;
        assert_eq!(
            sim.houses[&registered[slot]].native_scenario_name(),
            house["native_name"].as_str()
        );
    }
    let generated: Vec<_> = registered
        .iter()
        .map(|name| sim.houses[name].clone())
        .collect();
    for row in native["lookup_rows"].as_array().unwrap() {
        // Each control's HouseClass array holds exactly its listed Houses.
        sim.houses.clear_for_test();
        for slot in row["house_order"].as_array().unwrap() {
            sim.houses
                .register(generated[slot.as_u64().unwrap() as usize].clone());
        }
        let rng = sim.rng_state();
        let found = sim.scenario_house_named(row["token"].as_str().unwrap());
        let expected = row["resolved_house"]
            .as_u64()
            .map(|slot| registered[slot as usize]);
        assert_eq!(found, expected, "native FromName control {}", row["name"]);
        let ordinal =
            found.and_then(|owner| sim.houses.order().iter().position(|&entry| entry == owner));
        assert_eq!(
            ordinal.map_or(-1, |index| index as i64),
            row["result_index"].as_i64().unwrap()
        );
        assert_eq!(
            sim.rng_state(),
            rng,
            "the compared lookup leaves every RNG stream unchanged"
        );
    }
}

#[test]
fn rejected_authored_buildings_match_original_without_constructing() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let native = corpus();
    for row in native["building_reader_rows"].as_array().unwrap() {
        if row["outcome"] != "reader_return" {
            continue;
        }
        let mut sim = generated_houses(&retail.rules, &native);
        let registered = sim.houses.order().to_vec();
        sim.houses.reorder_for_test(
            &row["house_order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|slot| registered[slot.as_u64().unwrap() as usize])
                .collect::<Vec<_>>(),
        );
        sim.session.set_current_house_for_test(Some(
            registered[row["current_house"].as_u64().unwrap() as usize],
        ));
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
                row["game_mode"].as_i64().unwrap() != 0,
            ),
        );
        let ini = IniFile::from_str(&format!(
            "[Basic]\nNewINIFormat={}\n[Structures]\n0={}\n",
            row["basic"]["NewINIFormat"].as_str().unwrap(),
            row["structures"]["0"].as_str().unwrap()
        ));
        let entities = crate::map::entities::parse_map_entities(&ini);
        assert_eq!(entities.len(), 1);
        let before = sim.state_hash();
        let rng = sim.rng_state();
        assert_eq!(
            sim.spawn_from_map(&entities, Some(&retail.rules)),
            0,
            "native rejection {}",
            row["name"]
        );
        assert!(sim.entities().is_empty());
        assert_eq!(
            sim.state_hash(),
            before,
            "rejection must spend no object identity or constructor state: {}",
            row["name"]
        );
        assert_eq!(
            sim.rng_state(),
            rng,
            "all native-compared RNG buffers stay unchanged: {}",
            row["name"]
        );
    }
}

#[test]
fn mobile_readers_admit_current_house_and_resolve_first_computer() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let mut sim = generated_houses(&retail.rules, &corpus());
    let human = sim.houses.order()[0];
    let ai = sim.houses.order()[1];
    sim.session.set_current_house_for_test(Some(human));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.intern_rule_type_ids(&retail.rules);
    sim.resolve_type_handles(&retail.rules);
    crate::sim::arena_fixture::flat_ground(&mut sim, &retail.rules);
    let ini = IniFile::from_str(
        "[Units]\n0=<human player>,HTNK,256,8,8,0,Guard,None\n1=Computer,HTNK,256,12,8,0,Guard,None\n[Infantry]\n0=<human player>,E2,256,8,12,0,Guard,0,None\n[Aircraft]\n0=<human player>,BEAG,256,12,12,0,Guard,None\n",
    );
    let entities = crate::map::entities::parse_map_entities(&ini);
    assert_eq!(sim.spawn_from_map(&entities, Some(&retail.rules)), 4);
    for (cell, owner, type_name) in [
        ((8, 8), human, "HTNK"),
        ((12, 8), ai, "HTNK"),
        ((8, 12), human, "E2"),
        ((12, 12), human, "BEAG"),
    ] {
        let actor = sim
            .entities()
            .values()
            .find(|actor| (actor.position.rx(), actor.position.ry()) == cell)
            .unwrap();
        assert_eq!(actor.owner(), owner);
        assert_eq!(sim.interner.resolve(actor.type_ref()), type_name);
    }
}

#[test]
fn native_names_survive_load_and_hash_independently_of_runtime_keys() {
    let native = corpus();
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=Russians\n[Russians]\nSide=Nod\n",
    ))
    .unwrap();
    let mut sim = generated_houses(&rules, &native);
    let first_ai = sim.houses.order()[1];
    let second_ai = sim.houses.order()[2];
    sim.session
        .set_current_house_for_test(Some(sim.houses.order()[0]));
    sim.houses
        .get_mut(&first_ai)
        .unwrap()
        .set_is_human_for_test(true);
    let bytes = GameSnapshot::save(&sim, 0, 0, "native-house-name", 0);
    let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
    loaded.restore_after_snapshot_load().unwrap();
    assert_eq!(loaded.scenario_house_named("Computer"), Some(first_ai));
    assert_eq!(loaded.scenario_house_named("Computer1"), None);
    let before = loaded.state_hash();
    // Codec corruption is deliberate input, not an extra field writer API.
    let mut house = serde_json::to_value(&loaded.houses[&first_ai]).unwrap();
    house["native_scenario_name"] = "Different".into();
    loaded
        .houses
        .replace_for_test(serde_json::from_value(house.clone()).unwrap());
    assert_ne!(loaded.state_hash(), before);
    assert_eq!(loaded.scenario_house_named("Computer"), Some(second_ai));
    for invalid in [
        Value::Null,
        "123456789012345678901".into(),
        "bad\0name".into(),
        "\u{100}".into(),
    ] {
        let mut candidate = GameSnapshot::load(&bytes).unwrap().sim;
        house["native_scenario_name"] = invalid;
        candidate
            .houses
            .replace_for_test(serde_json::from_value(house.clone()).unwrap());
        assert!(
            matches!(candidate.restore_after_snapshot_load(), Err(SnapshotRestoreError::InvalidHouseScenarioName { owner, .. }) if owner == first_ai)
        );
    }
}
