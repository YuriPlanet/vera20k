//! Original Rules665650 and General66EC1B..66EC62 controls through the
//! retained production reader and RuleSet projection. Native scope and parser
//! exclusions: tools/rules_oracle/guided_controls.md.

use super::*;
use crate::rules::ruleset::RuleSet;
use serde_json::Value;

fn authored_ini(sections: &Value) -> IniFile {
    // Enter through the production lexical loader: empty authored values are
    // omitted there, unlike the oracle's separate raw-empty-cache probe.
    let mut text = String::new();
    for (section, keys) in sections.as_object().unwrap() {
        text.push_str(&format!("[{section}]\n"));
        for (key, value) in keys.as_object().unwrap() {
            text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
    }
    IniFile::from_str(&text)
}

#[test]
fn sequential_general_controls_match_original_and_survive_registry_handoff() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/guided_controls.json",
    ))
    .unwrap();
    let sequence = &native["sequential"];
    let mut state = NativeRulesRegistryState::default();
    assert_eq!(
        state.rules_missile_rot_var.to_bits(),
        u64::from_str_radix(
            sequence["constructor"]["missile_rot_var_bits"]
                .as_str()
                .unwrap(),
            16,
        )
        .unwrap()
    );
    assert_eq!(
        state.rules_safety_altitude,
        sequence["constructor"]["safety_altitude"].as_i64().unwrap() as i32
    );
    let rows = sequence["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 30);
    let mut compared_float_rows = 0;
    for row in rows {
        let name = row["name"].as_str().unwrap();
        let processed = RulesLayerStack::new(authored_ini(&row["authored_sections"]))
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
            .unwrap();
        let (rot, altitude, _) = processed.projectile_rule_controls();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        let native_altitude = row["after"]["safety_altitude"].as_i64().unwrap() as i32;
        assert_eq!(altitude, native_altitude, "retained owner: {name}");
        assert_eq!(
            rules.general.safety_altitude, native_altitude,
            "RuleSet projection: {name}"
        );
        // Failed sscanf preserves stale native caller memory; this bounded
        // fixture does not establish the full General caller's invalid policy.
        // The saved minimum-subnormal result also differs from Rust parsing.
        // Execute those inputs, but do not claim their float values match.
        if !matches!(
            name,
            "invalid_float" | "missing_after_invalid_retains_bits" | "float_subnormal"
        ) {
            let native_bits =
                u64::from_str_radix(row["after"]["missile_rot_var_bits"].as_str().unwrap(), 16)
                    .unwrap();
            assert_eq!(rot.to_bits(), native_bits, "retained owner: {name}");
            assert_eq!(
                rules.general.missile_rot_var.to_bits(),
                native_bits,
                "RuleSet projection: {name}"
            );
            compared_float_rows += 1;
        }
        let (_, receipt) = processed.into_ini_and_native_type_construction_trace();
        state = receipt.into_registry_state_discarding_events();
    }
    assert_eq!(compared_float_rows, 27);
}
