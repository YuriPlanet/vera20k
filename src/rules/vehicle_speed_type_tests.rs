//! UnitType7470D0/747620 defaults and retained SpeedType across Rules passes.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{ProcessedRulesLayers, RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;

#[test]
fn vehicle_speed_type_uses_crusher_default_and_explicit_override() {
    for (body, expected) in [
        ("Strength=100", SpeedType::Wheel),
        ("Crusher=no", SpeedType::Wheel),
        ("Crusher=yes", SpeedType::Track),
        ("Crusher=invalid", SpeedType::Wheel),
        ("crusher=yes", SpeedType::Wheel),
        ("OmniCrusher=yes\nMovementZone=Crusher", SpeedType::Wheel),
        ("Crusher=yes\nSpeedType=Wheel", SpeedType::Wheel),
        ("Crusher=no\nSpeedType=Track", SpeedType::Track),
        ("SpeedType=Float", SpeedType::Float),
        ("SpeedType=invalid", SpeedType::Invalid),
    ] {
        let ini = IniFile::from_str(&format!("[CAR]\n{body}\n"));
        let object = ObjectType::from_ini_section(
            "CAR",
            ini.section("CAR").unwrap(),
            ObjectCategory::Vehicle,
        );
        assert_eq!(object.speed_type, expected, "{body}");
    }
}

#[test]
fn vehicle_speed_type_keeps_resolved_default_across_crusher_changes() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=CAR\n1=TANK\n[CAR]\nCrusher=no\n[TANK]\nCrusher=yes\n",
    ));
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[CAR]\nCrusher=yes\n[TANK]\nCrusher=no\n"),
    );
    let processed = layers.process().unwrap();
    let saved = bincode::serialize(&processed).unwrap();
    let restored: ProcessedRulesLayers = bincode::deserialize(&saved).unwrap();
    let rules = RuleSet::from_processed_rules(&restored).unwrap();
    assert_eq!(rules.object("CAR").unwrap().speed_type, SpeedType::Wheel);
    assert!(rules.object("CAR").unwrap().crusher);
    assert_eq!(rules.object("TANK").unwrap().speed_type, SpeedType::Track);
    assert!(!rules.object("TANK").unwrap().crusher);
}

#[test]
fn vehicle_speed_type_invalid_value_resolves_only_on_an_admitted_pass() {
    let mut layers = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=CAR\n[CAR]\nCrusher=yes\nSpeedType=invalid\n",
    ));
    for (patch, expected) in [
        ("[Other]\nStrength=1\n", SpeedType::Invalid),
        ("[CAR]\nCrusher=no\n", SpeedType::Wheel),
        ("[CAR]\nCrusher=yes\n", SpeedType::Wheel),
        ("[CAR]\nSpeedType=Float\n", SpeedType::Float),
        ("[CAR]\nSpeedType=invalid\n", SpeedType::Invalid),
        ("[CAR]\nStrength=100\n", SpeedType::Track),
    ] {
        layers.push(RulesLayerKind::Scenario, IniFile::from_str(patch));
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(rules.object("CAR").unwrap().speed_type, expected, "{patch}");
    }
}

#[test]
fn vehicle_speed_type_constructor_waits_for_the_first_admitted_section() {
    let mut layers = RulesLayerStack::new(IniFile::from_str("[VehicleTypes]\n0=CAR\n"));
    assert_eq!(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .object("CAR")
            .unwrap()
            .speed_type,
        SpeedType::Invalid,
    );
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[CAR]\nCrusher=yes\n"),
    );
    assert_eq!(
        RuleSet::from_rules_layers(&layers)
            .unwrap()
            .object("CAR")
            .unwrap()
            .speed_type,
        SpeedType::Track,
    );
}

#[test]
fn retail_vehicles_select_wheel_or_track_land_rows() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    for id in ["FV", "DTRUCK", "DRON", "YTNK", "MIND", "FTRK"] {
        assert_eq!(
            rules.object(id).unwrap().speed_type,
            SpeedType::Wheel,
            "{id}"
        );
    }
    assert_eq!(rules.object("MTNK").unwrap().speed_type, SpeedType::Track);
    let wheel_defaults: Vec<_> = rules
        .all_objects()
        .filter(|object| object.category == ObjectCategory::Vehicle && !object.crusher)
        .filter(|object| {
            let section = retail.processed_rules.section_or_empty(&object.id);
            // Registry-only entries (retail YDUM) never run ReadINI and keep -1.
            section.keys().next().is_some() && !section.is_present("SpeedType")
        })
        .map(|object| {
            assert_eq!(object.speed_type, SpeedType::Wheel, "{}", object.id);
            object.id.as_str()
        })
        .collect();
    eprintln!(
        "retail Wheel defaults ({}): {wheel_defaults:?}",
        wheel_defaults.len()
    );
    let ore = rules
        .terrain_rules
        .semantics_for_land_type(LandType::Tiberium.as_index())
        .unwrap();
    let wheel = rules.object("FV").unwrap().speed_type;
    let track = rules.object("MTNK").unwrap().speed_type;
    assert_ne!(
        ore.cost_for_speed_type(wheel),
        ore.cost_for_speed_type(track)
    );
}
