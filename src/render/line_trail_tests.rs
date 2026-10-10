use super::*;
use serde_json::Value;

pub(super) fn native() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/line_trail.json",
    ))
    .unwrap()
}

pub(super) fn projected(call: &Value) -> ProjectedLine {
    ProjectedLine {
        from: std::array::from_fn(|i| call["from_point"][i].as_i64().unwrap() as i32),
        to: std::array::from_fn(|i| call["to_point"][i].as_i64().unwrap() as i32),
        z_adjust: [
            call["z_adjust"].as_i64().unwrap() as i32,
            call["z_adjust_end"].as_i64().unwrap() as i32,
        ],
        strength: call["intensity"].as_i64().unwrap() as i32,
    }
}

#[test]
fn native_line_trail_clipped_three_axis_raster_preserves_repeated_pixel_stores() {
    for row in native()["pixels"].as_array().unwrap() {
        let input = &row["input"];
        let z_origin = input["z_origin_y"].as_i64().unwrap_or(0) as i32;
        let old_z = input["old_z"].as_u64().unwrap() as u16;
        let alpha = |[x, y]: [i32; 2]| {
            if input["alpha"] == "mixed" {
                [0, 1, 63, 127, 255][((x / 9 + y / 7) % 5) as usize]
            } else {
                input["alpha"].as_u64().unwrap()
            }
        };
        let mut stores = Vec::new();
        for call in row["draw_calls"].as_array().unwrap() {
            let clip = std::array::from_fn(|i| call["clip"][i].as_i64().unwrap() as i32);
            rasterize(projected(call), clip, z_origin, |p| {
                if p.z < old_z && alpha(p.point) != 0 {
                    stores.push((p.point[1] * 64 + p.point[0]) * 2);
                }
            });
        }
        let expected: Vec<i32> = row["writes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["offset"].as_i64().unwrap() as i32)
            .collect();
        assert_eq!(stores, expected, "native case {}", row["name"]);
    }
}

#[test]
fn native_line_trail_world_projection_keeps_surface_clip_origin() {
    for row in native()["pixels"].as_array().unwrap() {
        let input = &row["input"];
        let origin: [i32; 3] = std::array::from_fn(|i| input["origin"][i].as_i64().unwrap() as i32);
        let delta: [i32; 3] = std::array::from_fn(|i| input["delta"][i].as_i64().unwrap() as i32);
        for call in row["draw_calls"].as_array().unwrap() {
            let segment = LineTrailSegment {
                from: ProjectileCoord::new(
                    origin[0] + delta[0],
                    origin[1] + delta[1],
                    origin[2] + delta[2],
                ),
                to: ProjectileCoord::new(origin[0], origin[1], origin[2]),
                color: [216, 216, 255],
                strength: call["intensity"].as_i64().unwrap() as i32,
            };
            let actual = segment.project([
                row["camera"][0].as_i64().unwrap() as i32,
                row["camera"][1].as_i64().unwrap() as i32 + 15,
            ]);
            let expected = projected(call);
            assert_eq!(actual.from, expected.from, "{}", row["name"]);
            assert_eq!(actual.to, expected.to, "{}", row["name"]);
            assert_eq!(actual.z_adjust, expected.z_adjust, "{}", row["name"]);
        }
    }
}
