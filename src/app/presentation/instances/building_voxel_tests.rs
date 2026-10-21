use super::*;
use crate::render::unit_atlas::{BarrelImagePitches, UnitAtlas, UnitSpriteEntry};
use std::collections::HashMap;

/// Exercise the production emitter against original 43DA80 submissions.
/// Synthetic entries mark which native part/frame/pitch the emitter picked;
/// this checks the real ordered draw output without a GPU or a second draw
/// implementation. The stock guns' rasters are checked below.
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
            0,
            DrawState::default(),
            3,
            -60,
            0,
            &mut pieces,
        );
        assert_eq!(pieces.len(), draws.len(), "case {case_index}: {case}");
        for (piece, draw) in pieces.iter().zip(draws) {
            assert_eq!(
                piece.target,
                ObjectTexture::UnitAtlasPage(usize::from(draw["part"] == "barrel")),
                "case {case_index}: native part order"
            );
            // The submitted point ([100,200] plus TurretAnimX/Y [3,-60]) and
            // the matrix translation; nothing else moves the gun.
            let native_x = f32::from_bits(draw["matrix_bits"][3].as_u64().unwrap() as u32);
            let native_y = f32::from_bits(draw["matrix_bits"][7].as_u64().unwrap() as u32);
            let point = &draw["point"];
            let expected = [
                point[0].as_i64().unwrap() as f32 + native_x,
                point[1].as_i64().unwrap() as f32 - native_y,
            ];
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

/// Every stock gun image, rasterized by the original Render core (754510) at
/// eight 43DA80 facings. A cache miss (706ED0) blits the crop at the
/// submitted point plus the rect origin; a hit (707480) draws the copy 6C89E0
/// cached from the same origin and rows. The production atlas raster must be
/// that crop, and the emitter must draw it at that origin.
#[test]
fn retail_building_guns_draw_the_native_crop_at_the_native_origin() {
    use crate::render::unit_atlas::UnitModel;
    use crate::rules::retail_ini_fixture::{retail_assets, retail_battle_rules};
    let Some(battle) = retail_battle_rules() else {
        return;
    };
    let (_, assets) = retail_assets().expect("the battle rules came from RA2_DIR");
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/building_gun_raster.json",
    ))
    .unwrap();
    for (name, asset) in native["assets"].as_object().unwrap() {
        let bytes = assets.get_ref(name).unwrap();
        assert_eq!(bytes.len() as u64, asset["bytes"], "{name} length");
        assert_eq!(
            crate::util::sha256::sha256_hex(bytes),
            asset["sha256"],
            "{name} hash"
        );
    }
    let rules = &battle.rules;
    let vpl = crate::assets::vpl_file::VplFile::from_bytes(assets.get_ref("VOXELS.VPL").unwrap())
        .unwrap();
    let layer = |part: &serde_json::Value| {
        if part == "barrel" {
            VxlLayer::Barrel
        } else {
            VxlLayer::Turret
        }
    };
    let draws = native["draws"].as_array().unwrap();
    let mut models = HashMap::new();
    for draw in draws {
        let building = draw["building"].as_str().unwrap();
        let object = rules.object(building).unwrap();
        // The corpus submits TurretOffset 0; stock ART sets none.
        assert_eq!(
            rules
                .art()
                .resolve_metadata_entry(building, &object.image)
                .map_or(0, |art| art.turret_offset),
            0,
            "{building}"
        );
        let model = models
            .entry(building)
            .or_insert_with(|| UnitModel::load(&assets, building, Some(rules)).unwrap());
        let primary = draw["primary_raw"].as_u64().unwrap() as u16;
        let elevation = draw["barrel_raw"].as_u64().unwrap() as u16;
        let key = UnitSpriteKey {
            type_id: building.into(),
            turret_index: 0,
            facing: canonical_turret_facing(primary),
            layer: layer(&draw["part"]),
            frame: 0,
            slope_type: 0,
            barrel_pitch: if draw["part"] == "barrel" {
                crate::render::vxl_raster::voxel_facing_step_u16(elevation) as i8 - 8
            } else {
                0
            },
        };
        let label = format!(
            "{building} {} {primary:#06x}/{elevation:#06x}",
            draw["part"]
        );
        let (sprite, _) = model.render(&key, Some(&vpl), None, &mut None).unwrap();
        let rect: Vec<i64> = draw["rect"]
            .as_array()
            .unwrap()
            .iter()
            .map(|word| word.as_i64().unwrap())
            .collect();
        assert_eq!(
            [
                sprite.offset_x as i64,
                sprite.offset_y as i64,
                i64::from(sprite.width),
                i64::from(sprite.height)
            ],
            [rect[0], rect[1], rect[4], rect[5]],
            "{label}: rect"
        );
        assert_eq!(
            crate::util::sha256::sha256_hex(&sprite.palette_indices),
            draw["crop_sha256"],
            "{label}: crop"
        );

        let mut atlas = UnitAtlas::from_test_entries(HashMap::from([(
            key.clone(),
            UnitSpriteEntry {
                uv_origin: [0.0; 2],
                uv_size: [1.0; 2],
                pixel_size: [sprite.width as f32, sprite.height as f32],
                offset_x: sprite.offset_x,
                offset_y: sprite.offset_y,
                native_draw_bounds: Some([rect[0], rect[1], rect[4], rect[5]].map(|v| v as i32)),
                page: 0,
            },
        )]));
        // The building's loaded parts decide its arm; only this one is drawn.
        for row in draws.iter().filter(|row| row["building"] == building) {
            atlas
                .frame_counts
                .insert((building.into(), layer(&row["part"]), 0), 1);
        }
        let mut pieces = Vec::new();
        emit_building_turret_vxl(
            &atlas,
            &mut BarrelImagePitches::default(),
            building,
            primary,
            elevation,
            0,
            ([0.0; 2], false),
            0,
            100.0,
            200.0,
            0,
            0.5,
            [1.0; 3],
            crate::render::palette_light::PaletteLight::default(),
            0,
            DrawState::default(),
            3,
            -60,
            0,
            &mut pieces,
        );
        let point = &draw["point"];
        assert_eq!(pieces.len(), 1, "{label}");
        assert_eq!(
            pieces[0].instance.position,
            [
                (point[0].as_i64().unwrap() + rect[0]) as f32,
                (point[1].as_i64().unwrap() + rect[1]) as f32
            ],
            "{label}: blit origin"
        );
    }
    assert_eq!(draws.len(), 80);
}

/// Building vtable `0x7E3EBC+0x2EC` (`0x43E900`) returns
/// `TurretAnimZAdjust - AdjustForZ` for a voxel turret, and the voxel blits pass
/// it as their Z seed. DrawBody's SHP leaves never call that slot, so the term
/// belongs to the guns alone; without it a gun loses the depth test to the base
/// it stands on.
#[test]
fn building_voxel_guns_seed_their_blit_z_with_turret_anim_z_adjust() {
    let mut entries = HashMap::new();
    entries.insert(
        UnitSpriteKey {
            type_id: "BUILDING".into(),
            turret_index: 0,
            facing: canonical_turret_facing(0),
            layer: VxlLayer::Turret,
            frame: 0,
            slope_type: 0,
            barrel_pitch: 0,
        },
        UnitSpriteEntry {
            uv_origin: [0.0; 2],
            uv_size: [1.0; 2],
            pixel_size: [40.0, 32.0],
            offset_x: 0.0,
            offset_y: 0.0,
            native_draw_bounds: Some([0, 0, 40, 32]),
            page: 0,
        },
    );
    let mut atlas = UnitAtlas::from_test_entries(entries);
    atlas
        .frame_counts
        .insert(("BUILDING".into(), VxlLayer::Turret, 0), 1);
    for (anim_z_adjust, lift_px) in [(0, 0), (-60, 0), (-60, 7), (0, 7)] {
        let mut pieces = Vec::new();
        emit_building_turret_vxl(
            &atlas,
            &mut BarrelImagePitches::default(),
            "BUILDING",
            0,
            0x4000,
            0,
            ([0.0; 2], false),
            0,
            100.0,
            200.0,
            lift_px,
            0.5,
            [1.0; 3],
            crate::render::palette_light::PaletteLight::default(),
            0,
            DrawState::default(),
            3,
            28,
            anim_z_adjust,
            &mut pieces,
        );
        assert_eq!(pieces.len(), 1, "z_adjust {anim_z_adjust} lift {lift_px}");
        assert_eq!(
            pieces[0].instance.z_adjust,
            (anim_z_adjust - lift_px) as f32,
            "z_adjust {anim_z_adjust} lift {lift_px}"
        );
        assert_eq!(
            pieces[0].instance.depth, 0.5,
            "the adjustment is a blit Z term, not a sort bias"
        );
    }
}

/// The retail types the adjustment reaches, read through the production rules
/// owner rather than restated here. `TESLA`/`ATESLA` set the key without a voxel
/// turret, so `0x43E900`'s `TurretAnimIsVoxel` gate leaves their value unused.
#[test]
fn retail_voxel_turret_buildings_supply_the_z_adjustment_the_blit_seeds_from() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    for (type_id, z_adjust) in [
        ("NASAM", -20),
        ("NALASR", -20),
        ("NAFLAK", -40),
        ("YAGGUN", -40),
        ("YAREFN", -50),
        ("GTGCAN", -60),
        ("CAOUTP", -140),
        ("CAEAST02", -150),
    ] {
        let object = rules.object(type_id).expect("retail building type");
        assert!(
            object.turret_anim_is_voxel,
            "{type_id} draws its turret as a VXL"
        );
        assert_eq!(object.turret_anim_z_adjust, z_adjust, "{type_id}");
    }
    for type_id in ["TESLA", "ATESLA"] {
        let object = rules.object(type_id).expect("retail building type");
        assert!(!object.turret_anim_is_voxel, "{type_id}");
        assert_eq!(object.turret_anim_z_adjust, -100, "{type_id}");
    }
}
