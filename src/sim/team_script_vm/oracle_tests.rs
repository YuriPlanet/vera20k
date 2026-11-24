//! `tools/ai_team_oracle.py`'s AI trigger weight feedback (`0x0041FD60`,
//! `0x0041FE20`), the team destructor's AI trigger loop (`0x006E8E02..
//! 0x006E8E3E`) replayed.

use serde_json::Value;

use super::*;
use crate::sim::ai_team_creation::tests::{ai_trigger, dword, flag, index, rows};
use crate::sim::intern::StringInterner;

fn bits(value: &Value) -> NativeF64Bits {
    NativeF64Bits::from_bits(value.as_u64().unwrap())
}

/// Retail's `AITriggerSuccessWeightDelta=20`, `AITriggerFailureWeightDelta=-50`
/// and `AITriggerTrackRecordCoefficient=1` unless the row says otherwise.
fn feedback_rules(row: &Value) -> AiTriggerFeedbackRules {
    let or = |key: &str, value: f64| {
        row.get(key)
            .map_or(NativeF64Bits::from_bits(value.to_bits()), bits)
    };
    AiTriggerFeedbackRules {
        success_weight_delta: or("success_delta", 20.0),
        failure_weight_delta: or("failure_delta", -50.0),
        track_record_coefficient: or("coefficient", 1.0),
    }
}

fn state(record: &AiTriggerTrackRecord) -> (u64, i32, i32) {
    (record.weight.bits(), record.successes, record.attempts)
}

fn native_state(result: &Value) -> (u64, i32, i32) {
    (
        result["weight"].as_u64().unwrap(),
        dword(&result["successes"]),
        dword(&result["attempts"]),
    )
}

/// A VM with TeamTypes `TEAMTYPE0..=2`, and the new team of the first for
/// house `OWNER`, built at `frame`.
fn one_team(interner: &mut StringInterner, frame: i32) -> (TeamScriptVm, u64, Vec<InternedId>) {
    let mut vm = TeamScriptVm::default();
    let script = interner.intern("SCRIPT");
    let task_force = interner.intern("TASKFORCE");
    vm.register_script(TeamScriptDefinition {
        id: script,
        actions: Vec::new(),
        source: TeamAiDefinitionSource::FixedAimd,
    });
    vm.register_task_force(TeamTaskForceDefinition {
        id: task_force,
        group: -1,
        entries: Vec::new(),
        source: TeamAiDefinitionSource::FixedAimd,
    });
    let team_types: Vec<InternedId> = (0..3)
        .map(|slot| {
            let id = interner.intern(&format!("TEAMTYPE{slot}"));
            vm.register_team_type(TeamTypeDefinition {
                id,
                script_id: script,
                task_force_id: task_force,
                priority: 0,
                is_base_defense: false,
                suicide: false,
                aggressive: false,
                combined_movement_zone: MovementZone::Normal,
                base_zone_relation_enforced: false,
                transport_crossing_required: false,
            });
            id
        })
        .collect();
    let owner = interner.intern("OWNER");
    let team = vm
        .construct_team(
            team_types[0],
            owner,
            TeamLimitScope::PerHouse,
            frame,
            false,
            None,
        )
        .unwrap();
    (vm, team, team_types)
}

#[test]
fn the_destructor_feeds_back_the_triggers_the_original_does() {
    for row in rows("destructor") {
        let mut interner = StringInterner::new();
        let (mut vm, team, team_types) = one_team(&mut interner, 0);
        let mut triggers = Vec::new();
        for (slot, trigger) in row["triggers"].as_array().unwrap().iter().enumerate() {
            let id = interner.intern(&format!("TRIGGER{slot}"));
            let mut definition = ai_trigger(
                id,
                team_types[index(&trigger["team_type"])],
                None,
                bits(&trigger["weight"]),
            );
            definition.weights[1] = NativeF64Bits::from_bits(10.0f64.to_bits());
            definition.weights[2] = NativeF64Bits::from_bits(70.0f64.to_bits());
            vm.register_ai_trigger(definition);
            let record = vm.ai_trigger_records.get_mut(&id).unwrap();
            record.successes = dword(&trigger["successes"]);
            record.attempts = dword(&trigger["attempts"]);
            triggers.push(id);
        }
        vm.teams.get_mut(&team).unwrap().succeeded = flag(&row["succeeded"]);

        // The destructor's feedback half; its member release is `destroy_team`'s.
        vm.record_trigger_outcome(team, &feedback_rules(&row));

        let results = row["results"].as_array().unwrap();
        for (id, result) in triggers.iter().zip(results) {
            assert_eq!(
                state(&vm.ai_trigger_records[id]),
                native_state(result),
                "{row}"
            );
        }
    }
}

/// `AITriggerTrackRecordCoefficient=1e999` reads as infinity. A failure at an
/// even track record multiplies `s/n - 0.5 = 0` by it, a NaN that the failure
/// compare (`0x0041FE60`, `TEST AH,0x41`) keeps; the NaN weight is unordered
/// with the minimum, which `0x0041FE8A` (`TEST AH,0x1`) then stores.
#[test]
fn an_infinite_track_record_coefficient_drops_a_failed_weight_to_its_minimum() {
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[General]\nAITriggerTrackRecordCoefficient=1e999\n",
        ))
        .unwrap();
    let feedback = AiTriggerFeedbackRules::new(&rules.general);
    let [minimum, maximum] = [10.0f64, 70.0].map(|value| NativeF64Bits::from_bits(value.to_bits()));
    let mut record = AiTriggerTrackRecord {
        weight: NativeF64Bits::from_bits(50.0f64.to_bits()),
        successes: 1,
        attempts: 2,
    };
    record.record(false, [minimum, maximum], &feedback);
    assert_eq!(state(&record), (minimum.bits(), 1, 3));
}
