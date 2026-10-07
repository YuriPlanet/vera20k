//! `tools/threat_mask_oracle.py`'s native rows replayed: `Greatest_Threat`'s
//! flags word, `Evaluate_Candidate`'s All-To-Hunt and quarry terms with its
//! final acceptance, and `Calculate_Threat_Score`'s SpecialThreatValue term
//! with `EnemyHouseThreatBonus=`.

use super::*;
use crate::sim::house_state::HouseState;
use serde_json::Value;

fn rows(section: &str) -> Vec<Value> {
    let oracle: Value =
        serde_json::from_str(crate::test_fixture::text("tools/threat_mask_oracle.json")).unwrap();
    oracle[section].as_array().unwrap().clone()
}

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn flag(value: &Value) -> bool {
    value.as_bool().unwrap()
}

fn double(row: &Value, key: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(row[key].as_str().unwrap(), 16).unwrap())
}

#[test]
fn flags_word_matches_the_original() {
    for row in rows("flags") {
        let mask = row["mask"].as_u64().unwrap() as u32;
        assert_eq!(
            u64::from(super::super::threat_mask::flags_for(mask)),
            row["flags"].as_u64().unwrap(),
            "mask {mask:#x}"
        );
    }
}

#[test]
fn quarry_terms_and_acceptance_match_the_original() {
    let mut interner = crate::sim::intern::test_interner();
    let scanner = interner.intern("Scanner");
    let enemy = interner.intern("Enemy");
    let other = interner.intern("Other");
    for (number, row) in rows("quarry_terms").iter().enumerate() {
        let mut house = HouseState::new(scanner, 0, None, false, 0, 10);
        house.enemy_house = flag(&row["enemy"]).then_some(enemy);
        if flag(&row["bias"]) {
            house.strategy_emergency.set_all_to_hunt_bias();
        }
        let owner = if flag(&row["enemy_owns"]) {
            enemy
        } else {
            other
        };
        let all_to_hunt =
            crate::sim::house_strategy::all_to_hunt_score_override(&house, owner).is_some();
        let result = quarry_terms(
            row["mask"].as_u64().unwrap() as u32,
            int(&row["score"]),
            all_to_hunt,
            QuarryFacts {
                building: flag(&row["building"]),
                power_bonus: int(&row["power"]),
                max_occupants: int(&row["max_occupants"]),
                needs_engineer: flag(&row["needs_engineer"]),
                factory: flag(&row["factory"]),
                can_be_occupied: flag(&row["can_be_occupied"]),
                occupants: int(&row["occupants"]),
                armed: flag(&row["armed"]),
            },
        )
        .and_then(finish_score);
        assert_eq!(
            result,
            row["result"].as_i64().map(|score| score as i32),
            "quarry row {number}: {row}"
        );
    }
}

#[test]
fn special_threat_and_enemy_house_bonus_match_the_original() {
    for (number, row) in rows("enemy_bonus").iter().enumerate() {
        let bonus =
            (flag(&row["enemy"]) && flag(&row["enemy_owns"])).then(|| double(row, "bonus_bits"));
        let score = special_threat_terms(
            load_threat_double(double(row, "score_bits")),
            load_threat_double(double(row, "coefficient_bits")),
            double(row, "special_bits"),
            bonus,
        );
        assert_eq!(
            ScoreX87::store_f64_masked_chop(score).bits(),
            u64::from_str_radix(row["result_bits"].as_str().unwrap(), 16).unwrap(),
            "bonus row {number}: {row}"
        );
    }
}
