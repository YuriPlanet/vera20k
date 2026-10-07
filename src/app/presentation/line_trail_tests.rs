use super::*;
use serde_json::Value;

fn coord(v: &Value) -> ProjectileCoord {
    ProjectileCoord::new(
        v[0].as_i64().unwrap() as i32,
        v[1].as_i64().unwrap() as i32,
        v[2].as_i64().unwrap() as i32,
    )
}

#[test]
fn native_line_trail_ring_fades_on_each_composite_and_detaches_before_retirement() {
    let fixture: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/line_trail.json",
    ))
    .unwrap();
    for row in fixture["rows"].as_array().unwrap() {
        let initial = &row["initial"]["trail"];
        let color = std::array::from_fn(|i| initial["rgb"][i].as_u64().unwrap() as u8);
        let mut runtime = LineTrails::default();
        runtime.attach(7, color, 16, row["detail"].as_i64().unwrap() as i32);
        let mut last = ProjectileCoord::new(256, 256, 0);
        for step in row["steps"].as_array().unwrap() {
            let visit = step["visit"].as_u64().unwrap() as i32;
            if visit <= 36 {
                last = ProjectileCoord::new(
                    256 + 16 * visit,
                    256,
                    if visit % 2 == 1 { 104 } else { 0 },
                );
            }
            if visit == 40 {
                runtime.detach(7);
            }
            runtime.composite(|_| Some(last));
            assert_eq!(
                runtime.trails.len(),
                step["registry_count"].as_u64().unwrap() as usize,
                "detail={}, visit={visit}",
                row["detail"]
            );
            if let Some(trail) = runtime.trails.first() {
                let native = &step["trail"];
                assert_eq!(trail.head, native["head"].as_u64().unwrap() as usize);
                assert_eq!(
                    trail.decrement,
                    native["decrement"].as_i64().unwrap() as i32
                );
                for (actual, native) in trail.samples.iter().zip(native["ring"].as_array().unwrap())
                {
                    assert_eq!(actual.coord, coord(&native["xyz"]), "visit={visit}");
                    assert_eq!(
                        actual.strength,
                        native["strength"].as_i64().unwrap() as i32,
                        "visit={visit}"
                    );
                }
            }
            let native_draws = step["calls"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|c| c["address"] == "0x4beac0")
                .count();
            assert_eq!(runtime.segments.len(), native_draws, "visit={visit}");
        }
    }
}

#[test]
fn native_line_trail_save_load_drops_ring_and_does_not_reconstruct_from_live_owner() {
    let fixture: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/line_trail.json",
    ))
    .unwrap();
    for row in fixture["persistence"].as_array().unwrap() {
        assert_eq!(row["loaded_trail_pointer"], 0);
        assert_eq!(row["postload_registry_count"], 0);
        let mut runtime = LineTrails::default();
        runtime.attach(7, [216, 216, 255], 16, 2);
        runtime.composite(|_| Some(ProjectileCoord::new(256, 256, 0)));
        runtime.clear_on_load();
        assert!(
            runtime
                .composite(|_| Some(ProjectileCoord::new(384, 256, 104)))
                .is_empty()
        );
        assert!(runtime.trails.is_empty());
    }
}

#[test]
fn native_line_trail_registry_emits_reverse_attached_order() {
    let fixture: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/line_trail.json",
    ))
    .unwrap();
    for row in fixture["ordered_pixel_cases"].as_array().unwrap() {
        let input = &row["input"];
        let origin = coord(&input["origin"]);
        let delta = coord(&input["delta"]);
        let mut runtime = LineTrails::default();
        for (index, color) in input["colors"].as_array().unwrap().iter().enumerate() {
            runtime.attach(
                index as u64,
                std::array::from_fn(|i| color[i].as_u64().unwrap() as u8),
                16,
                2,
            );
        }
        assert!(runtime.composite(|_| Some(origin)).is_empty());
        let actual = runtime.composite(|_| {
            Some(ProjectileCoord::new(
                origin.x + delta.x,
                origin.y + delta.y,
                origin.z + delta.z,
            ))
        });
        let expected = row["draw_calls"].as_array().unwrap();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(
                actual.color,
                std::array::from_fn(|i| expected["rgb"][i].as_u64().unwrap() as u8)
            );
            assert_eq!(
                actual.strength,
                expected["intensity"].as_i64().unwrap() as i32
            );
        }
    }
}
