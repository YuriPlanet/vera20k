//! Original executed Session Country controls; see session_country.meta.json.
use super::*;
use serde_json::{Value, json};

fn fixture(root: &str) -> NativeRulesProcessOwner {
    NativeRulesProcessOwner::from_cold_start_sources(
        IniFile::from_str(root),
        None,
        IniFile::empty(),
        Arc::new(SoundRegistry::from_ini(&IniFile::empty())),
        Default::default(),
    )
    .unwrap()
}

fn snapshot(owner: &NativeRulesProcessOwner) -> Value {
    let state = owner.registry.as_ref().unwrap().registry_state();
    Value::Array(
        state
            .country_projection()
            .enumerate()
            .map(|(index, (id, body, _, _))| {
                json!({"index":index,"id":id,"name":body.read_name("Name",0x31).unwrap_or(id),
                            "multiplay":u8::from(body.read_bool("Multiplay",false))
                })
            })
            .collect(),
    )
}

#[test]
fn session_country_read_and_deferred_bodies_match_original_execution() {
    let corpus: Value =
        crate::test_fixture::json("tools/rules_oracle/session_country.json").clone();
    for row in corpus["rows"].as_array().unwrap() {
        let mut owner = fixture(corpus["root"].as_str().unwrap());
        let settings = IniFile::from_str(&format!(
            "[MultiPlayer]\n{}",
            row["token"]
                .as_str()
                .map(|value| format!("Side={value}\n"))
                .unwrap_or_default()
        ));
        for phase in row["phases"].as_array().unwrap() {
            assert_eq!(
                owner.read_multiplayer_country(
                    &settings,
                    corpus["current"].as_i64().unwrap() as i32
                ),
                phase["result"].as_i64().unwrap() as i32,
                "token {:?}",
                row["token"]
            );
            assert_eq!(
                snapshot(&owner),
                phase["allocated"],
                "token {:?}",
                row["token"]
            );
            owner.refresh_multiplayer_country_bodies();
            assert_eq!(
                snapshot(&owner),
                phase["refreshed"],
                "token {:?}",
                row["token"]
            );
            let NativeRulesRegistryOwner::BeforeReset(trace) = owner.registry.as_ref().unwrap()
            else {
                panic!("shell phase lost trace")
            };
            assert_eq!(
                trace.events().len(),
                phase["refreshed"].as_array().unwrap().len()
            );
        }
    }
}

#[test]
fn session_country_allocation_projects_and_survives_until_type_reset() {
    let root = "[Countries]\n0=Alpha\n[Alpha]\nName=Alias\n[Orphan]\nName=Other\nMultiplay=yes\n";
    let mut owner = fixture(root);
    let settings = IniFile::from_str("[MultiPlayer]\nSide=Orphan\n");
    assert_eq!(owner.read_multiplayer_country(&settings, -1), 1);
    owner.refresh_multiplayer_country_bodies();
    let mut projection = RuleSet::from_ini(&IniFile::from_str(root)).unwrap();
    owner.project_multiplayer_countries(&mut projection);
    assert_eq!(
        projection.country_name(crate::rules::ruleset::CountryIdx(1)),
        Some("Orphan")
    );
    assert!(projection.country_multiplay("Orphan"));
    assert_eq!(owner.country_name(1), Some("Orphan"));
    // Ordinary pre-reset body refresh preserves allocated orphan identities.
    owner.refresh_multiplayer_country_bodies();
    assert_eq!(owner.country_name(1), Some("Orphan"));
    let loaded = owner
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(None),
            &IniFile::empty(),
        )
        .unwrap();
    let (rules, _, _, receipt) = loaded.into_parts();
    assert!(rules.country_index("Orphan").is_none());
    assert_eq!(owner.country_name(0), Some("Alpha"));
    assert_eq!(owner.country_name(1), None);
    // Shell allocations predate ClearScene's numeric reset and never replay.
    assert_eq!(receipt.pre_reset().event_count(), 0);
}

#[test]
fn retail_session_master_uses_complete_original_country_array() {
    let Some(root) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let corpus: Value =
        crate::test_fixture::json("tools/rules_oracle/session_country.json").clone();
    let mut owner = fixture("");
    // Select the physical retail source with its native parser through the
    // existing test fixture loader; no compatibility projection allocates it.
    owner.sources.selected_rules_root = root;
    owner.read_multiplayer_country(&IniFile::empty(), -1);
    let ids: Vec<_> = owner.multiplayer_countries().map(|(id, _)| id).collect();
    let expected: Vec<_> = corpus["retail_registered"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, expected);
}
