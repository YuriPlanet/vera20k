//! Action42 consumes the existing process-resident Weapon family by ordinal.
//! Numeric/reference goldens are from original771C70/772080 execution; the
//! retained-state and safe-admission checks exercise the production Rules owner.
use super::*;
use crate::rules::ruleset::RuleSet;
use crate::util::save_codec;
use serde_json::{Value, json};

fn native() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/trigger_do_explosion.json")
}

fn field_state(rules: &RuleSet, name: &str) -> Value {
    let weapon = rules.weapon(name).unwrap();
    json!({"damage": weapon.damage, "warhead": weapon.warhead})
}

fn control_pass(keys: &Value) -> IniFile {
    let mut pass = IniFile::empty();
    if let Some(keys) = keys.as_object() {
        // These native controls supply cached entries directly. Preserve
        // present-empty values with set(); the physical loader omits them.
        let mut section = IniSection::new("Action42FieldControl".into());
        for (key, value) in keys {
            section.set(key, value.as_str().unwrap());
        }
        pass.replace_first_section(section);
    }
    pass
}

fn discovery() -> IniFile {
    IniFile::from_str(
        "[VehicleTypes]\n0=ProbeCarrier\n[ProbeCarrier]\nPrimary=Action42FieldControl\n\
         [Warheads]\n0=AP\n",
    )
}

#[test]
fn weapon_registry_retains_original_damage_and_warhead_controls_through_save_and_reset() {
    let native = &native()["weapon_field_controls"];
    let mut processed = RulesLayerStack::new(discovery()).process().unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    assert_eq!(
        field_state(&rules, "Action42FieldControl"),
        native["constructor"]
    );
    for row in native["rows"].as_array().unwrap() {
        assert_eq!(
            field_state(&rules, "Action42FieldControl"),
            row["before"],
            "{} before",
            row["name"]
        );
        let (_, trace) = processed.into_ini_and_native_type_construction_trace();
        processed = RulesLayerStack::new(control_pass(&row["keys"]))
            .process_with_fixed_art_and_registry_state(
                &IniFile::empty(),
                trace.into_registry_state_discarding_events(),
            )
            .unwrap();
        rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(
            field_state(&rules, "Action42FieldControl"),
            row["after"],
            "{} after",
            row["name"]
        );
        assert!(std::ptr::eq(
            rules.weapon_by_array_index(0).unwrap(),
            rules.weapon("Action42FieldControl").unwrap()
        ));
        if row["after"]["warhead"].is_string() {
            assert!(
                rules
                    .warhead(row["after"]["warhead"].as_str().unwrap())
                    .is_some(),
                "a retained pointer must remain materialized after an empty key"
            );
        }
    }

    let saved = save_codec::serialize(&processed).unwrap();
    let restored: ProcessedRulesLayers = save_codec::deserialize(&saved).unwrap();
    let restored_rules = RuleSet::from_processed_rules(&restored).unwrap();
    assert_eq!(
        field_state(&restored_rules, "Action42FieldControl"),
        field_state(&rules, "Action42FieldControl")
    );
    assert_eq!(
        rules.simulation_config_hash(),
        restored_rules.simulation_config_hash()
    );
    let (_, trace) = restored.into_ini_and_native_type_construction_trace();
    let reset = RulesLayerStack::new(discovery())
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            trace
                .into_registry_state_discarding_events()
                .destructive_reset(),
        )
        .unwrap();
    assert_eq!(
        field_state(
            &RuleSet::from_processed_rules(&reset).unwrap(),
            "Action42FieldControl"
        ),
        native["constructor"]
    );
}

#[test]
fn retail_campaign_weapon_ordinals_and_fields_match_original_process_arrays() {
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    use crate::rules::retail_ini_fixture::{retail_assets, retail_rules_owner};
    use crate::rules::retail_sources::select_ini;

    let Some((_, assets)) = retail_assets() else {
        return;
    };
    for row in native()["maps"].as_array().unwrap() {
        let map_name = row["map"].as_str().unwrap();
        let map = select_ini(&assets, map_name).unwrap();
        let named_ini = format!("{}.INI", map_name.strip_suffix(".MAP").unwrap());
        let named = assets
            .resolve_ref(&named_ini)
            .map(|_| select_ini(&assets, &named_ini).unwrap());
        let mut owner = retail_rules_owner(&assets);
        let (rules, _, _, _) = owner
            .load_scenario(
                NativeScenarioRulesPrefix::Campaign(named.as_ref().map(|source| &source.ini)),
                &map.ini,
            )
            .unwrap()
            .into_parts();
        let expected = row["weapon_registry"].as_array().unwrap();
        let actual: Vec<_> = (0..expected.len())
            .map(|index| {
                let weapon = rules
                    .weapon_by_array_index(i32::try_from(index).unwrap())
                    .unwrap_or_else(|| panic!("{map_name} weapon index {index}"));
                json!({"id": weapon.id, "damage": weapon.damage, "warhead": weapon.warhead})
            })
            .collect();
        assert_eq!(actual, *expected, "{map_name} native Weapon array");
        assert!(rules.weapon_by_array_index(-1).is_none());
        assert!(
            rules
                .weapon_by_array_index(i32::try_from(expected.len()).unwrap())
                .is_none()
        );
        assert!(rules.weapon_by_array_index(i32::MAX).is_none());
    }
}

#[test]
fn effective_weapon_order_damage_and_warhead_change_retained_rules_identity() {
    let controls = native()["weapon_field_controls"]["rows"]
        .as_array()
        .unwrap();
    let keys = &controls
        .iter()
        .find(|row| row["name"] == "set_both")
        .unwrap()["keys"];
    let signed = controls
        .iter()
        .find(|row| row["name"] == "signed_and_none")
        .unwrap();
    let mut pass = IniFile::from_str(
        "[VehicleTypes]\n0=CarrierA\n1=CarrierB\n[CarrierA]\nPrimary=First\n\
         [CarrierB]\nPrimary=Second\n[Warheads]\n0=AP\n",
    );
    for name in ["First", "Second"] {
        let mut section = IniSection::new(name.into());
        for (key, value) in keys.as_object().unwrap() {
            section.set(key, value.as_str().unwrap());
        }
        pass.replace_first_section(section);
    }
    let processed = RulesLayerStack::new(pass).process().unwrap();
    let original = RuleSet::from_processed_rules(&processed).unwrap();
    let saved = save_codec::serialize(&processed).unwrap();
    for change in ["order", "damage", "warhead"] {
        // Declared Rust saved-state priors isolate typed ownership from the
        // identical stored INI/hash. They are not native history goldens.
        // Normal Process can also change the compatibility text; comparing
        // those source hashes would not test the effective-state hash here.
        let mut candidate: ProcessedRulesLayers = save_codec::deserialize(&saved).unwrap();
        let weapons = candidate
            .native_type_construction_trace
            .registry_state
            .families
            .get_mut(&RulesTypeFamily::Weapon)
            .unwrap();
        match change {
            "order" => weapons.swap(0, 1),
            "damage" => {
                weapons[0].weapon.damage = signed["after"]["damage"]
                    .as_i64()
                    .unwrap()
                    .try_into()
                    .unwrap();
            }
            "warhead" => {
                assert!(signed["after"]["warhead"].is_null());
                weapons[0].weapon.warhead = None;
            }
            _ => unreachable!(),
        }
        let changed = RuleSet::from_processed_rules(&candidate).unwrap();
        assert_eq!(original.source_ini_hash(), changed.source_ini_hash());
        assert_ne!(
            original.simulation_config_hash(),
            changed.simulation_config_hash(),
            "{change}"
        );
    }
}

#[test]
fn saved_weapon_warhead_index_is_validated_before_projection() {
    let mut pass = discovery();
    let keys = &native()["weapon_field_controls"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "set_both")
        .unwrap()["keys"];
    pass.merge(&control_pass(keys));
    let mut processed = RulesLayerStack::new(pass).process().unwrap();
    processed
        .native_type_construction_trace
        .registry_state
        .families
        .get_mut(&RulesTypeFamily::Weapon)
        .unwrap()[0]
        .weapon
        .warhead = Some(usize::MAX);
    let saved = save_codec::serialize(&processed).unwrap();
    let restored = save_codec::deserialize(&saved).unwrap();
    assert!(matches!(
        RuleSet::from_processed_rules(&restored),
        Err(RulesError::InvalidSavedState(_))
    ));
}

#[test]
fn ambiguous_truncated_weapon_or_warhead_refuses_only_the_ordinal_slot() {
    // Explicit Rust safety boundary for issue1784, not native aliasing parity.
    let prefix = "ABCDEFGHIJKLMNOPQRSTUVWX";
    for (registry, first, second) in [
        ("", format!("{prefix}A"), format!("{prefix}B")),
        (
            &format!("[Warheads]\n0={prefix}A\n1={prefix}B\n"),
            "First".to_owned(),
            "Second".to_owned(),
        ),
    ] {
        let pass = IniFile::from_str(&format!(
            "{registry}[VehicleTypes]\n0=CarrierA\n1=CarrierB\n2=CarrierC\n\
             [CarrierA]\nPrimary={first}\n[CarrierB]\nPrimary={second}\n\
             [CarrierC]\nPrimary=Unrelated\n\
             [First]\nWarhead={prefix}\n[Second]\nWarhead={prefix}\n",
        ));
        let rules = RuleSet::from_ini(&pass).unwrap();
        assert!(rules.weapon_by_array_index(0).is_none());
        assert!(rules.weapon_by_array_index(1).is_none());
        assert_eq!(rules.weapon_by_array_index(2).unwrap().id, "Unrelated");
    }
}
