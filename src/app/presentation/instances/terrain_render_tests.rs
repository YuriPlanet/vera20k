//! Native retained Terrain coordinates through static and animated DrawIt.
//! The corpus stops at the shape sink; this compares instance inputs, not pixels.

use super::native_terrain_instances;
use crate::render::overlay_atlas::OverlaySpriteEntry;

#[test]
fn terrain_retained_xyz_projection_and_piece_z_match_original_render() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/terrain_render.json",
    ))
    .unwrap();
    let static_rows = native["rows"].as_array().unwrap();
    let animated_rows = native["animated_rows"].as_array().unwrap();
    assert_eq!(static_rows.len(), 82);
    assert_eq!(animated_rows.len(), 44);
    let rows = static_rows.iter().chain(animated_rows);
    // Zero stored offsets isolate the original pre-shape draw point. Actual
    // image/canvas offsets have their separate production atlas comparisons.
    let sprite = OverlaySpriteEntry {
        uv_origin: [0.0; 2],
        uv_size: [1.0; 2],
        pixel_size: [1.0; 2],
        offset_x: 0.0,
        offset_y: 0.0,
    };
    for row in rows {
        let draws = row["draws"].as_array().unwrap();
        assert_eq!(draws.len(), 2, "{row}: ordinary body and shadow");
        let coord = row["retained"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_i64().unwrap() as i32)
            .collect::<Vec<_>>();
        // Same exact projection and lift used by build_overlay_instances.
        let point = crate::util::lepton::absolute_leptons_to_screen(coord[0], coord[1], coord[2]);
        let lift = crate::util::native_x87::adjust_for_z_standard(coord[2]);
        assert_eq!(i64::from(lift), row["lift_px"].as_i64().unwrap(), "{row}");
        // VERA's common world-pixel frame has a documented +15 row origin;
        // original CoordsToClient2 and the saved native point do not.
        let native_frame_point = [point.0, point.1 - crate::map::terrain::TILE_HEIGHT / 2.0];
        assert_eq!(
            native_frame_point,
            [
                row["projected_point"][0].as_i64().unwrap() as f32,
                row["projected_point"][1].as_i64().unwrap() as f32,
            ],
            "{row}: original projection"
        );
        let pieces = native_terrain_instances(
            &sprite,
            &sprite,
            [
                point.0,
                point.1
                    - if row.get("animation").is_some() {
                        16.0
                    } else {
                        0.0
                    },
            ],
            lift,
            0.3,
            [1.0; 3],
            Default::default(),
        );
        let dirty_translation =
            row["viewport_y"].as_i64().unwrap() - row["dirty_y"].as_i64().unwrap();
        for (piece, draw) in pieces.iter().zip(draws) {
            // Native rebases its draw point into the dirty destination rect.
            // The production world builder leaves that translation to camera
            // and clipping, so compare in the common world-pixel frame.
            assert_eq!(
                piece.position,
                [
                    draw["point"][0].as_i64().unwrap() as f32,
                    (draw["point"][1].as_i64().unwrap() - dirty_translation) as f32
                        + crate::map::terrain::TILE_HEIGHT / 2.0,
                ],
                "{row}: shared body/shadow point"
            );
            assert_eq!(
                i64::from(piece.z_gradient),
                draw["gradient"].as_i64().unwrap(),
                "{row}: gradient"
            );
            assert_eq!(
                piece.z_adjust,
                draw["z_adjust"].as_i64().unwrap() as f32,
                "{row}: retained height and class Z"
            );
        }
    }
}
