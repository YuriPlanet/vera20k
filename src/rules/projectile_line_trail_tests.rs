use super::*;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;
use serde_json::Value;

fn native() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/line_trail.json",
    ))
    .unwrap()
}

#[test]
fn production_line_trail_art_reader_matches_native_retention_and_defined_rgb_scans() {
    for case in native()["reader_controls"].as_array().unwrap() {
        let mut art = String::from("[DRAGON]\n");
        for (key, value) in case["input"].as_object().unwrap() {
            art.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
        art.push_str("[PLAIN]\nFixtureOnly=1\n");
        let art = IniFile::from_str(&art);
        let first = IniFile::from_str(
            "[General]\nMetallicDebris=D\n[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nProjectile=SHOT\n[SHOT]\nImage=DRAGON\n",
        );
        let mut stack = RulesLayerStack::new(first);
        for field in ["first", "omitted"] {
            if field == "omitted" {
                stack.push(
                    RulesLayerKind::Scenario,
                    IniFile::from_str("[SHOT]\nImage=PLAIN\n"),
                );
            }
            let processed = stack.process_with_fixed_art(&art).unwrap();
            let rules = RuleSet::from_processed_rules(&processed).unwrap();
            let actual = rules.projectile("SHOT").unwrap();
            let expected = &case[field];
            assert_eq!(
                actual.use_line_trail,
                expected["enabled"].as_bool().unwrap(),
                "{case}"
            );
            assert_eq!(
                actual.line_trail_color_decrement,
                expected["decrement"].as_i64().unwrap() as i32,
                "{case}"
            );
            let raw = case["input"]["LineTrailColor"].as_str();
            if matches!(
                raw,
                Some("1,2" | "1h,2,3" | "junk" | "1%" | "1,2%,3" | "1 ,2,3")
            ) {
                // Original copies uninitialized caller stack for incomplete
                // scans (independent rgb_stack_controls). Chosen deterministic
                // malformed fallback is retained RGB, not parity with residue.
                assert_eq!(actual.line_trail_color, [128; 3]);
            } else {
                assert_eq!(
                    actual.line_trail_color,
                    std::array::from_fn(|i| expected["color"][i].as_u64().unwrap() as u8),
                    "{case}"
                );
            }
        }
    }
}

#[test]
fn physical_dragon_line_trail_art_matches_original_object_reader() {
    use crate::rules::retail_ini_fixture::retail_ini;
    let (Some(rules), Some(art)) = (retail_ini("rulesmd.ini"), retail_ini("artmd.ini")) else {
        return;
    };
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&rules, &art).unwrap();
    let actual = rules.projectile("AAHeatSeeker2").unwrap();
    let fixture = native();
    let expected = &fixture["type_layers"][0]["line_trail"];
    assert_eq!(
        actual.use_line_trail,
        expected["enabled"].as_bool().unwrap()
    );
    assert_eq!(
        actual.line_trail_color,
        std::array::from_fn(|i| expected["color"][i].as_u64().unwrap() as u8)
    );
    assert_eq!(
        actual.line_trail_color_decrement,
        expected["decrement"].as_i64().unwrap() as i32
    );
}
