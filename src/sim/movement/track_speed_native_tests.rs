use super::*;
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/track_speed_native.json"
    ))
    .unwrap()
}

fn native64(value: &Value) -> NativeF64Bits {
    NativeF64Bits::from_bits(u64::from_str_radix(value.as_str().unwrap(), 16).unwrap())
}

fn integer(input: &Value, key: &str, fallback: i32) -> i32 {
    input[key].as_i64().map_or(fallback, |value| value as i32)
}

fn flag(input: &Value, key: &str, fallback: bool) -> bool {
    input[key].as_bool().unwrap_or(fallback)
}

fn getter_inputs(input: &Value) -> FootSpeedInputs {
    FootSpeedInputs {
        raw_type_speed: integer(input, "raw", 17),
        house_multiplier: NativeF32Bits::from_bits(
            u32::from_str_radix(input["house_bits"].as_str().unwrap(), 16).unwrap(),
        ),
        crate_multiplier: native64(&input["crate_bits"]),
        faster: flag(input, "faster", false),
        veteran_multiplier: native64(&input["veteran_bits"]),
        applied_fraction: native64(&input["applied_bits"]),
        unit_flag_carrier: integer(input, "flag_owner", -1) != -1,
    }
}

#[test]
fn live_getter_matches_original_staged_house_crate_veterancy_fraction_and_ctf() {
    let corpus = corpus();
    let cases = corpus["getters"].as_array().unwrap();
    assert_eq!(cases.len(), 75);
    for case in cases {
        assert_eq!(
            current_speed(getter_inputs(&case["input"])).unwrap(),
            case["output"].as_i64().unwrap() as i32,
            "{}",
            case["state"]
        );
    }
}

#[test]
fn drive_ship_prefixes_match_original_fraction_bits_setter_gates_distance_and_wallet() {
    let corpus = corpus();
    let cases = corpus["prefixes"].as_array().unwrap();
    assert_eq!(cases.len(), 116);
    let mut families = [0; 2];
    let mut distance_cases = 0;
    for case in cases {
        let input = &case["input"];
        let expected = &case["output"];
        families[usize::from(input["family"] == "ship")] += 1;
        let distance = if let Some(native_distance) = expected["distance"].as_i64() {
            let current: [i32; 3] = if input["current"].is_null() {
                [2176, 2176, 208]
            } else {
                serde_json::from_value(input["current"].clone()).unwrap()
            };
            let destination =
                serde_json::from_value(input["resolved_destination"].clone()).unwrap();
            let actual = braking_distance(current, destination).unwrap();
            assert_eq!(actual, native_distance as i32, "distance {input}");
            distance_cases += 1;
            actual
        } else {
            // The original bypass did not evaluate geometry at all.
            i32::MIN
        };
        let actual = track_prefix(TrackSpeedInputs {
            target: native64(&input["target_bits"]),
            applied: native64(&input["applied_bits"]),
            accelerates: flag(input, "accelerates", true),
            is_unit: true,
            unit_passive: flag(input, "passive", false),
            selector: integer(input, "selector", 1),
            raw_type_speed: integer(input, "raw", 17),
            acceleration: native64(&input["accel_bits"]),
            deceleration: native64(&input["decel_bits"]),
            slowdown_distance: integer(input, "slowdown", 500),
            distance,
            sinking: flag(input, "sinking", false),
            crush_slowdown: flag(input, "crush", false),
        })
        .unwrap();
        assert_eq!(
            actual.target,
            native64(&expected["target_bits"]),
            "target {input}"
        );
        assert_eq!(
            actual.applied,
            native64(&expected["applied_bits"]),
            "applied {input}"
        );
        assert_eq!(
            usize::from(actual.called_setter),
            expected["setters"].as_u64().unwrap() as usize,
            "setter {input}"
        );
        assert_eq!(
            actual.propagate_to_linked_units,
            expected["propagate"].as_bool().unwrap(),
            "linked-unit arm {input}"
        );
        let mut getter = getter_inputs(&case["getter"]);
        getter.applied_fraction = actual.applied;
        let speed = current_speed(getter).unwrap();
        assert_eq!(expected["getters"], 1, "retry must still evaluate getter");
        assert_eq!(
            invocation_budget(
                speed,
                integer(input, "residual", 7),
                flag(input, "retry", false)
            ),
            integer(expected, "budget", 0),
            "wallet {input}"
        );
    }
    assert_eq!(families, [58, 58]);
    assert_eq!(distance_cases, 88);
}

#[test]
fn setter_matches_original_finite_owner_corpus() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/foot_speed_owner.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let requested =
            NativeF64Bits::from_bits(case["input"]["requested"].as_f64().unwrap().to_bits());
        let expected =
            NativeF64Bits::from_bits(case["output"]["applied"].as_f64().unwrap().to_bits());
        assert_eq!(set_fraction(requested).unwrap(), expected);
    }
}
