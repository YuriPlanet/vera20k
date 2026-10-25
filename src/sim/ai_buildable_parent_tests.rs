//! Original FirstBuildable5051E0 mixed identity gates, not a second selector.
use super::*;
use crate::rules::ini_parser::IniFile;

/// ReadOwners4750D0 and FirstBuildable505230 distinguish a present zero
/// mask from the constructor's -1. Later empty reads retain that mask.
#[test]
fn comma_only_required_houses_survives_process_save_and_changes_ai_choice_identity() {
    use crate::rules::native_processing::{
        NativeRulesRegistryState, ProcessedRulesLayers, RulesLayerStack,
    };

    let advance = |state: NativeRulesRegistryState, text: &str| {
        RulesLayerStack::new(IniFile::from_str(text))
            .process_with_fixed_art_and_registry_state(&IniFile::empty(), state)
            .unwrap()
    };
    let take_state = |processed: ProcessedRulesLayers| {
        processed
            .into_ini_and_native_type_construction_trace()
            .1
            .into_registry_state_discarding_events()
    };
    let selected = |rules: &RuleSet| {
        first_buildable_from_array(rules, &["TARGET".into()], "C1", 0, true).is_some()
    };
    let seed = |required: &str| {
        advance(
            NativeRulesRegistryState::default(),
            &format!(
                "[Countries]\n0=C0\n1=C1\n[C1]\nParentCountry=C1\n[BuildingTypes]\n0=TARGET\n\
             [TARGET]\nStrength=100\nOwner=C1\nAIBasePlanningSide=0\nRequiredHouses={required}\n"
            ),
        )
    };
    assert!(selected(
        &RuleSet::from_processed_rules(&seed("C1")).unwrap()
    ));
    let zero = advance(
        take_state(seed(",,,")),
        "[TARGET]\nStrength=100\nRequiredHouses=\n",
    );
    let bytes = serde_json::to_vec(&zero).unwrap();
    let restored: ProcessedRulesLayers = serde_json::from_slice(&bytes).unwrap();
    let zero_rules = RuleSet::from_processed_rules(&restored).unwrap();
    assert!(
        !selected(&zero_rules),
        "a blank later pass and save retain the zero mask"
    );
    assert!(!selected(&zero_rules.clone()));

    let allowed = advance(
        take_state(seed("C1")),
        "[TARGET]\nStrength=100\nRequiredHouses=\n",
    );
    let allowed_rules = RuleSet::from_processed_rules(&allowed).unwrap();
    assert!(selected(&allowed_rules));
    assert_eq!(
        zero_rules.source_ini_hash(),
        allowed_rules.source_ini_hash()
    );
    assert_ne!(
        zero_rules.simulation_config_hash(),
        allowed_rules.simulation_config_hash()
    );

    let absent = advance(take_state(restored), "[TARGET]\nStrength=100\n");
    assert!(!selected(&RuleSet::from_processed_rules(&absent).unwrap()));
    let replaced = advance(take_state(absent), "[TARGET]\nRequiredHouses=C1\n");
    assert!(selected(&RuleSet::from_processed_rules(&replaced).unwrap()));
    let reset = take_state(replaced).destructive_reset();
    let reset = advance(
        reset,
        "[Countries]\n0=C0\n1=C1\n[C1]\nParentCountry=C1\n[BuildingTypes]\n0=TARGET\n\
         [TARGET]\nStrength=100\nOwner=C1\nAIBasePlanningSide=0\n",
    );
    assert!(selected(&RuleSet::from_processed_rules(&reset).unwrap()));
}

fn mask_names(mask: i64) -> String {
    if mask == -1 {
        return String::new();
    }
    (0..32)
        .filter(|bit| mask & (1i64 << bit) != 0)
        .map(|bit| match bit {
            0 => "BASE".into(),
            1 => "PARENT".into(),
            2 => "CHILD".into(),
            _ => format!("N{bit}"),
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[test]
fn first_buildable_keeps_parent_owner_and_actual_restrictions_separate() {
    let gold = crate::test_fixture::json("tools/rules_oracle/session_country_parent.json");
    for row in gold["first_buildable_controls"].as_array().unwrap() {
        let mut text = String::from("[Countries]\n0=BASE\n1=PARENT\n2=CHILD\n");
        for index in 3..32 {
            text.push_str(&format!("{index}=N{index}\n"));
        }
        let owner = row["owner_text"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| mask_names(row["owner_mask"].as_i64().unwrap()));
        text.push_str(&format!("[PARENT]\nName=ParentAlias\n[CHILD]\nParentCountry={}\n[BuildingTypes]\n0=TARGET\n[TARGET]\nStrength=100\nAIBasePlanningSide=0\nOwner={}\n",
            row["parent"].as_str().unwrap(), owner));
        for (field, key) in [
            ("required_mask", "RequiredHouses"),
            ("forbidden_mask", "ForbiddenHouses"),
        ] {
            if row[field].as_i64().unwrap() != -1 {
                text.push_str(&format!(
                    "{key}={}\n",
                    mask_names(row[field].as_i64().unwrap())
                ));
            }
        }
        let rules = RuleSet::from_ini(&IniFile::from_str(&text)).unwrap();
        let selected =
            first_buildable_from_array(&rules, &["TARGET".into()], "CHILD", 0, true).is_some();
        assert_eq!(
            selected,
            row["selected"].as_bool().unwrap(),
            "{}",
            row["id"]
        );
        assert_eq!(
            house_country_bit(&rules, "CHILD"),
            4,
            "actual restrictions do not inherit"
        );
    }
}

#[test]
fn opening_mcv_uses_parent_owner_without_production_restrictions() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=ZERO\n1=PARENT\n2=CHILD\n[CHILD]\nParentCountry=PARENT\n\
         [General]\nBaseUnit=WRONG,RIGHT\n[VehicleTypes]\n0=WRONG\n1=RIGHT\n\
         [WRONG]\nStrength=100\nOwner=CHILD\n\
         [RIGHT]\nStrength=100\nOwner=PARENT\nRequiredHouses=ZERO\nForbiddenHouses=CHILD\n",
    ))
    .unwrap();
    assert_eq!(
        crate::sim::scenario_bootstrap::launch_mcv_type_for_country(
            crate::skirmish_launch::LaunchCountry::from_country_index(2),
            &rules
        ),
        Some("RIGHT")
    );
}
