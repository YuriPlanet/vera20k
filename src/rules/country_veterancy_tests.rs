//! Country list reads through the production registry; these routing checks
//! supplement original reader/starting-rank evidence in country_veterancy.md.

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::native_processing::{ProcessedRulesLayers, RulesLayerKind, RulesLayerStack};
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::util::save_codec;

fn layers() -> RulesLayerStack {
    RulesLayerStack::new(IniFile::from_str(
        "[Countries]\n0=Alpha\n[Alpha]\nVeteranInfantry=I\nVeteranUnits=U\nVeteranAircraft=A\n\
         [InfantryTypes]\n0=I\n1=OTHER_I\n[VehicleTypes]\n0=U\n1=OTHER_U\n[AircraftTypes]\n0=A\n1=OTHER_A\n\
         [I]\nStrength=100\n[U]\nStrength=200\n[A]\nStrength=100\n\
         [OTHER_I]\nStrength=100\n[OTHER_U]\nStrength=200\n[OTHER_A]\nStrength=100\n",
    ))
}

fn memberships(processed: &ProcessedRulesLayers) -> [bool; 6] {
    let rules = RuleSet::from_processed_rules(processed).unwrap();
    ["I", "U", "A", "OTHER_I", "OTHER_U", "OTHER_A"]
        .map(|id| rules.country_starts_veteran("alpha", rules.object(id).unwrap()))
}

fn continue_process(processed: ProcessedRulesLayers, pass: IniFile) -> ProcessedRulesLayers {
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    RulesLayerStack::new(pass)
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            trace.into_registry_state_discarding_events(),
        )
        .unwrap()
}

#[test]
fn country_veteran_list_histories_match_original_reached_reader() {
    let corpus = crate::test_fixture::json("tools/rules_oracle/country_veterancy.json");
    let mut processed = RulesLayerStack::new(IniFile::from_str("[Countries]\n0=SYNTHCOUNTRY\n"))
        .process()
        .unwrap();
    for row in corpus["reader_controls"].as_array().unwrap() {
        // The native fixture supplies a CCINI cache. Preserve cached empty
        // values and exact section/key case instead of physical-file elision.
        let mut pass = IniFile::empty();
        for (name, values) in row["sections"].as_object().unwrap() {
            let mut section = IniSection::new(name.clone());
            for (key, value) in values.as_object().unwrap() {
                section.set(key, value.as_str().unwrap());
            }
            pass.replace_first_section(section);
        }
        processed = continue_process(processed, pass);
        let saved = serde_json::to_value(&processed).unwrap();
        let retained = &saved["native_type_construction_trace"]["registry_state"]["families"]["Country"]
            [0]["country_veterancy"];
        for (field, native_key) in [
            ("infantry", "VeteranInfantry"),
            ("units", "VeteranUnits"),
            ("aircraft", "VeteranAircraft"),
        ] {
            assert_eq!(
                retained[field], row["lists"][native_key]["names"],
                "{}: {native_key}",
                row["id"]
            );
        }
    }
}

#[test]
fn country_veteran_lists_save_handoff_hash_and_destructive_reset_keep_one_authority() {
    let processed = layers().process().unwrap();
    let bytes = save_codec::serialize(&processed).unwrap();
    let restored: ProcessedRulesLayers = save_codec::deserialize(&bytes).unwrap();
    assert_eq!(memberships(&restored), memberships(&processed));
    assert_eq!(
        RuleSet::from_processed_rules(&restored)
            .unwrap()
            .simulation_config_hash(),
        RuleSet::from_processed_rules(&processed)
            .unwrap()
            .simulation_config_hash()
    );
    let continued = continue_process(restored, IniFile::empty());
    assert_eq!(
        memberships(&continued),
        [true, true, true, false, false, false]
    );

    let mut other = layers();
    other.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[Alpha]\nVeteranUnits=OTHER_U\n"),
    );
    let other = continue_process(other.process().unwrap(), IniFile::empty());
    let left = RuleSet::from_processed_rules(&continued).unwrap();
    let right = RuleSet::from_processed_rules(&other).unwrap();
    assert_eq!(left.source_ini_hash(), right.source_ini_hash());
    assert_ne!(
        left.simulation_config_hash(),
        right.simulation_config_hash()
    );

    let (_, trace) = continued.into_ini_and_native_type_construction_trace();
    let reset = RulesLayerStack::new(IniFile::from_str(
        "[Countries]\n0=Alpha\n[Alpha]\nName=reset\n[VehicleTypes]\n0=U\n[U]\nStrength=200\n",
    ))
    .process_with_fixed_art_and_registry_state(
        &IniFile::empty(),
        trace
            .into_registry_state_discarding_events()
            .destructive_reset(),
    )
    .unwrap();
    let rules = RuleSet::from_processed_rules(&reset).unwrap();
    assert!(!rules.country_starts_veteran("Alpha", rules.object("U").unwrap()));

    let mut bad = serde_json::to_value(layers().process().unwrap()).unwrap();
    bad["native_type_construction_trace"]["registry_state"]["families"]["Country"][0]["country_veterancy"]
        ["units"] = serde_json::json!(["UNREGISTERED"]);
    let bad: ProcessedRulesLayers = serde_json::from_value(bad).unwrap();
    assert!(RuleSet::from_processed_rules(&bad).is_err());
}

#[test]
fn country_veteran_references_allocate_types_in_original_reader_order() {
    let processed = RulesLayerStack::new(IniFile::from_str(
        "[Countries]\n0=Alpha\n[Alpha]\nVeteranInfantry=LAZY_I, SECOND\nVeteranUnits=LAZY_U\nVeteranAircraft=LAZY_A\n",
    )).process().unwrap();
    let reached: Vec<_> = processed
        .techno_type_states()
        .map(|(category, id, _)| (category, id.to_owned()))
        .collect();
    assert_eq!(
        reached,
        vec![
            (ObjectCategory::Infantry, "LAZY_I".into()),
            (ObjectCategory::Infantry, " SECOND".into()),
            (ObjectCategory::Vehicle, "LAZY_U".into()),
            (ObjectCategory::Aircraft, "LAZY_A".into()),
        ]
    );
    // Case-insensitive identity lookup and category separation reach the same
    // retained list, rather than interpreting ParentCountry as inheritance.
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    for id in ["LAZY_I", " SECOND", "LAZY_U", "LAZY_A"] {
        assert!(rules.country_starts_veteran("ALPHA", rules.object(id).unwrap()));
    }
}
