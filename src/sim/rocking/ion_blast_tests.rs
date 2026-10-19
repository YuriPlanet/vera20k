//! Original53CBE0 numeric controls, with declared Map/surface boundaries.
use super::*;
use serde_json::Value;

#[test]
fn original_projected_samples_and_angle_words_match() {
    let rows: Vec<Value> =
        serde_json::from_str(crate::test_fixture::text("tools/ion_blast_forces.json")).unwrap();
    assert_eq!(rows.len(), 10);
    for row in rows {
        let input = &row["input"];
        let origin = [25728, 25728, 0];
        let position =
            std::array::from_fn(|i| origin[i] + input["delta"][i].as_i64().unwrap() as i32);
        let radius = projected_radius(origin, position);
        let calls = row["sample_calls"].as_array().unwrap();
        if calls.is_empty() {
            assert!(radius >= 256);
            continue;
        }
        assert_eq!(calls[0], serde_json::json!([256 + radius, 128]), "{input}");
        if input["sample"].as_i64().unwrap() < 0 || input["loaded"] != true {
            continue;
        }
        let mut state = Some(RockingState {
            angle_sideways: NativeF32Bits::from_bits(0x3e80_0000),
            angle_forwards: NativeF32Bits::from_bits(0xbf00_0000),
            vel_sideways: NativeF32Bits::from_bits(0x3e00_0000),
            vel_forwards: NativeF32Bits::from_bits(0xbe80_0000),
        });
        apply(
            &mut state,
            origin,
            position,
            input["facing"].as_u64().unwrap() as u16,
            input["frame"].as_i64().unwrap() as i32,
        );
        let expected: Vec<_> = row["angles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u32)
            .collect();
        assert_eq!(state.unwrap().native_bits().as_slice(), expected, "{input}");
    }
}
