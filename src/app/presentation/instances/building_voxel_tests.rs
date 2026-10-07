use super::*;
use crate::render::unit_atlas::{BarrelImagePitches, UnitAtlas, UnitSpriteEntry};
use std::collections::HashMap;

/// Exercise the production emitter against original 43DA80 submissions.
/// Synthetic entries mark which native part/frame/pitch the emitter picked;
/// this checks the real ordered draw output without a GPU or a second draw
/// implementation. Retail asset/raster coverage lives in unit_atlas_tests.
#[test]
fn building_voxel_parts_follow_native_frames_order_and_translation() {
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/building_barrel.json",
    ))
    .unwrap();
    for (case_index, case) in native["draw_cases"].as_array().unwrap().iter().enumerate() {
        // Normal retail startup has a VPL. Its absence bypasses the native
        // cache, but is not the asset-backed presentation route tested here.
        if case["vpl"] == false {
            continue;
        }
        let primary = case["primary_raw"].as_u64().unwrap() as u16;
        let elevation = case["barrel_raw"].as_u64().unwrap() as u16;
        let pitch = crate::render::vxl_raster::voxel_facing_step_u16(elevation) as i8 - 8;
        let draws = case["draws"].as_array().unwrap();
        let mut entries = HashMap::new();
        for draw in draws {
            let barrel = draw["part"] == "barrel";
            entries.insert(
                UnitSpriteKey {
                    type_id: "BUILDING".into(),
                    turret_index: 0,
                    facing: canonical_turret_facing(primary),
                    layer: if barrel {
                        VxlLayer::Barrel
                    } else {
                        VxlLayer::Turret
                    },
                    frame: draw["frame"].as_i64().unwrap() as u32,
                    slope_type: 0,
                    barrel_pitch: if barrel { pitch } else { 0 },
                },
                UnitSpriteEntry {
                    uv_origin: [0.0; 2],
                    uv_size: [1.0; 2],
                    pixel_size: [40.0, 32.0],
                    offset_x: 0.0,
                    offset_y: 0.0,
                    native_draw_bounds: Some([0, 0, 40, 32]),
                    page: usize::from(barrel),
                },
            );
        }
        let mut atlas = UnitAtlas::from_test_entries(entries);
        for (index, (present, layer)) in [
            ("turret_present", VxlLayer::Turret),
            ("barrel_present", VxlLayer::Barrel),
        ]
        .into_iter()
        .enumerate()
        {
            if case[present] == true {
                atlas.frame_counts.insert(
                    ("BUILDING".into(), layer, 0),
                    case["motion_counts"][index].as_u64().unwrap() as u32,
                );
            }
        }
        let recoil = std::array::from_fn(|index| case["recoil"][index].as_f64().unwrap() as f32);
        let active = case["recoil_states"]
            .as_array()
            .unwrap()
            .iter()
            .any(|state| state.as_i64().unwrap() != 0);
        let mut pieces = Vec::new();
        emit_building_turret_vxl(
            &atlas,
            &mut BarrelImagePitches::default(),
            "BUILDING",
            primary,
            elevation,
            case["turret_anim_frame"].as_i64().unwrap() as i32,
            (recoil, active),
            case["turret_offset"].as_i64().unwrap() as i32,
            100.0,
            200.0,
            0,
            0.5,
            [1.0; 3],
            crate::render::palette_light::PaletteLight::default(),
            DrawState::default(),
            3,
            -60,
            &mut pieces,
        );
        assert_eq!(pieces.len(), draws.len(), "case {case_index}: {case}");
        for (piece, draw) in pieces.iter().zip(draws) {
            assert_eq!(
                piece.target,
                ObjectTexture::UnitAtlasPage(usize::from(draw["part"] == "barrel")),
                "case {case_index}: native part order"
            );
            let native_x = f32::from_bits(draw["matrix_bits"][3].as_u64().unwrap() as u32);
            let native_y = f32::from_bits(draw["matrix_bits"][7].as_u64().unwrap() as u32);
            let expected = [103.0 + native_x, 143.0 - native_y];
            for axis in 0..2 {
                assert!(
                    (piece.instance.position[axis] - expected[axis]).abs() <= 0.0001,
                    "case {case_index}, {} axis{axis}: {:?} vs {expected:?}",
                    draw["part"],
                    piece.instance.position
                );
            }
            assert_eq!(
                piece.instance.depth, 0.5,
                "gun shares its building sort depth"
            );
            assert_eq!(piece.policy, BlitPolicy::z_read(SpriteEncoding::Voxel));
        }
    }
}
