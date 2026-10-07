//! Original input controls for the bridge path-finishing consumer.
//! These pin reader/getter values, not whole A* or AI selection equivalence.

use super::*;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::object_type::ObjectType;
use crate::rules::retail_ini_fixture;
use crate::rules::ruleset::RuleSet;
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::components::NavigationState;
use crate::sim::intern::StringInterner;
use serde_json::Value;

fn packet() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/astar_threat_inputs.json",
    ))
    .unwrap()
}

fn stored_bits(value: &Value) -> NativeF64Bits {
    let raw = value.as_str().unwrap();
    let mut bytes = [0; 8];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&raw[index * 2..index * 2 + 2], 16).unwrap();
    }
    NativeF64Bits::from_bits(u64::from_le_bytes(bytes))
}

#[test]
fn original_coefficient_reader_and_nullable_team_getter_controls() {
    let packet = packet();
    assert_eq!(
        packet["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    for row in packet["type_readers"].as_array().unwrap() {
        let mut section = IniSection::new("MTNK".into());
        if let Some(text) = row["value"].as_str() {
            section.set("ThreatAvoidanceCoefficient", text);
        }
        assert_eq!(
            section.read_double_bits("ThreatAvoidanceCoefficient", stored_bits(&row["before"])),
            stored_bits(&row["after"]),
            "{row}"
        );
        if !row["value"].is_null() {
            let object = ObjectType::from_ini_section("MTNK", &section, ObjectCategory::Vehicle);
            assert_eq!(
                object.threat_avoidance_coefficient,
                stored_bits(&row["after"])
            );
        }
    }
    let default = ObjectType::from_ini_section(
        "MTNK",
        &IniSection::new("MTNK".into()),
        ObjectCategory::Vehicle,
    );
    assert_eq!(
        default.threat_avoidance_coefficient,
        NativeF64Bits::POSITIVE_ZERO
    );
    let mut nav = NavigationState::at_frame(37);
    assert_eq!(nav.path_threat_coefficient(), NativeF64Bits::POSITIVE_ZERO);
    let rows = packet["getter"].as_array().unwrap();
    assert_eq!(rows.len(), 12);
    for row in rows {
        let retained = NativeF64Bits::from_bits(
            u64::from_str_radix(row["coefficient_bits"].as_str().unwrap(), 16).unwrap(),
        );
        nav.retain_threat_avoidance_after_unlimbo(retained);
        let expected = NativeF64Bits::from_bits(
            u64::from_str_radix(row["returned_bits"].as_str().unwrap(), 16).unwrap(),
        );
        assert_eq!(
            nav.path_threat_coefficient_for_team(row["team_flag"] == 1),
            expected,
            "{row}"
        );
        assert_eq!(
            nav.path_threat_coefficient(),
            retained,
            "the Team override must not overwrite Foot+530"
        );
    }
}

#[test]
fn original_team_boolean_controls_preserve_current_defaults() {
    let packet = packet();
    let mut current = packet["team_constructor_default"].as_u64().unwrap() != 0;
    assert!(!current);
    for row in packet["team_readers"].as_array().unwrap() {
        assert_eq!(u64::from(current), row["before"].as_u64().unwrap());
        let mut fields = IniSection::new("FINISH_TEAM".into());
        if let Some(value) = row["value"].as_str() {
            fields.set("AvoidThreats", value);
        }
        current = fields.read_bool("AvoidThreats", current);
        assert_eq!(u64::from(current), row["after"].as_u64().unwrap(), "{row}");
    }
}

#[test]
fn original_team_avoid_threats_defaults_survive_production_fixed_map_reads() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
    ))
    .unwrap();
    let native = packet();
    for row in native["team_readers"].as_array().unwrap() {
        let before = row["before"].as_u64().unwrap() != 0;
        let expected = row["after"].as_u64().unwrap() != 0;
        let fixed = IniFile::from_str(&format!(
            "[TeamTypes]\n0=FINISH_TEAM\n[FINISH_TEAM]\nScript=MOVE\nTaskForce=FORCE\n\
             AvoidThreats={}\n[ScriptTypes]\n0=MOVE\n[MOVE]\n0=0,0\n\
             [TaskForces]\n0=FORCE\n[FORCE]\n0=1,E1\n",
            if before { "yes" } else { "no" },
        ));
        let mut map_text = "[TeamTypes]\n0=FINISH_TEAM\n[FINISH_TEAM]\nScript=MOVE\n".to_string();
        if let Some(value) = row["value"].as_str() {
            map_text.push_str(&format!("AvoidThreats={value}\n"));
        }
        // A supplied native empty cache and an omitted physical empty key
        // both keep the current field; the production parser owns omission.
        let map = IniFile::from_str(&map_text);
        let registry = TeamAiIniRegistry::from_sources(&fixed, &map, true);
        let mut interner = StringInterner::new();
        let (vm, diagnostics) = TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);
        assert!(diagnostics.is_empty(), "{row}: {diagnostics:?}");
        let id = interner.intern("FINISH_TEAM");
        assert_eq!(vm.team_type_ini[&id].avoid_threats, expected, "{row}");
        assert_eq!(
            registry
                .team_type_read_sequence()
                .map(|read| read.section().read_bool("AvoidThreats", false))
                .collect::<Vec<_>>(),
            [before, expected],
            "original reached-read order: {row}",
        );
    }
}

#[test]
fn original_coefficient_defaults_survive_production_rules_layers() {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};

    let mut layers = RulesLayerStack::new(IniFile::from_str("[VehicleTypes]\n0=MTNK\n"));
    for row in packet()["type_readers"].as_array().unwrap() {
        let before = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(
            before.object("MTNK").unwrap().threat_avoidance_coefficient,
            stored_bits(&row["before"]),
            "before: {row}",
        );
        let mut input = "[MTNK]\nStrength=400\n".to_string();
        if let Some(value) = row["value"].as_str() {
            input.push_str(&format!("ThreatAvoidanceCoefficient={value}\n"));
        }
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(&input));
        let after = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(
            after.object("MTNK").unwrap().threat_avoidance_coefficient,
            stored_bits(&row["after"]),
            "after: {row}",
        );
    }
}

#[test]
fn retail_hills_battle_mtnk_and_all_stock_team_overrides_match_original_readers() {
    let Some(retail) = retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some(aimd_bytes) = retail_ini_fixture::retail_ini_bytes("aimd.ini") else {
        return;
    };
    let packet = packet();
    assert_eq!(
        crate::util::sha256::sha256_hex(&aimd_bytes),
        packet["retail_aimd"]["sha256"].as_str().unwrap()
    );
    let mtnk = retail
        .rules
        .object("MTNK")
        .expect("stock Hills/Battle MTNK");
    assert_eq!(
        mtnk.threat_avoidance_coefficient,
        NativeF64Bits::POSITIVE_ZERO
    );
    let aimd = IniFile::from_bytes(&aimd_bytes).unwrap();
    let registry = TeamAiIniRegistry::from_sources(&aimd, &IniFile::from_str(""), true);
    let mut interner = StringInterner::new();
    let (vm, diagnostics) =
        TeamScriptVm::from_ini_registry(&registry, &mut interner, &retail.rules);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let rows = packet["retail_aimd"]["teams"].as_array().unwrap();
    assert_eq!(rows.len(), 163);
    assert_eq!(vm.team_type_ini.len(), rows.len());
    let mut enabled = 0;
    for row in rows {
        let id = interner.intern(row["name"].as_str().unwrap());
        let expected = row["native"].as_u64().unwrap() != 0;
        assert_eq!(vm.team_type_ini[&id].avoid_threats, expected, "{row}");
        enabled += usize::from(expected);
    }
    assert_eq!(enabled, 51);
}
