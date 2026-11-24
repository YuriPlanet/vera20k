//! Original Country+98 through the retained production Rules owner.
use super::ini_parser::{IniFile, IniSection};
use super::native_processing::{ProcessedRulesLayers, RulesLayerStack};
use super::ruleset::RuleSet;
use crate::util::save_codec;

fn start() -> ProcessedRulesLayers {
    RulesLayerStack::new(IniFile::from_str(
        "[Countries]\n0=ZERO\n1=PARENT\n2=CHILD\n",
    ))
    .process()
    .unwrap()
}

fn advance(processed: ProcessedRulesLayers, body: &str) -> ProcessedRulesLayers {
    let (_, trace) = processed.into_ini_and_native_type_construction_trace();
    RulesLayerStack::new(IniFile::from_str(body))
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            trace.into_registry_state_discarding_events(),
        )
        .unwrap()
}

#[test]
fn retained_parent_reads_match_original_physical_ini_histories() {
    let gold = crate::test_fixture::json("tools/rules_oracle/session_country_parent.json");
    let mut processed = start();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    assert_eq!(
        rules.country_parent_name("CHILD"),
        gold["constructor"]["parent"].as_str()
    );
    for row in gold["reader_controls"].as_array().unwrap() {
        processed = advance(processed, row["body"].as_str().unwrap());
        let rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(
            rules.country_parent_name("CHILD"),
            row["parent"].as_str(),
            "{}",
            row["id"]
        );
    }
}

#[test]
fn parent_owner_lookup_uses_original_alias_and_signed_sentinels() {
    let gold = crate::test_fixture::json("tools/rules_oracle/session_country_parent.json");
    for row in gold["lookup_controls"].as_array().unwrap() {
        let processed = advance(start(), "[PARENT]\nName=ParentAlias\n");
        let token = row["token"].as_str().unwrap();
        // A cached empty value is distinct from a physical elided empty key.
        let mut pass = IniFile::empty();
        let mut section = IniSection::new("CHILD".into());
        section.set("Name", "CHILD");
        section.set("ParentCountry", token);
        pass.replace_first_section(section);
        let (_, trace) = processed.into_ini_and_native_type_construction_trace();
        let selected = RulesLayerStack::new(pass)
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        let rules = RuleSet::from_processed_rules(&selected).unwrap();
        let index = row["index"].as_i64().unwrap() as i32;
        assert_eq!(
            rules.country_owner_mask("CHILD"),
            1u32.wrapping_shl(index as u32),
            "{token}"
        );
        assert_eq!(rules.country_index("CHILD").unwrap().0, 2);
    }
}

#[test]
fn authored_house_masks_match_original_owner_reader() {
    use super::object_type::ObjectCategory;
    let gold = crate::test_fixture::json("tools/rules_oracle/session_country_parent.json");
    for row in gold["house_mask_controls"].as_array().unwrap() {
        let text = format!(
            "[Countries]\n0=BASE\n1=PARENT\n2=CHILD\n[PARENT]\nName=ParentAlias\n\
             [BuildingTypes]\n0=TARGET\n[TARGET]\nStrength=100\nOwner={}\n",
            row["text"].as_str().unwrap()
        );
        let rules = RuleSet::from_ini(&IniFile::from_str(&text)).unwrap();
        let object = rules
            .object_in_category(ObjectCategory::Building, "TARGET")
            .unwrap();
        assert_eq!(
            rules.house_type_mask(&object.owner),
            row["mask"].as_u64().unwrap() as u32,
            "{}",
            row["text"]
        );
    }
}

#[test]
fn parent_state_survives_save_handoff_and_is_cleared_by_destructive_reset() {
    let processed = advance(start(), "[CHILD]\nParentCountry=PARENT\n");
    let bytes = save_codec::serialize(&processed).unwrap();
    let saved: ProcessedRulesLayers = save_codec::deserialize(&bytes).unwrap();
    assert_eq!(
        RuleSet::from_processed_rules(&saved)
            .unwrap()
            .simulation_config_hash(),
        RuleSet::from_processed_rules(&processed)
            .unwrap()
            .simulation_config_hash()
    );
    let continued = advance(saved, "");
    let rules = RuleSet::from_processed_rules(&continued).unwrap();
    assert_eq!(rules.country_parent_name("CHILD"), Some("PARENT"));
    let self_parent = advance(advance(start(), "[CHILD]\nName=CHILD\n"), "");
    assert_ne!(
        rules.simulation_config_hash(),
        RuleSet::from_processed_rules(&self_parent)
            .unwrap()
            .simulation_config_hash()
    );
    let (_, trace) = continued.into_ini_and_native_type_construction_trace();
    let reset = RulesLayerStack::new(IniFile::from_str("[Countries]\n0=CHILD\n"))
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            trace
                .into_registry_state_discarding_events()
                .destructive_reset(),
        )
        .unwrap();
    assert_eq!(
        RuleSet::from_processed_rules(&reset)
            .unwrap()
            .country_parent_name("CHILD"),
        Some("")
    );
}

#[test]
fn retained_parent_lookup_aliases_and_order_enter_configuration_hash() {
    let make = |alias: &str, reverse: bool| {
        let registry = if reverse {
            "[Countries]\n0=ZERO\n1=CHILD\n2=PARENT\n"
        } else {
            "[Countries]\n0=ZERO\n1=PARENT\n2=CHILD\n"
        };
        let processed = RulesLayerStack::new(IniFile::from_str(&format!(
            "{registry}[PARENT]\nName={alias}\n[CHILD]\nParentCountry=ParentAlias\n"
        )))
        .process()
        .unwrap();
        RuleSet::from_processed_rules(&advance(processed, "")).unwrap()
    };
    let selected = make("ParentAlias", false);
    let missing = make("OtherAlias", false);
    let reordered = make("ParentAlias", true);
    assert_eq!(selected.source_ini_hash(), missing.source_ini_hash());
    assert_eq!(selected.source_ini_hash(), reordered.source_ini_hash());
    assert_ne!(
        selected.country_owner_mask("CHILD"),
        missing.country_owner_mask("CHILD")
    );
    assert_ne!(
        selected.country_owner_mask("CHILD"),
        reordered.country_owner_mask("CHILD")
    );
    assert_ne!(
        selected.simulation_config_hash(),
        missing.simulation_config_hash()
    );
    assert_ne!(
        selected.simulation_config_hash(),
        reordered.simulation_config_hash()
    );
}
