//! Native pins for the house self-heal arithmetic (issue #1219).
//!
//! `tools/selfheal_oracle.json` holds the executed original results for the four
//! leaf functions the mechanism rests on:
//!
//! - `HouseClass::HasInfSelfHeal @ 0x0050D9C0` / `HasUnitSelfHeal @ 0x0050D9D0`
//!   — `return count > 0` over the signed range.
//! - `HouseClass::GetInfSelfHealStep @ 0x0050D9E0` /
//!   `GetUnitSelfHealStep @ 0x0050D9F0` — the amount times the count, with the
//!   *other* Rules field sentinel-set, so each row fixes which key an arm reads.
//!
//! These rows are executed native results, not hand calculations: they are what
//! establishes that the infantry arm reads `Rules+0x34` with `House+0x164` and
//! the unit arm `Rules+0x3C` with `House+0x168`, and that the step is a plain
//! 32-bit `imul`.
//!
//! Coverage limits: the rows exercise the leaves only. `TechnoClass::AI_Update`'s
//! two arms and the three `BuildingClass` lifecycle sites are read from
//! instructions; `techno_ai_selfheal_tests.rs` covers those as Rust regressions.

use super::*;
use crate::rules::ini_parser::IniFile;
use serde_json::Value;

fn vectors() -> Value {
    serde_json::from_str(crate::test_fixture::text("tools/selfheal_oracle.json")).unwrap()
}

fn rows(section: &str) -> Vec<Value> {
    vectors()[section].as_array().unwrap().clone()
}

fn int(value: &Value, key: &str) -> i32 {
    value[key].as_i64().unwrap() as i32
}

/// The predicate the heal arms consult before any arithmetic: only a positive
/// count admits the pulse, so zero and every negative value decline.
#[test]
fn the_count_predicate_matches_the_original() {
    let cases = rows("predicate");
    assert!(!cases.is_empty());
    for row in &cases {
        let count = int(row, "count");
        assert_eq!(
            count > 0,
            row["result"].as_i64().unwrap() != 0,
            "native predicate disagrees for {row}"
        );
        // Each arm loads its own counter; the row records which offset that was.
        assert_eq!(
            row["house_offset"].as_i64().unwrap(),
            if row["arm"] == "infantry" {
                0x164
            } else {
                0x168
            },
            "{row}"
        );
    }
}

/// `Get*SelfHealStep` is `amount * count` over the full 32-bit domain, and the
/// row's recorded Rules offset is the key that arm reads.
#[test]
fn the_step_is_the_amount_times_the_count_on_the_pinned_offsets() {
    let cases = rows("step");
    assert!(cases.len() > 50);
    for row in &cases {
        let (amount, count) = (int(row, "amount"), int(row, "count"));
        assert_eq!(
            amount.wrapping_mul(count),
            int(row, "step"),
            "native step disagrees for {row}"
        );
        let infantry = row["arm"] == "infantry";
        assert_eq!(
            row["house_offset"].as_i64().unwrap(),
            if infantry { 0x164 } else { 0x168 },
            "{row}"
        );
        // The executed rows are what pin the keys: SelfHealInfantryAmount is
        // Rules+0x34 and SelfHealUnitAmount is Rules+0x3C.
        assert_eq!(
            row["rules_amount_offset"].as_i64().unwrap(),
            if infantry { 0x34 } else { 0x3C },
            "{row}"
        );
    }
}

/// The port reads the same two keys, so a rules set authoring the stock values
/// yields exactly the amounts the executed rows used in that arm's role.
#[test]
fn the_authored_keys_fill_the_roles_the_rows_pin() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nSelfHealInfantryFrames=50\nSelfHealInfantryAmount=20\n\
         SelfHealUnitFrames=75\nSelfHealUnitAmount=5\n\
         [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
    ))
    .unwrap();
    for (arm, expected) in [("infantry", 20), ("units", 5)] {
        let row = rows("step")
            .into_iter()
            .find(|row| row["arm"] == arm && int(row, "amount") == expected)
            .unwrap_or_else(|| panic!("a {arm} step row authored at {expected}"));
        let port = if arm == "infantry" {
            rules.general.self_heal_infantry_amount
        } else {
            rules.general.self_heal_unit_amount
        };
        assert_eq!(
            port, expected,
            "the {arm} key must carry the amount the {arm} arm uses"
        );
        // And the row's own arithmetic agrees with what the port will compute.
        assert_eq!(int(&row, "step"), expected.wrapping_mul(int(&row, "count")));
    }
}
