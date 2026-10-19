//! Selected Scatter controls execute original536D80/730FE0/6FFE00/4C65E0.
//! The shared ground-input fixture supplies actor construction/flat terrain;
//! these checks cover input order, event bytes and cue admission, not movement.

use super::queue_scatter_orders;
use crate::audio::events::GameSoundEvent;
use crate::rules::{area_guard_tests as native, retail_ini_fixture};
use crate::sim::command::{Command, CommandRecord, ScatterRecord};
use serde_json::Value;

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/input_oracle/player_scatter.json")
}

#[test]
#[ignore = "slow; run when changing what it covers"]
fn player_scatter_matches_native_selected_order_cue_and_no_voice_rng() {
    let controls = corpus();
    let shared = native::native();
    let mut checked = 0;
    for row in controls["rows"].as_array().unwrap() {
        let Some(retail) = retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let mut f = super::area_guard_tests::fixture(&shared, row, retail);
        let selected: Vec<_> = row["input"]["selection"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| {
                name.as_str()
                    .and_then(|name| f.ids.get(name))
                    .copied()
                    .unwrap_or(u64::MAX)
            })
            .collect();
        let boundary = &row["boundaries"][0];
        let last = boundary["last_sequence"].as_u64().unwrap();
        let calls: Vec<_> = row["ordered_calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call["sequence"].as_u64().unwrap() <= last)
            .collect();
        let before_rng = f.sim.rng_state();
        let sounds = queue_scatter_orders(&mut f.sim, &f.rules, &selected);
        assert_eq!(
            boundary["before"]["rng"], boundary["after"]["rng"],
            "{}: native input has no RNG draws",
            row["name"]
        );
        assert_eq!(
            f.sim.rng_state(),
            before_rng,
            "{}: input never draws voice RNG",
            row["name"]
        );
        let expected_sounds: Vec<_> = calls
            .iter()
            .filter(|call| call["kind"] == "ScatterSound")
            .map(|call| call["sound_name"].as_str().unwrap())
            .collect();
        let actual_sounds: Vec<_> = sounds
            .iter()
            .map(|event| {
                let GameSoundEvent::UiSound { sound_id } = event else {
                    panic!("Scatter must only emit its centered command cue")
                };
                sound_id.as_str()
            })
            .collect();
        assert_eq!(
            actual_sounds, expected_sounds,
            "{}: centered cue",
            row["name"]
        );
        let expected_ids: Vec<_> = calls
            .iter()
            .filter(|call| call["kind"] == "TargetEvent_Construct")
            .map(|call| f.native_ids[&(call["args"][2].as_u64().unwrap())])
            .collect();
        let pending = f.sim.pending_command_snapshot();
        let actual_ids: Vec<_> = pending
            .iter()
            .map(|cmd| {
                assert_eq!(cmd.owner, f.sim.session.current_house().unwrap());
                let Command::Scatter { entity_id } = cmd.payload else {
                    panic!("not a Scatter event");
                };
                let record = f.sim.encode_scatter_record(cmd.owner, entity_id).unwrap();
                assert_eq!(
                    ScatterRecord::decode(&record).unwrap().entity_id as u64,
                    entity_id
                );
                entity_id
            })
            .collect();
        // Native full-OutList still constructs both events and cues; the
        // current offline semantic ingress is not its bounded queue owner.
        // Compare attempted records here, never claim overflow integration.
        assert_eq!(
            actual_ids, expected_ids,
            "{}: ordered attempted events",
            row["name"]
        );
        checked += 1;
    }
    assert_eq!(checked, 8);
}

#[test]
fn player_scatter_sound_uses_the_full_retail_layered_reader() {
    let empty = crate::rules::ruleset::RuleSet::from_ini(
        &crate::rules::ini_parser::IniFile::from_str("[General]\n"),
    )
    .unwrap();
    assert_eq!(
        empty.general.scatter_sound, None,
        "original66585F constructor sentinel -1"
    );
    let Some(retail) = retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let controls = corpus();
    let last = controls["command_sound_layers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    assert_eq!(
        retail.rules.general.scatter_sound.as_deref(),
        last["guard"]["name"].as_str()
    );
}

#[test]
fn player_scatter_constructor_preserves_native_padding_and_house_null_semantics() {
    for row in corpus()["constructor_rows"].as_array().unwrap() {
        let decode = |key: &str| {
            let text = row[key].as_str().unwrap();
            let bytes: Vec<_> = text
                .as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect();
            CommandRecord::decode_exact(&bytes).unwrap()
        };
        let mut record = decode("before");
        ScatterRecord {
            house_id: row["args"][0].as_u64().unwrap() as i8,
            frame: row["frame"].as_u64().unwrap() as i32,
            entity_id: row["args"][2].as_u64().unwrap() as i32,
        }
        .write_into(&mut record);
        assert_eq!(record, decode("after"), "{}", row["name"]);
    }
}
