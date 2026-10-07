//! Original TechnoType approach flags through the production Rules pass owner.
//!
//! The native corpus executes UnitType7470D0 and ReadINI7144A0..7144D4,
//! including ReadBool5295F0's retained defaults. Registry chronology is owned
//! by native_processing; the late-discovery cases exercise that existing owner.

use super::{RulesLayerKind, RulesLayerStack};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fv_cell_attack/approach_rules.json",
    ))
    .unwrap()
}

fn named_case<'a>(native: &'a Value, name: &str) -> &'a Value {
    native["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == name)
        .unwrap()
}

fn assert_flags(layers: &RulesLayerStack, expected: &Value, label: &str) {
    let rules = RuleSet::from_rules_layers(layers).expect("production rules passes");
    let object = rules.object("FV").expect("registered FV");
    assert_eq!(
        [
            u64::from(object.can_approach_target),
            u64::from(object.can_recalc_approach_target),
        ],
        [
            expected["CanApproachTarget"].as_u64().unwrap(),
            expected["CanRecalcApproachTarget"].as_u64().unwrap(),
        ],
        "{label}"
    );
}

fn supplied_pass(step: &Value) -> IniFile {
    let mut ini = IniFile::empty();
    let section = ini.projection_section_mut("FV");
    for (key, value) in step["input"]["selected"].as_object().unwrap() {
        // These native controls supply cached lexical values, so retain their
        // exact spelling without adding another physical-line normalization.
        section.set(key, value.as_str().unwrap());
    }
    ini
}

#[test]
fn approach_flags_match_original_constructor_and_all_layer_reads() {
    let native = corpus();
    let cases = native["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 11);
    for case in cases {
        // The native fixture constructs FV before the first scalar read. This
        // registry-only pass establishes the same reader default boundary.
        let mut layers = RulesLayerStack::new(IniFile::from_str("[VehicleTypes]\n0=FV\n"));
        assert_flags(&layers, &native["constructor"]["after"], "constructor");
        for (index, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            if step["absent"] == true {
                continue;
            }
            let label = format!("{} pass {index}", case["name"]);
            assert_flags(&layers, &step["before"], &format!("{label} before"));
            let kind = match index {
                0 => RulesLayerKind::RulesMd,
                1 => RulesLayerKind::LangRule,
                2 => RulesLayerKind::GameMode,
                _ => RulesLayerKind::Scenario,
            };
            layers.push(kind, supplied_pass(step));
            assert_flags(&layers, &step["after"], &label);
        }
        assert_flags(&layers, &case["after"], case["name"].as_str().unwrap());
    }
}

#[test]
fn approach_flags_ignore_orphan_values_before_late_registry_discovery() {
    let native = corpus();
    let explicit_false = named_case(&native, "exact_false");
    let invalid = named_case(&native, "invalid_defaults");
    let retained_missing = named_case(&native, "retained_after_missing");
    let mut layers = RulesLayerStack::new(supplied_pass(&explicit_false["steps"][0]));
    assert!(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .object("FV")
            .is_none()
    );

    layers.push(
        RulesLayerKind::GameMode,
        IniFile::from_str("[VehicleTypes]\n0=FV\n"),
    );
    assert_flags(&layers, &native["constructor"]["after"], "late constructor");
    layers.push(
        RulesLayerKind::Scenario,
        supplied_pass(&invalid["steps"][0]),
    );
    assert_flags(&layers, &invalid["after"], "invalid after late discovery");
    layers.push(
        RulesLayerKind::Scenario,
        supplied_pass(&explicit_false["steps"][0]),
    );
    assert_flags(&layers, &explicit_false["after"], "first valid late body");
    layers.push(
        RulesLayerKind::Scenario,
        supplied_pass(&retained_missing["steps"][1]),
    );
    assert_flags(&layers, &retained_missing["after"], "retained late body");
}

#[test]
fn approach_flags_wait_for_next_body_pass_after_post_family_allocation() {
    let native = corpus();
    let explicit_false = named_case(&native, "exact_false");
    let mut first = supplied_pass(&explicit_false["steps"][0]);
    // ReadCrateRules follows ReadTypeData. Its new Unit must not retroactively
    // read the body from the already-completed UnitType loop in this pass.
    first
        .projection_section_mut("CrateRules")
        .set("UnitCrateType", "FV");
    let mut layers = RulesLayerStack::new(first);
    assert_flags(&layers, &native["constructor"]["after"], "unread new Unit");
    layers.push(RulesLayerKind::GameMode, IniFile::empty());
    assert_flags(
        &layers,
        &native["constructor"]["after"],
        "absent later body",
    );
    layers.push(
        RulesLayerKind::Scenario,
        supplied_pass(&explicit_false["steps"][0]),
    );
    assert_flags(&layers, &explicit_false["after"], "first reached body");
}

#[test]
fn retail_fv_approach_flags_match_original_root_reader() {
    let Some(raw) = crate::rules::retail_ini_fixture::retail_ini_bytes("rulesmd.ini") else {
        return;
    };
    let native = corpus();
    let first = &named_case(&native, "physical_retail")["steps"][0];
    assert_eq!(
        crate::util::sha256::sha256_hex(&raw),
        first["input"]["sha256"]
    );
    let layers = RulesLayerStack::new(IniFile::from_bytes(&raw).unwrap());
    assert_flags(&layers, &first["after"], "physical RULESMD.INI");
}

#[test]
#[ignore = "requires the unmodified retail install and stock Anytown rules layers"]
fn retail_fv_approach_flags_match_original_root_mode_and_map_readers() {
    let retail = std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("RA2_DIR"));
    let assets = crate::assets::asset_manager::AssetManager::new(
        &retail,
        crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
    )
    .unwrap();
    let native = corpus();
    let case = named_case(&native, "physical_retail");
    let mut layers: Option<RulesLayerStack> = None;
    for (index, step) in case["steps"].as_array().unwrap().iter().enumerate() {
        if step["absent"] == true {
            assert!(assets.get(step["name"].as_str().unwrap()).is_none());
            continue;
        }
        let name = step["input"]["name"].as_str().unwrap();
        let raw = assets
            .get(name)
            .unwrap_or_else(|| panic!("retail asset {name}"));
        assert_eq!(
            crate::util::sha256::sha256_hex(&raw),
            step["input"]["sha256"],
            "native input identity: {name}"
        );
        let pass = IniFile::from_bytes(&raw).unwrap();
        if let Some(layers) = layers.as_mut() {
            let kind = match index {
                1 => RulesLayerKind::LangRule,
                2 => RulesLayerKind::GameMode,
                _ => RulesLayerKind::Scenario,
            };
            layers.push(kind, pass);
        } else {
            layers = Some(RulesLayerStack::new(pass));
        }
        assert_flags(layers.as_ref().unwrap(), &step["after"], name);
    }
    assert_flags(
        layers.as_ref().unwrap(),
        &case["after"],
        "retail final state",
    );
}
