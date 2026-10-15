use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;

fn retained(radial_segments: i32) -> RuleSet {
    let prior = RulesLayerStack::new(IniFile::from_str(&format!(
        "[VehicleTypes]\n0=TEST\n[TEST]\nStrength=200\nRadialFireSegments={radial_segments}\n"
    )))
    .process()
    .unwrap();
    let (_, trace) = prior.into_ini_and_native_type_construction_trace();
    let (_, _, state) = trace.into_parts();
    let current = RulesLayerStack::new(IniFile::from_str(
        "[VehicleTypes]\n0=TEST\n[TEST]\nStrength=200\n",
    ))
    .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
    .unwrap();
    RuleSet::from_processed_rules(&current).unwrap()
}

#[test]
fn retained_radial_fire_segments_affect_simulation_config_hash() {
    let first = retained(10);
    let second = retained(3);
    assert_eq!(first.source_ini_hash(), second.source_ini_hash());
    assert_ne!(
        first.simulation_config_hash(),
        second.simulation_config_hash()
    );
    assert_eq!(first.object("TEST").unwrap().radial_fire_segments, 10);
    assert_eq!(second.object("TEST").unwrap().radial_fire_segments, 3);
}

#[test]
fn radial_fire_segments_default_and_signed_reader_apply_to_each_techno_family() {
    for category in [
        ObjectCategory::Building,
        ObjectCategory::Aircraft,
        ObjectCategory::Vehicle,
        ObjectCategory::Infantry,
    ] {
        for (body, expected) in [
            ("[TEST]\nStrength=200\n", 0),
            ("[TEST]\nRadialFireSegments=-7\n", -7),
            ("[TEST]\nRadialFireSegments=17suffix\n", 17),
            ("[TEST]\nradialfiresegments=10\n", 0),
        ] {
            let ini = IniFile::from_str(body);
            let object =
                ObjectType::from_ini_section("TEST", ini.section("TEST").unwrap(), category);
            assert_eq!(
                object.radial_fire_segments, expected,
                "{category:?}: {body}"
            );
        }
    }
}

#[test]
fn reached_passes_keep_current_default_and_roundtrip_retained_signed_value() {
    for (layer, expected) in [
        ("[OTHER]\nStrength=50\n", 10),
        ("[TEST]\nStrength=50\n", 10),
        ("[TEST]\nRadialFireSegments=\n", 10),
        ("[TEST]\nradialfiresegments=20\n", 10),
        ("[TEST]\nRadialFireSegments=garbage\n", 0),
        ("[TEST]\nRadialFireSegments=$invalid\n", 10),
        ("[TEST]\nRadialFireSegments=-7\n", -7),
        ("[TEST]\nRadialFireSegments=0\n", 0),
        ("[TEST]\nRadialFireSegments=17suffix\n", 17),
    ] {
        let mut stack = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=TEST\n[TEST]\nStrength=200\nRadialFireSegments=10\n",
        ));
        stack.push(RulesLayerKind::Scenario, IniFile::from_str(layer));
        let processed = stack.process().unwrap();
        let restored = serde_json::from_slice(&serde_json::to_vec(&processed).unwrap()).unwrap();
        let before = RuleSet::from_processed_rules(&processed).unwrap();
        let after = RuleSet::from_processed_rules(&restored).unwrap();
        assert_eq!(
            before.object("TEST").unwrap().radial_fire_segments,
            expected,
            "{layer}"
        );
        assert_eq!(
            after.object("TEST").unwrap().radial_fire_segments,
            expected,
            "{layer}"
        );
        assert_eq!(
            before.simulation_config_hash(),
            after.simulation_config_hash()
        );
    }
}

#[test]
fn retail_aegis_retains_radial_fire_segments_through_battle_rules() {
    let Some((root, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let processed = RulesLayerStack::new(root)
        .process_with_fixed_art(&art)
        .unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    assert_eq!(rules.object("AEGIS").unwrap().radial_fire_segments, 10);
    assert_eq!(rules.object("MTNK").unwrap().radial_fire_segments, 0);
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    assert_eq!(
        battle.rules.object("AEGIS").unwrap().radial_fire_segments,
        10
    );
}
