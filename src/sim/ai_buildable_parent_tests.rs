//! Original FirstBuildable5051E0 mixed identity gates, not a second selector.
use super::*;
use crate::rules::ini_parser::IniFile;

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
