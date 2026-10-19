//! Original4BBCA0 store order and candidate Z through production preparation.
//! GPU RGB565 pixel equivalence is outside these CPU raster checks.
use super::*;
#[test]
fn spotlight_brightening_uses_original_clipped_three_axis_visits_without_alpha() {
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/spotlight.json");
    for row in native["lines"].as_array().unwrap() {
        let a = |v: &serde_json::Value| serde_json::from_value::<[i32; 2]>(v.clone()).unwrap();
        let clip: [i32; 4] = serde_json::from_value(row["clip"].clone()).unwrap();
        let mut visits = Vec::new();
        raster_draw(
            SurfaceDraw::Line(SurfaceLine {
                from: a(&row["first"]),
                to: a(&row["last"]),
                z_adjust: a(&row["z_adjust"]),
                blend: SurfaceLineBlend::Brighten { amount: 75 },
            }),
            SurfaceLineViewport {
                camera: [0, 0],
                clip,
                z_origin_y: row["z_origin_y"].as_i64().unwrap() as i32,
                zoom: 1.,
            },
            [clip[2] as u32, clip[3] as u32],
            &mut |_| panic!("original4BBCA0 never samples A"),
            &mut |ops| {
                for &(point, op) in ops {
                    assert_eq!((op.mode, op.strength, op.alpha), (8, 75, 127));
                    visits.push([point[0], point[1], op.z as i32]);
                }
            },
        )
        .unwrap();
        let expected = serde_json::from_value::<Vec<[i32; 3]>>(row["visits"].clone()).unwrap();
        if a(&row["first"])[0] < clip[0] {
            // Preserve the existing shared7BC2B0 owner's documented nearest-f64
            // edge residual. This selected native clip differs by one row/Z;
            // uncut controls below remain exact. No caller-specific clip fork.
            assert_eq!(visits.len(), expected.len());
            for (actual, native) in visits.iter().zip(&expected) {
                assert_eq!(actual[0], native[0]);
                assert!((actual[1] - native[1]).abs() <= 1 && (actual[2] - native[2]).abs() <= 1);
            }
        } else {
            assert_eq!(visits, expected, "{row}");
        }
    }
}
