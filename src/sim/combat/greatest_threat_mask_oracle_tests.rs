//! `tools/threat_mask_oracle.py`'s native rows replayed: `Greatest_Threat`'s
//! flags word, `Evaluate_Candidate`'s All-To-Hunt and quarry terms with its
//! final acceptance, `Calculate_Threat_Score`'s SpecialThreatValue term
//! with `EnemyHouseThreatBonus=`, and `Evaluate_Candidate`'s distance cutoff.

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
        house.set_enemy_house_for_test(flag(&row["enemy"]).then_some(enemy));
        if flag(&row["bias"]) {
            house.strategy_emergency_mut().set_all_to_hunt_bias();
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

#[test]
fn distance_cutoff_matches_the_original() {
    let rows = rows("cutoff");
    assert_eq!(rows.len(), 173);
    for (number, row) in rows.iter().enumerate() {
        let coord = |key: &str| DriveCoord {
            x: int(&row[key][0]),
            y: int(&row[key][1]),
            z: int(&row[key][2]),
        };
        let high_flying = flag(&row["high_flying"]);
        assert_eq!(
            cutoff_distance(coord("scanner"), coord("candidate"), high_flying),
            int(&row["distance"]),
            "cutoff row {number}: {row}"
        );
        assert_eq!(
            !candidate_beyond_cutoff(
                coord("scanner"),
                coord("candidate"),
                high_flying,
                int(&row["range"])
            ),
            flag(&row["admitted"]),
            "cutoff row {number}: {row}"
        );
    }
}

/// Evaluate_Candidate's disguise arm (`0x006F84B1..0x006F854B`) executed
/// natively: the blink's remaining frames through `CdTimer::remaining`, the
/// gate, and one Scenario `RandomRanged(0, 99)` (stream offset `0x218`) only
/// for a computer attacker while the candidate blinks.
#[test]
fn disguise_arm_matches_the_original() {
    use crate::sim::cloak_disguise::{DisguiseGateOutcome, disguise_rejects_candidate};
    use crate::sim::house_state::HouseDifficulty;
    use crate::sim::rng::SimRng;
    use crate::sim::timer::CdTimer;

    let mut rules =
        RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str("")).expect("rules");
    rules.general.disabled_disguise_detection_percent = vec![15, 5, 2];
    let scanner = crate::sim::intern::test_interner().intern("Scanner");
    let rows = rows("disguise");
    assert!(
        rows.iter()
            .any(|row| row["admitted"] == true && !row["draws"].as_array().unwrap().is_empty())
    );
    for row in rows {
        let blink = CdTimer::from_raw(int(&row["start"]), int(&row["duration"]))
            .remaining(int(&row["frame"]));
        let outcome = disguise_rejects_candidate(
            flag(&row["disguised"]),
            flag(&row["detects"]),
            blink,
            flag(&row["human"]),
        );
        let draws = row["draws"].as_array().unwrap();
        let mut house = HouseState::new(scanner, 0, None, flag(&row["human"]), 0, 10);
        house.set_difficulty_for_test(
            HouseDifficulty::from_native(int(&row["difficulty"])).unwrap(),
        );
        let roll = int(&row["roll"]);
        let mut rng = SimRng::answering(0, 99, roll);
        let mut once = rng.clone();
        once.next_range_i32_inclusive(0, 99);
        let admitted = match outcome {
            DisguiseGateOutcome::Accept => true,
            DisguiseGateOutcome::Reject => false,
            DisguiseGateOutcome::AiDetectionRoll => {
                let admitted = disguise_detection_roll(&mut rng, Some(&house), &rules);
                assert_eq!(rng.logical_state(), once.logical_state(), "{row}");
                admitted
            }
        };
        assert_eq!(admitted, flag(&row["admitted"]), "{row}");
        assert_eq!(
            outcome == DisguiseGateOutcome::AiDetectionRoll,
            !draws.is_empty(),
            "{row}"
        );
        for draw in draws {
            assert_eq!(draw, &serde_json::json!([0x218, 0, 99]), "{row}");
        }
    }
}
