use super::*;

/// Building draw43DA80 supplies the same yaw/pitch rotation consumed by
/// the shared rasterizer, including its C0-only route. The original matrix
/// corpus establishes that call-site connection independently of unit draw.
#[test]
fn building_gun_rotations_match_original_submitted_matrices() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/building_barrel.json",
    ))
    .unwrap();
    for (index, case) in native["draw_cases"].as_array().unwrap().iter().enumerate() {
        let facing = case["primary_raw"].as_u64().unwrap() as u16;
        let step = voxel_facing_step_u16(facing);
        let elevation = case["barrel_raw"].as_u64().unwrap() as u16;
        for draw in case["draws"].as_array().unwrap() {
            let params = VxlRenderParams {
                barrel_pitch: if draw["part"] == "barrel" {
                    voxel_facing_step_u16(elevation) as i8 - 8
                } else {
                    0
                },
                ..Default::default()
            };
            let matrix = voxel_params_draw_rotation(&params, step);
            for row in 0..3 {
                for col in 0..3 {
                    let expected =
                        f32::from_bits(draw["matrix_bits"][row * 4 + col].as_u64().unwrap() as u32);
                    assert_eq!(
                        matrix.col(col)[row],
                        expected,
                        "case {index}, {} rotation[{row},{col}]",
                        draw["part"]
                    );
                }
            }
        }
    }
}
