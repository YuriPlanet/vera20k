//! InfantryClass::ReceiveDamage's prone head (`0x00517FC1..0x00517FEF`) on the
//! production ProneDamage reader. Native rows:
//! tools/rules_oracle/read_double_percent.{py,json,meta.json}, group B.

use super::infantry_prone_raw_damage;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::rules::warhead_type::WarheadType;
use crate::sim::game_entity::{GameEntity, InfantryRuntime};
use serde_json::Value;

fn native() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/read_double_percent.json",
    ))
    .unwrap()
}

fn bits(value: &Value) -> u64 {
    u64::from_str_radix(value.as_str().unwrap(), 16).unwrap()
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn warhead(raw: &str) -> WarheadType {
    let ini = IniFile::from_str(&format!("[WH]\nProneDamage={raw}\n"));
    WarheadType::from_ini_section("WH", ini.section("WH").unwrap())
}

fn infantry(prone: bool) -> GameEntity {
    let mut entity = GameEntity::test_default(1, "E1", "Americans", 5, 5);
    entity.category = EntityCategory::Infantry;
    let mut runtime = InfantryRuntime::new();
    runtime.is_prone = prone;
    entity.infantry = Some(runtime);
    entity
}

#[test]
fn prone_head_matches_original_on_the_production_reader() {
    let native = native();
    let prone = &native["prone"];
    let target = infantry(true);
    let damages: Vec<i32> = prone["damages"]
        .as_array()
        .unwrap()
        .iter()
        .map(int)
        .collect();
    let rows = prone["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 15);
    for row in rows {
        let raw = row["raw"].as_str().unwrap();
        let warhead = warhead(raw);
        assert_eq!(
            warhead.prone_damage_f64.to_bits(),
            bits(&row["multiplier_bits"]),
            "ProneDamage={raw}"
        );
        let results = row["results"].as_array().unwrap();
        assert_eq!(results.len(), damages.len());
        for (&damage, result) in damages.iter().zip(results) {
            assert_eq!(
                infantry_prone_raw_damage(&target, &warhead, damage, false),
                int(result),
                "ProneDamage={raw}, damage {damage}"
            );
        }
    }
    let extremes = prone["extremes"].as_array().unwrap();
    assert_eq!(extremes.len(), 9);
    for row in extremes {
        let raw = row["raw"].as_str().unwrap();
        let damage = int(&row["damage"]);
        assert_eq!(
            infantry_prone_raw_damage(&target, &warhead(raw), damage, false),
            int(&row["result"]),
            "ProneDamage={raw}, damage {damage}"
        );
    }
    let seventy = warhead("70%");
    for row in prone["gates"].as_array().unwrap() {
        let target = infantry(row["prone"].as_bool().unwrap());
        assert_eq!(
            infantry_prone_raw_damage(
                &target,
                &seventy,
                int(&row["damage"]),
                row["ignore_defenses"].as_bool().unwrap(),
            ),
            int(&row["result"]),
            "{}",
            row["case"]
        );
    }
}

/// Retail `rulesmd.ini` (the local `ini/`; skipped without it): every
/// warhead's ProneDamage is the native reader's chopped product, one ulp
/// below the nearest-rounded one for `70%` (0.7's own double) and `80%` (one
/// ulp below 0.8), so those warheads cut some prone hits one lower.
#[test]
fn retail_prone_damage_reads_the_original_multipliers() {
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let rules = RuleSet::from_ini(&ini).unwrap();
    let native = native();
    let native_rows = native["prone"]["rows"].as_array().unwrap();
    let native_row = |raw: &str| {
        native_rows
            .iter()
            .find(|row| row["raw"] == raw && row["retail"] == true)
            .unwrap_or_else(|| panic!("no native row for retail ProneDamage={raw}"))
    };
    let mut authored = 0;
    for warhead in rules.warheads_iter() {
        let Some(raw) = ini
            .section(&warhead.id)
            .and_then(|section| section.get_for_test("ProneDamage"))
        else {
            assert_eq!(warhead.prone_damage_f64, 1.0, "{} default", warhead.id);
            continue;
        };
        authored += 1;
        assert_eq!(
            warhead.prone_damage_f64.to_bits(),
            bits(&native_row(raw)["multiplier_bits"]),
            "{} ProneDamage={raw}",
            warhead.id
        );
    }
    // 60 sections set ProneDamage. Nothing allocates SANoBuilding, and RPG's
    // only weapon, [RPGTower], is referenced by nothing.
    assert_eq!(authored, 58);

    // Deployed GI Para (SSA 80%, 25), elite Conscript M1CarbineE (SA 70%, 20),
    // SquidPunch (HE 70%, 100) and the elite Kirov's BlimpBombE (KTSTLEXP 70%,
    // 250), whose section is also an Animation.
    let target = infantry(true);
    let hit = |warhead: &str, damage| {
        infantry_prone_raw_damage(&target, rules.warhead(warhead).unwrap(), damage, false)
    };
    assert_eq!(hit("SSA", 25), 19);
    assert_eq!(hit("SA", 20), 13);
    assert_eq!(hit("HE", 100), 69);
    assert_eq!(hit("KTSTLEXP", 250), 174);
}
