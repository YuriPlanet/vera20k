//! The native corpus contains full estimator rows and direct kernel rows.
//! Only actual observed `489180` invocations are compared here: this does not
//! claim implementation of the surrounding estimated-damage factor pipeline.

use super::*;
use crate::util::native_x87::X87Chop53;
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Deserialize)]
struct Corpus {
    row_count: usize,
    rows: Vec<Row>,
}

#[derive(Deserialize)]
struct Row {
    input: Input,
    result_i32: i32,
    callbacks: Vec<Callback>,
    floating_stores: Vec<Store>,
    original_code_and_tables_unchanged: bool,
}

#[derive(Deserialize)]
struct Input {
    name: String,
    scope: String,
    warhead_cell_spread: String,
    warhead_percent_at_max: String,
    warhead_verses: [String; 11],
    scenario_flags: u8,
    rules_max_damage: i32,
}

#[derive(Deserialize)]
struct Callback {
    name: String,
    damage: Option<i32>,
    warhead: Option<u32>,
    armor: Option<u8>,
    distance: Option<i32>,
}

#[derive(Deserialize)]
struct Store {
    instruction: String,
    bits: String,
}

fn corpus() -> Corpus {
    serde_json::from_str(include_str!(
        "../../../../tools/spatial_oracle/estimated_damage.json"
    ))
    .expect("original estimated-damage/kernel corpus")
}

fn f32_bits(bits: &str) -> NativeF32Bits {
    NativeF32Bits::from_bits(u32::from_str_radix(bits, 16).unwrap())
}

fn widened_f32(bits: &str) -> f64 {
    let raw = f32_bits(bits).bits();
    if raw & 0x7f80_0000 == 0x7f80_0000 {
        // Widen exceptional encodings deterministically, preserving the raw
        // quiet/signaling bit until the modeled native load quiets it.
        f64::from_bits(
            (u64::from(raw & 0x8000_0000) << 32)
                | 0x7ff0_0000_0000_0000
                | (u64::from(raw & 0x007f_ffff) << 29),
        )
    } else {
        f64::from(f32::from_bits(raw))
    }
}

fn verses(input: &Input) -> [f64; 11] {
    std::array::from_fn(|index| {
        f64::from_bits(u64::from_str_radix(&input.warhead_verses[index], 16).unwrap())
    })
}

#[test]
fn shared_kernel_matches_every_representable_native_invocation() {
    let corpus = corpus();
    assert_eq!(corpus.row_count, 1066);
    assert_eq!(corpus.rows.len(), corpus.row_count);
    let mut names = BTreeSet::new();
    let (mut compared, mut wrapper_only, mut null_warhead) = (0, 0, 0);
    let mut nonzero_distance = 0;
    for row in corpus.rows {
        assert!(names.insert(row.input.name.clone()));
        assert!(row.original_code_and_tables_unchanged);
        let calls: Vec<_> = row
            .callbacks
            .iter()
            .filter(|call| call.name == "warhead")
            .collect();
        let Some(call) = calls.first() else {
            // Null targets/weapon flags/nonpositive weapon damage return from
            // the native wrapper without entering the shared kernel.
            assert_eq!(row.input.scope, "estimator", "{}", row.input.name);
            wrapper_only += 1;
            continue;
        };
        assert_eq!(calls.len(), 1, "{}", row.input.name);
        if call.warhead == Some(0) {
            // This decoded-field API has no null pointer. Its production
            // caller owns admission; retain and check the native observation.
            assert_eq!(row.result_i32, 0, "{}", row.input.name);
            null_warhead += 1;
            continue;
        }
        let distance = call.distance.unwrap();
        if row.input.scope == "estimator" {
            assert_eq!(distance, 0);
            assert!(call.damage.unwrap() >= 1);
        }
        nonzero_distance += usize::from(distance != 0);
        let actual = apply_warhead_damage(
            call.damage.unwrap(),
            widened_f32(&row.input.warhead_cell_spread),
            widened_f32(&row.input.warhead_percent_at_max),
            &verses(&row.input),
            ArmorClass(call.armor.unwrap()),
            distance,
            row.input.scenario_flags & 0x20 != 0,
            row.input.rules_max_damage,
        );
        assert_eq!(actual, row.result_i32, "{}", row.input.name);
        compared += 1;
    }
    assert_eq!((compared, wrapper_only, null_warhead), (971, 31, 64));
    assert_eq!(nonzero_distance, 476);
}

#[test]
fn shared_masked_spills_match_original_stores_including_both_overflow_signs() {
    let mut checked = 0;
    let (mut positive_overflow, mut negative_overflow) = (false, false);
    for row in corpus().rows {
        let Some(call) = row.callbacks.iter().find(|call| call.name == "warhead") else {
            continue;
        };
        let Some(stored_damage) = row
            .floating_stores
            .iter()
            .find(|store| store.instruction == "004891CA")
        else {
            continue;
        };
        let stored_product = row
            .floating_stores
            .iter()
            .find(|store| store.instruction == "004891D4")
            .unwrap();
        let damage = X87::load_i32(call.damage.unwrap());
        let product = X87::mul(
            damage,
            X87::load_f32(f32_bits(&row.input.warhead_percent_at_max)),
        );
        assert_eq!(
            X87::store_f32_masked_chop(damage),
            f32_bits(&stored_damage.bits),
            "{}",
            row.input.name
        );
        assert_eq!(
            X87::store_f32_masked_chop(product),
            f32_bits(&stored_product.bits),
            "{}",
            row.input.name
        );
        let finite_overflow = X87Chop53::load_f32(f32_bits(&row.input.warhead_percent_at_max))
            .is_ok_and(|pam| {
                X87Chop53::store_f32(X87Chop53::mul(
                    X87Chop53::load_i32(call.damage.unwrap()),
                    pam,
                ))
                .is_err()
            });
        if finite_overflow {
            match stored_product.bits.as_str() {
                "7f7fffff" => positive_overflow = true,
                "ff7fffff" => negative_overflow = true,
                other => panic!("unexpected overflow store {other}: {}", row.input.name),
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 881);
    assert!(positive_overflow && negative_overflow);
}

#[test]
fn parser_admitted_percent_overflow_uses_native_masked_kernel() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::warhead_type::WarheadType;

    // Native exceptional_percent_{7f800000,ff800000}_{0,1}_0 rows:
    // spread0 bypasses interpolation; spread1 produces an invalid cancellation
    // then original _ftol's low32 indefinite. The complete parser admits these.
    for authored in ["1e40", "-1e40"] {
        for (spread, expected) in [(0, 100), (1, 0)] {
            let ini = IniFile::from_str(&format!(
                "[WH]\nCellSpread={spread}\nPercentAtMax={authored}\n"
            ));
            let warhead = WarheadType::from_ini_section("WH", ini.section("WH").unwrap());
            assert!(warhead.percent_at_max_f64.is_infinite());
            let actual = apply_warhead_damage(
                100,
                warhead.cell_spread_f64,
                warhead.percent_at_max_f64,
                &warhead.verses_f64,
                ArmorClass(2),
                0,
                false,
                i32::MAX,
            );
            assert_eq!(actual, expected, "PAM={authored}, spread={spread}");
        }
    }
}

#[test]
fn native_binary32_spill_can_change_distance_zero_damage() {
    let run = |spread| {
        apply_warhead_damage(
            16_777_217,
            spread,
            0.0,
            &[1.0; 11],
            ArmorClass(2),
            0,
            false,
            i32::MAX,
        )
    };
    assert_eq!(run(1.0), 16_777_216);
    assert_eq!(run(0.0), 16_777_217);
}
