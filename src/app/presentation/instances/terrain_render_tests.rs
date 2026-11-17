//! Native retained Terrain coordinates through static and animated DrawIt.
//! The corpus stops at the shape sink; this compares instance inputs, not pixels.

use super::{native_terrain_instance, terrain_draw_frame};
use crate::render::overlay_atlas::OverlaySpriteEntry;
use crate::render::terrain_draw::TerrainPiece;

/// Terrain DrawIt (0x71C1B0): a static type draws frame 1 below 2 health and
/// frame 0 otherwise; an animated type draws its stage whatever its health.
#[cfg(not(feature = "phobos"))]
#[test]
fn terrain_draw_frame_follows_health_and_stage() {
    assert_eq!(terrain_draw_frame(None, 2), 0);
    assert_eq!(terrain_draw_frame(None, 1), 1);
    assert_eq!(terrain_draw_frame(Some(7), 1), 7);
    // TerrainAI71C7DD and DrawIt71C208 retain the full Stage. A 514-frame
    // animated spawner reaches256 before its midpoint257 resets the stage.
    assert_eq!(terrain_draw_frame(Some(256), 1), 256);
}

/// Phobos `TerrainClass_Draw_PickFrame` (0x71C1FE): the type's
/// `HasDamagedFrames`/`HasCrumblingFrames`/`AnimationLength` select the
/// frame; an unset key set draws the static frame 0 whatever the health.
#[cfg(feature = "phobos")]
#[test]
fn phobos_terrain_draw_frame_follows_type_keys_and_crumbling() {
    // (stage, health, strength, has_damaged, has_crumbling, crumbling, anim_len, frames, animated)
    let frame =
        |stage, health, strength, damaged, crumbling_frames, crumbling, anim, frames, animated| {
            terrain_draw_frame(
                stage,
                health,
                strength,
                0.5,
                damaged,
                crumbling_frames,
                crumbling,
                anim,
                frames,
                animated,
            )
        };
    assert_eq!(frame(None, 1, 100, false, false, false, None, 0, false), 0);
    assert_eq!(frame(None, 1, 100, true, false, false, None, 0, false), 1);
    assert_eq!(frame(None, 60, 100, true, false, false, None, 0, false), 0);
    // Animated: stage + damaged * AnimationLength; a crumbling tree moves to
    // the next half plus 1.
    assert_eq!(
        frame(Some(3), 1, 100, true, false, false, Some(4), 8, true),
        7
    );
    assert_eq!(
        frame(Some(3), 1, 100, true, true, true, Some(4), 8, true),
        12
    );
    assert_eq!(
        frame(Some(2), 1, 100, true, true, true, Some(4), 8, false),
        3
    );
    assert_eq!(
        frame(Some(256), 1, 100, false, false, false, None, 0, true),
        256
    );
}

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
        draw_point_offset: [0; 2],
        extended: false,
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
        let draw_point = [
            point.0,
            point.1
                - if row.get("animation").is_some() {
                    16.0
                } else {
                    0.0
                },
        ];
        let pieces = [TerrainPiece::Body, TerrainPiece::Shadow].map(|piece| {
            native_terrain_instance(
                &sprite,
                piece,
                draw_point,
                lift,
                0.3,
                [1.0; 3],
                Default::default(),
            )
        });
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

/// Phobos `ConditionYellow.Terrain` (`Ext/Rules/Body.cpp:147`,
/// `Ext/TerrainType/Hooks.cpp:106,274`): the damaged-frame threshold falls
/// back to `[AudioVisual] ConditionYellow`, and a damaged-frame tree switches
/// exactly at the authored ratio.
#[cfg(feature = "phobos")]
#[test]
fn phobos_terrain_condition_yellow_reads_and_switches_the_damaged_frame() {
    use super::terrain_condition_yellow;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[AudioVisual]\nConditionYellow=0.5\nConditionYellow.Terrain=0.1\n",
    ))
    .unwrap();
    assert_eq!(
        terrain_condition_yellow(&rules),
        f64::from(0.1f32),
        "the Phobos Parser<double> widening"
    );
    let fallback =
        RuleSet::from_ini(&IniFile::from_str("[AudioVisual]\nConditionYellow=0.4\n")).unwrap();
    assert_eq!(
        terrain_condition_yellow(&fallback),
        f64::from(0.4f32),
        "the shared ConditionYellow remains binary32-widened"
    );
    assert_eq!(
        terrain_condition_yellow(&rules),
        rules.terrain_condition_yellow(),
        "the draw reads the rules owner the 0x71B98B damage latch also asks"
    );

    let frame = |rules: &RuleSet, health| {
        terrain_draw_frame(
            None,
            health,
            100,
            terrain_condition_yellow(rules),
            true,
            false,
            false,
            None,
            0,
            false,
        )
    };
    assert_eq!(
        frame(&rules, 10),
        1,
        "at the authored ratio the frame flips"
    );
    assert_eq!(frame(&rules, 11), 0);
    assert_eq!(
        frame(&fallback, 40),
        1,
        "the shared ConditionYellow fallback"
    );
    assert_eq!(frame(&fallback, 41), 0);
}
