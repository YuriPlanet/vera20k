use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::components::Health;
use serde_json::Value;

fn bits(row: &Value, key: &str) -> NativeF64Bits {
    NativeF64Bits::from_bits(u64::from_str_radix(row[key].as_str().unwrap(), 16).unwrap())
}

#[test]
fn native_health_term_spills_and_retaliation_predicates_preserve_nonfinite_values() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/threat_health.json"
    ))
    .unwrap();
    let rows = corpus["health_terms"].as_array().unwrap();
    assert_eq!(rows.len(), 672);
    for row in rows {
        let health = Health {
            current: row["current"].as_i64().unwrap() as i32,
        };
        let strength = row["strength"].as_i64().unwrap() as i32;
        let ratio = health.ratio(strength);
        let before_store = ScoreX87::add(
            ScoreX87::mul(ratio, ScoreX87::load_f64(bits(row, "coefficient_bits"))),
            ScoreX87::load_f64(bits(row, "previous_bits")),
        );
        assert_eq!(
            ScoreX87::store_f64_masked_chop(before_store),
            bits(row, "health_spill_bits"),
            "{row}"
        );
        let result = ScoreX87::add(
            ScoreX87::add(
                ScoreX87::mul(
                    ScoreX87::load_i32(row["beyond"].as_i64().unwrap() as i32),
                    ScoreX87::load_f64(bits(row, "distance_coefficient_bits")),
                ),
                spill_threat_double(before_store),
            ),
            load_threat_double(THREAT_SCORE_BASE),
        );
        assert_eq!(
            truncate_score(result),
            row["score"].as_i64().unwrap() as i32,
            "{row}"
        );
    }
    let comparisons = corpus["retaliation_comparisons"].as_array().unwrap();
    assert_eq!(comparisons.len(), 49);
    for row in comparisons {
        assert_eq!(
            super::super::combat_targeting::retaliation_score_refuses(
                ScoreX87::load_f64(bits(row, "current_bits")),
                ScoreX87::load_f64(bits(row, "attacker_bits"))
            ),
            row["refuse"].as_bool().unwrap(),
            "{row}"
        );
    }
}

#[test]
fn actual_scorer_consumes_live_signed_strength_and_masked_zero_division() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/threat_health.json"
    ))
    .unwrap();
    let mut compared = 0;
    for row in corpus["health_terms"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["previous_bits"] == "0000000000000000" && row["beyond"] == 0)
    {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[VehicleTypes]\n0=SCORER\n1=CANDIDATE\n[SCORER]\nStrength=100\n[CANDIDATE]\nStrength={}\n",
            row["strength"]))).unwrap();
        let mut interner = crate::sim::intern::test_interner();
        let mut entities = EntityStore::new();
        let mut scorer = GameEntity::test_default(1, "SCORER", "Americans", 2, 2);
        scorer.type_ref = interner.intern("SCORER");
        entities.insert(scorer);
        let mut candidate = GameEntity::test_default(2, "CANDIDATE", "Soviet", 2, 2);
        candidate.type_ref = interner.intern("CANDIDATE");
        candidate.health.current = row["current"].as_i64().unwrap() as i32;
        entities.insert(candidate);
        let result = calculate_threat_score(
            &entities,
            1,
            2,
            &rules,
            &interner,
            None,
            None,
            ThreatCoefficients {
                my_effectiveness: 0.0,
                target_effectiveness: 0.0,
                target_special_threat: 0.0,
                target_strength: f64::from_bits(bits(row, "coefficient_bits").bits()),
                target_distance: -10.0,
            },
            ThreatReference::NullCoord,
        )
        .unwrap();
        assert_eq!(
            truncate_score(result),
            row["score"].as_i64().unwrap() as i32,
            "{row}"
        );
        compared += 1;
    }
    assert_eq!(compared, 168);
}
