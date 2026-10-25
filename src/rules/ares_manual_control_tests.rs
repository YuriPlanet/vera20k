//! Saved Ares instructions supply bool/default/layer outcomes. Production
//! Rules processing owns type allocation, retention and the typed projection.

use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;
use crate::util::save_codec;

#[test]
fn ares_no_manual_fire_native_physical_layers_reach_every_techno_family() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/no_manual_fire.json");
    for family in [
        "BuildingTypes",
        "VehicleTypes",
        "AircraftTypes",
        "InfantryTypes",
    ] {
        let mut layers =
            RulesLayerStack::new(IniFile::from_str(&format!("[{family}]\n0=CONTROL\n")));
        let initial = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
        assert_eq!(
            initial.object("CONTROL").unwrap().no_manual_fire(),
            corpus["physical_readers"]["constructor_default"] == 1,
        );
        for row in corpus["physical_readers"]["rows"].as_array().unwrap() {
            layers.push(
                RulesLayerKind::Scenario,
                IniFile::from_str(row["physical_ini"].as_str().unwrap()),
            );
            let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
            assert_eq!(
                rules.object("CONTROL").unwrap().no_manual_fire(),
                crate::rules::ARES_ENABLED && row["after"] == 1,
                "{family}: {}",
                row["file"],
            );
        }
    }
}

#[test]
fn ares_no_manual_fire_registry_save_retains_policy_and_configuration_identity() {
    let continue_policy = |setting| {
        let initial = RulesLayerStack::new(IniFile::from_str(&format!(
            "[VehicleTypes]\n0=CONTROL\n[CONTROL]\nNoManualFire={setting}\n"
        )))
        .process()
        .unwrap();
        let (_, trace) = initial.into_ini_and_native_type_construction_trace();
        let registry = trace.into_registry_state_discarding_events();
        let registry = save_codec::deserialize(&save_codec::serialize(&registry).unwrap()).unwrap();
        let resumed = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=CONTROL\n[CONTROL]\nNoManualFire=invalid\n",
        ))
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), registry)
        .unwrap();
        RuleSet::from_processed_rules(&resumed).unwrap()
    };
    let enabled = continue_policy("yes");
    let disabled = continue_policy("no");
    assert_eq!(
        enabled.object("CONTROL").unwrap().no_manual_fire(),
        crate::rules::ARES_ENABLED,
    );
    assert!(!disabled.object("CONTROL").unwrap().no_manual_fire());
    assert_eq!(
        enabled.simulation_config_hash() != disabled.simulation_config_hash(),
        crate::rules::ARES_ENABLED,
        "identical current source stacks must retain effective policy identity",
    );
}

#[test]
fn ares_no_manual_fire_ignores_a_body_from_before_type_allocation() {
    let mut layers = RulesLayerStack::new(IniFile::from_str("[CONTROL]\nNoManualFire=yes\n"));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[VehicleTypes]\n0=CONTROL\n[CONTROL]\nStrength=100\n"),
    );
    let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
    assert!(!rules.object("CONTROL").unwrap().no_manual_fire());
}
