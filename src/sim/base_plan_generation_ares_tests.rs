//! Ares3.0 ListContainsAll10008370 controls and planner integration.

use super::*;
use crate::rules::ini_parser::IniFile;

#[test]
fn planned_prerequisites_match_executed_controls() {
    let data = crate::test_fixture::json("tools/rules_oracle/ares_prerequisites.json");
    for row in data["controls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row.get("planned_building_indices").is_some())
    {
        let members = row["generic_custom_members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|code| ["A", "B", "COLLIDE"][code.as_u64().unwrap() as usize])
            .collect::<Vec<_>>();
        let alternates = row["alternate_names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| name.as_str().unwrap())
            .collect::<Vec<_>>();
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[BuildingTypes]\n0=A\n1=B\n2=COLLIDE\n3=Q\n\
             [GenericPrerequisites]\ncustom={}\n\
             [General]\nPrerequisiteCustomAlternate={}\n",
            if members.is_empty() {
                ",,,".to_owned()
            } else {
                members.join(",")
            },
            alternates.join(",")
        )))
        .unwrap();
        let mut candidate = rules.object("Q").unwrap().clone();
        candidate.prerequisite = row["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|code| match code.as_i64().unwrap() {
                code if code >= 0 => Prerequisite::Building(code as i32),
                code => Prerequisite::GenericGroup((-code - 1) as u32),
            })
            .collect();
        let present = row["planned_building_indices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| {
                rules
                    .building_type_at(index.as_i64().unwrap() as i32)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            prerequisites_satisfied(&candidate, &present, &rules),
            row["result"].as_bool().unwrap(),
            "{}",
            row["name"]
        );
    }
}

#[test]
fn generic_stock_groups_replace_ai_families_and_ignore_alternates() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=MEMBER\n1=AI_POWER\n2=ALTERNATE\n3=Q\n\
         [AI]\nBuildPower=AI_POWER\n\
         [General]\nPrerequisitePower=MEMBER\nPrerequisitePowerAlternate=ALTERNATE\n\
         [Q]\nPrerequisite=POWER\n",
    ))
    .unwrap();
    let candidate = rules.object("Q").unwrap();
    for (name, expected) in [("MEMBER", true), ("AI_POWER", false), ("ALTERNATE", false)] {
        assert_eq!(
            prerequisites_satisfied(candidate, &[rules.object(name).unwrap()], &rules),
            expected,
            "{name}"
        );
    }
}

#[test]
fn planned_group_keeps_same_pass_boundary() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=A\n1=B\n2=Q\n\
         [GenericPrerequisites]\ncustom=B\n[Q]\nPrerequisite=custom\n",
    ))
    .unwrap();
    let candidate = rules.object("Q").unwrap();
    let mut priority = vec![rules.object("A").unwrap()];
    let pass_start_len = priority.len();
    priority.push(rules.object("B").unwrap());
    assert!(!prerequisites_satisfied(
        candidate,
        &priority[..pass_start_len],
        &rules
    ));
    assert!(prerequisites_satisfied(candidate, &priority, &rules));
}

#[test]
fn planned_members_do_not_recurse_into_another_group() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=A\n1=Q\n\
         [GenericPrerequisites]\ninner=A\nouter=inner\n[Q]\nPrerequisite=outer\n",
    ))
    .unwrap();
    assert!(!prerequisites_satisfied(
        rules.object("Q").unwrap(),
        &[rules.object("A").unwrap()],
        &rules
    ));
}
