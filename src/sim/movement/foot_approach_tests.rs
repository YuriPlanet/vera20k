//! Original candidate geometry observations, not a second search implementation.
use super::*;
use serde_json::Value;

#[test]
fn original_approach_candidate_geometry_and_order() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fv_cell_attack/approach_vectors.json",
    ))
    .unwrap();
    assert_eq!(
        native["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    let original_offsets = native["original_constants"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|row| row.get("signed_offsets"))
        .unwrap();
    assert_eq!(serde_json::json!(OFFSETS), *original_offsets);
    for (i, offset) in native["numeric"]["neighbor_offsets"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(serde_json::json!(cell_delta_unchecked(i as u8)), *offset);
    }
    let mut count = 0;
    for pose in native["numeric"]["poses"].as_array().unwrap() {
        let source: [i32; 2] = serde_json::from_value(pose["source_xy"].clone()).unwrap();
        let target: [i32; 2] = serde_json::from_value(pose["target_xy"].clone()).unwrap();
        let facing = facing16_between(target, source);
        assert_eq!(u64::from(facing), pose["facing_word"].as_u64().unwrap());
        let base = ((u32::from(facing) + 128) >> 8) as u8;
        assert_eq!(u64::from(base), pose["base_direction"].as_u64().unwrap());
        for row in pose["candidates"].as_array().unwrap() {
            let offset = row["offset"].as_i64().unwrap() as i16;
            let direction = (i16::from(base).wrapping_add(offset) as u16) << 8;
            let radius = row["radius"].as_i64().unwrap() as i32;
            let actual = facing_step_world_xy(target, direction, radius);
            let expected: [i32; 3] = serde_json::from_value(row["unsnapped_xyz"].clone()).unwrap();
            assert_eq!(
                actual,
                [expected[0], expected[1]],
                "source={source:?} target={target:?} offset={offset} radius={radius}"
            );
            count += 1;
        }
    }
    assert_eq!(count, 465);
}
