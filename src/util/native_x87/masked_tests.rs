//! Bounded value comparisons against sequenced original retail instructions.
use super::*;
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Deserialize)]
struct Row {
    input: Input,
    output_bits: Option<String>,
    ordering: Option<String>,
    integer_low: Option<i32>,
    original_opcodes_authenticated: bool,
    agrees_with_unicorn: bool,
}

#[derive(Deserialize)]
struct Input {
    name: String,
    operation: String,
    lhs_format: String,
    lhs_bits: String,
    rhs_format: Option<String>,
    rhs_bits: Option<String>,
}

fn load(format: &str, bits: &str) -> MaskedX87Value {
    match format {
        "f32" => MaskedX87Chop53::load_f32(NativeF32Bits::from_bits(
            u32::from_str_radix(bits, 16).unwrap(),
        )),
        "f64" => MaskedX87Chop53::load_f64(NativeF64Bits::from_bits(
            u64::from_str_radix(bits, 16).unwrap(),
        )),
        _ => panic!("unknown operand format"),
    }
}

#[test]
fn masked_values_match_hardware_instruction_fragments() {
    let rows: Vec<Row> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/x87_masked_hardware.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 2376);
    let mut names = BTreeSet::new();
    let mut counts = [0; 7];
    let mut mixed = 0;
    let mut emulator_disagreements = 0;
    for row in rows {
        let input = row.input;
        assert!(names.insert(input.name.clone()));
        assert!(row.original_opcodes_authenticated);
        emulator_disagreements += usize::from(!row.agrees_with_unicorn);
        let lhs = load(&input.lhs_format, &input.lhs_bits);
        let rhs = input
            .rhs_format
            .as_deref()
            .zip(input.rhs_bits.as_deref())
            .map(|(format, bits)| {
                mixed += usize::from(format != input.lhs_format);
                load(format, bits)
            });
        let value = match input.operation.as_str() {
            "load" => {
                counts[0] += 1;
                lhs
            }
            "add" => {
                counts[1] += 1;
                MaskedX87Chop53::add(lhs, rhs.unwrap())
            }
            "sub" => {
                counts[2] += 1;
                MaskedX87Chop53::sub(lhs, rhs.unwrap())
            }
            "mul" => {
                counts[3] += 1;
                MaskedX87Chop53::mul(lhs, rhs.unwrap())
            }
            "div" => {
                counts[4] += 1;
                MaskedX87Chop53::div(lhs, rhs.unwrap())
            }
            "compare" => {
                counts[5] += 1;
                let expected = match row.ordering.as_deref().unwrap() {
                    "less" => MaskedX87Ordering::Less,
                    "equal" => MaskedX87Ordering::Equal,
                    "greater" => MaskedX87Ordering::Greater,
                    "unordered" => MaskedX87Ordering::Unordered,
                    _ => panic!("unknown native condition"),
                };
                assert_eq!(
                    MaskedX87Chop53::compare(lhs, rhs.unwrap()),
                    expected,
                    "{}",
                    input.name
                );
                assert!(row.output_bits.is_none() && row.integer_low.is_none());
                continue;
            }
            "ftol" => {
                counts[6] += 1;
                assert_eq!(
                    MaskedX87Chop53::ftol_i32_low_masked(lhs),
                    row.integer_low.unwrap(),
                    "{}",
                    input.name
                );
                assert!(row.output_bits.is_none() && row.ordering.is_none());
                continue;
            }
            _ => panic!("unknown operation"),
        };
        assert!(row.ordering.is_none() && row.integer_low.is_none());
        assert_eq!(
            MaskedX87Chop53::store_f32_masked_chop(value).bits(),
            u32::from_str_radix(row.output_bits.as_deref().unwrap(), 16).unwrap(),
            "{}",
            input.name
        );
    }
    assert_eq!(counts, [28, 464, 464, 464, 464, 464, 28]);
    assert_eq!(mixed, 360);
    assert_eq!(emulator_disagreements, 228);
}
