//! Parser/load comparisons consume saved Ares 3.0 instruction executions.
//! Layer/lifetime/buffer checks are Rust regressions; they do not replay a
//! complete Ares scenario or the physical INI loader.

use super::GenericPrerequisites;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::native_processing::{ProcessedRulesLayers, RulesLayerStack};
use crate::rules::prerequisite::{Prerequisite, PrerequisiteGroup, parse_prerequisites};
use crate::rules::ruleset::RuleSet;
use serde_json::Value;

// Existing BuildingType slots supplied by the native harness, not values
// derived from this implementation.
fn oracle_building_index(name: &str) -> Option<i32> {
    ["A", "B", "COLLIDE"]
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(name))
        .map(|index| index as i32)
}

fn oracle_catalog() -> GenericPrerequisites {
    let mut catalog = GenericPrerequisites::default();
    catalog.apply_pass(
        &IniFile::from_str("[GenericPrerequisites]\ncustom=,,,\nfuture=,,,\nCOLLIDE=,,,\n"),
        oracle_building_index,
        |_| None,
    );
    catalog
}

fn native_code(entry: Prerequisite) -> i64 {
    match entry {
        Prerequisite::Building(index) => i64::from(index),
        Prerequisite::Group(group) => -1 - group.index() as i64,
        Prerequisite::GenericGroup(index) => -1 - i64::from(index),
    }
}

#[test]
fn generic_parser_matches_executed_ares_read_controls() {
    let corpus = crate::test_fixture::json("tools/rules_oracle/ares_prerequisites.json");
    let catalog = oracle_catalog();
    let mut compared = 0;
    for row in corpus["controls"].as_array().unwrap() {
        if row.get("supplied_read_result").is_none() {
            continue;
        }
        let prior: Vec<_> = row["initial"]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| Prerequisite::Building(i32::try_from(index.as_i64().unwrap()).unwrap()))
            .collect();
        let mut section = IniSection::new("Unit".to_owned());
        if let Some(text) = row["supplied_read_result"].as_str() {
            section.set("Prerequisite", text);
        }
        let result = parse_prerequisites(
            section.read_list("Prerequisite", 0x800),
            &prior,
            oracle_building_index,
            Some(&catalog),
        );
        assert_eq!(
            serde_json::to_value(result.into_iter().map(native_code).collect::<Vec<_>>()).unwrap(),
            row["result"],
            "{}",
            row["name"]
        );
        compared += 1;
    }
    assert_eq!(compared, 9, "all saved Parse controls must be compared");
}

#[test]
fn generic_load_precedence_matches_executed_ares_controls() {
    use crate::rules::object_type::ObjectCategory;

    let corpus = crate::test_fixture::json("tools/rules_oracle/ares_prerequisites.json");
    let mut compared = 0;
    for row in corpus["controls"].as_array().unwrap() {
        let Some(reads) = row.get("reads").and_then(Value::as_object) else {
            continue;
        };
        let group_name = row["group"].as_str().unwrap();
        let initial: Vec<_> = row["initial"]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| ["A", "B", "COLLIDE"][index.as_u64().unwrap() as usize])
            .collect();
        let mut initial = IniFile::from_str(&format!(
            "[GenericPrerequisites]\n{group_name}={}\n",
            initial.join(",")
        ));
        let alternates = row["initial_alternate_names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect::<Vec<_>>()
            .join(",");
        if !alternates.is_empty() {
            initial
                .projection_section_mut("General")
                .set("PrerequisiteCustomAlternate", &alternates);
        }
        let techno = |name: &str| match name {
            "TANK" => Some((ObjectCategory::Vehicle, name.to_owned())),
            "INF" => Some((ObjectCategory::Infantry, name.to_owned())),
            _ => None,
        };
        let mut catalog = oracle_catalog();
        catalog.apply_pass(&initial, oracle_building_index, techno);
        let mut pass = IniFile::empty();
        for (path, value) in reads {
            let (section, key) = path.split_once('/').unwrap();
            pass.projection_section_mut(section)
                .set(key, value.as_str().unwrap());
        }
        catalog.apply_pass(&pass, oracle_building_index, techno);
        let entry = catalog.group_entry(group_name).unwrap();
        let group = catalog.group(entry.group_index().unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(
                group
                    .members()
                    .iter()
                    .copied()
                    .map(native_code)
                    .collect::<Vec<_>>()
            )
            .unwrap(),
            row["result"],
            "{}",
            row["name"]
        );
        assert_eq!(
            serde_json::to_value(
                group
                    .alternates()
                    .iter()
                    .map(|item| item.id())
                    .collect::<Vec<_>>()
            )
            .unwrap(),
            row["alternate_names"],
            "{}",
            row["name"]
        );
        compared += 1;
    }
    assert_eq!(
        compared, 6,
        "all saved LoadFromINI controls must be compared"
    );
}

const TYPE_RULES: &str = "\
[BuildingTypes]
0=A
1=B
2=COLLIDE
[VehicleTypes]
0=UNIT
[GenericPrerequisites]
custom=A,B
COLLIDE=B
[UNIT]
Prerequisite=custom,COLLIDE,POWER
";

#[test]
fn ares_build_resolves_custom_members_and_building_collision_without_ini_opt_in() {
    let rules = RuleSet::from_ini(&IniFile::from_str(TYPE_RULES)).unwrap();
    assert_eq!(
        rules
            .ares_prerequisite_group(Prerequisite::GenericGroup(6))
            .unwrap()
            .members(),
        [Prerequisite::Building(0), Prerequisite::Building(1)]
    );
    assert_eq!(
        rules.object("UNIT").unwrap().prerequisite,
        [
            Prerequisite::GenericGroup(6),
            Prerequisite::Building(2),
            Prerequisite::Group(PrerequisiteGroup::Power),
        ]
    );
}

#[test]
fn registration_keeps_source_order_first_spelling_and_exact_read_keys() {
    let mut catalog = GenericPrerequisites::default();
    catalog.apply_pass(
        &IniFile::from_str(
            "[General]\nPrerequisiteCustom=B\n\
             [GenericPrerequisites]\ncustom=future\nfuture=A\n",
        ),
        oracle_building_index,
        |_| None,
    );
    assert_eq!(
        catalog.group(6).unwrap().members(),
        [Prerequisite::GenericGroup(7)]
    );
    assert_eq!(
        catalog.group(7).unwrap().members(),
        [Prerequisite::Building(0)]
    );
    catalog.apply_pass(
        &IniFile::from_str(
            "[General]\nPrerequisiteCustom=A\n\
             [GenericPrerequisites]\nCUSTOM=B\nfuture=,,,\n",
        ),
        oracle_building_index,
        |_| None,
    );
    // Case-insensitive registration finds the first spelling, but the
    // subsequent native INI read still names that exact stored spelling.
    assert_eq!(catalog.groups().len(), 8);
    assert_eq!(catalog.group(6).unwrap().name(), "custom");
    assert_eq!(
        catalog.group(6).unwrap().members(),
        [Prerequisite::Building(0)]
    );
    assert!(catalog.group(7).unwrap().members().is_empty());
}

#[test]
fn ordinary_handoff_and_save_retain_groups_but_scenario_reset_rebuilds_them() {
    let first = RulesLayerStack::new(IniFile::from_str(&format!(
        "[General]\nPrerequisitePower=A\n{TYPE_RULES}"
    )))
    .process()
    .unwrap();
    let (_, trace) = first.into_ini_and_native_type_construction_trace();
    let retained = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(
            &IniFile::empty(),
            trace.into_registry_state_discarding_events(),
        )
        .unwrap();
    let saved = crate::util::save_codec::serialize(&retained).unwrap();
    let restored: ProcessedRulesLayers = crate::util::save_codec::deserialize(&saved).unwrap();
    restored.validate_saved_state().unwrap();
    let groups = restored.ares_prerequisites().unwrap();
    assert_eq!(groups.groups().len(), 8);
    assert_eq!(
        groups.group(6).unwrap().members(),
        [Prerequisite::Building(0), Prerequisite::Building(1)]
    );
    assert_eq!(
        groups.group(0).unwrap().members(),
        [Prerequisite::Building(0)]
    );
    assert_eq!(
        RuleSet::from_processed_rules(&restored)
            .unwrap()
            .simulation_config_hash(),
        RuleSet::from_processed_rules(&retained)
            .unwrap()
            .simulation_config_hash()
    );

    let (_, trace) = restored.into_ini_and_native_type_construction_trace();
    let reset = trace
        .into_registry_state_discarding_events()
        .destructive_reset();
    let reset = RulesLayerStack::new(IniFile::empty())
        .process_with_fixed_art_and_registry_state(&IniFile::empty(), reset)
        .unwrap();
    let groups = reset.ares_prerequisites().unwrap();
    assert_eq!(groups.groups().len(), PrerequisiteGroup::ALL.len());
    for (definition, native) in groups.groups().iter().zip(PrerequisiteGroup::ALL) {
        assert_eq!(definition.name(), native.token());
        assert!(definition.members().is_empty());
        assert!(definition.alternates().is_empty());
    }
}

#[test]
fn malformed_saved_prerequisites_are_rejected_before_projection() {
    use crate::rules::error::RulesError;

    let processed = RulesLayerStack::new(IniFile::from_str(&format!(
        "[General]\nPrerequisiteCustomAlternate=UNIT\n{TYPE_RULES}"
    )))
    .process()
    .unwrap();
    let saved = serde_json::to_value(processed).unwrap();
    let restored: ProcessedRulesLayers = serde_json::from_value(saved.clone()).unwrap();
    restored.validate_saved_state().unwrap();
    RuleSet::from_processed_rules(&restored).unwrap();

    for (case, path, replacement) in [
        (
            "missing enabled catalog",
            "/ares_prerequisites",
            Value::Null,
        ),
        (
            "unit requires an unallocated custom group",
            "/families/Vehicle/0/prerequisite/0",
            serde_json::json!({ "GenericGroup": u32::MAX }),
        ),
        (
            "existing alternate ID has the wrong category",
            "/ares_prerequisites/groups/6/alternates/0/category",
            serde_json::json!("Aircraft"),
        ),
    ] {
        let mut corrupted = saved.clone();
        let registry = &mut corrupted["native_type_construction_trace"]["registry_state"];
        *registry
            .pointer_mut(path)
            .expect("fixture reaches retained prerequisite state") = replacement;
        let restored: ProcessedRulesLayers = serde_json::from_value(corrupted).unwrap();
        assert!(
            matches!(
                restored.validate_saved_state(),
                Err(RulesError::InvalidSavedState(_))
            ),
            "{case} must fail retained-state validation"
        );
        assert!(
            matches!(
                RuleSet::from_processed_rules(&restored),
                Err(RulesError::InvalidSavedState(_))
            ),
            "{case} must fail before creating a RuleSet projection"
        );
    }
}

#[test]
fn generic_name_and_read_capacity_keep_instruction_boundaries() {
    // Name[32]/_TRUNCATE and ReadString2048 are instruction-level bounds.
    // CRT copying and ReadString are supplied oracle leaves, so these are
    // Rust regressions, not native boundary-execution goldens.
    let short = "N".repeat(31);
    let long = format!("{short}X");
    let mut catalog = GenericPrerequisites::default();
    catalog.apply_pass(
        &IniFile::from_str(&format!("[GenericPrerequisites]\n{short}=A\n{long}=B\n")),
        oracle_building_index,
        |_| None,
    );
    assert_eq!(catalog.groups().len(), 8);
    assert_eq!(catalog.group(6).unwrap().name(), short);
    assert_eq!(catalog.group(7).unwrap().name(), short);
    assert_eq!(catalog.group_entry(&long), None);

    let mut section = IniSection::new("Unit".to_owned());
    for (tail, expected_len, expected_last) in [
        ("B", 1024, Prerequisite::Building(1)),
        (",B", 1023, Prerequisite::Building(0)),
    ] {
        // Inject an already-stored value to exercise ReadString capacity
        // independently of the physical INI line buffer.
        section.set("Prerequisite", &format!("{}{tail}", "A,".repeat(1023)));
        let parsed = parse_prerequisites(
            section.read_list("Prerequisite", 0x800),
            &[],
            oracle_building_index,
            Some(&catalog),
        );
        assert_eq!(parsed.len(), expected_len);
        assert_eq!(parsed.last(), Some(&expected_last));
    }
}
