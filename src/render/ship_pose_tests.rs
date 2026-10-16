//! Ship69F670 numeric controls and physical model/pose-page integration.
//! Native corpus assertions are added with the original harness output;
//! ordinary per-part integration is independent of a final-frame golden.
use super::*;
use crate::render::unit_atlas::{PoseTilt, UnitModel, UnitSpriteKey, VxlLayer};

#[test]
fn physical_destroyer_pose_uses_loaded_hull_and_omits_unregistered_guns() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let rules = &battle.rules;
    let model = UnitModel::load(&assets, "DEST", Some(rules)).expect("physical stock DEST model");
    let vpl = VplFile::from_bytes(assets.get_ref("VOXELS.VPL").unwrap()).unwrap();
    let pose = GroundPose {
        angles: [0.5, 0.],
        body_facing: 64,
        slope_blend: None,
        turret_offset: rules.art().get("DEST").map_or(0, |art| art.turret_offset),
    };
    let key = UnitSpriteKey {
        type_id: "DEST".into(),
        turret_index: 0,
        facing: pose.body_facing,
        layer: VxlLayer::Body,
        frame: 0,
        slope_type: 0,
        barrel_pitch: 0,
    };
    let (body, body_bounds) = model
        .render_pose(&key, Some(&vpl), PoseTilt::Ground(pose))
        .unwrap();
    assert!(body.palette_indices.iter().any(|&index| index != 0));
    assert!(body_bounds.is_some());
    let mut cache = crate::render::unit_pose_cache::VxlPoseFrameCache::default();
    let entry = cache
        .render(&assets, Some(rules), &key, PoseTilt::Ground(pose))
        .unwrap();
    assert_eq!(entry.pixel_size, [body.width as f32, body.height as f32]);
    assert_eq!(
        [entry.offset_x, entry.offset_y],
        [body.offset_x, body.offset_y]
    );
    assert_eq!(entry.native_draw_bounds, body_bounds);
    assert!(
        !rules.object("DEST").unwrap().has_turret,
        "actual RULESMD Turret=no"
    );
    for layer in [VxlLayer::Turret, VxlLayer::Barrel] {
        let key = UnitSpriteKey {
            layer,
            ..key.clone()
        };
        assert!(
            model
                .render_pose(&key, Some(&vpl), PoseTilt::Ground(pose))
                .is_none(),
            "no invented stock DEST gun assets"
        );
        assert!(
            cache
                .render(&assets, Some(rules), &key, PoseTilt::Ground(pose))
                .is_none()
        );
    }
    let level = GroundPose {
        angles: [0.; 2],
        ..pose
    };
    let (unposed, _) = model
        .render_pose(&key, Some(&vpl), PoseTilt::Ground(level))
        .unwrap();
    assert_ne!(
        (body.width, body.height, body.palette_indices),
        (unposed.width, unposed.height, unposed.palette_indices),
        "completed grapple roll must affect actual rasterized hull"
    );
}

/// Existing original Unit73BA4C..73BC26 controls already execute the entire
/// uncached gun arm. The new ship consumer must call that same numeric owner
/// for a flat unrolled hull, including relative facing and translated pitch.
#[test]
fn ship_parts_reuse_native_unit_uncached_gun_arm_controls() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/barrel_pitch.json",
    ))
    .unwrap();
    let bits = |matrix: Mat4| {
        (0..3)
            .flat_map(|row| (0..4).map(move |col| matrix.col(col)[row].to_bits()))
            .collect::<Vec<_>>()
    };
    for row in corpus["arm_cases"].as_array().unwrap() {
        let body_step = row["body_step"].as_u64().unwrap() as u8;
        let turret_step = row["turret_step"].as_u64().unwrap() as u8;
        let raw_pitch = row["barrel"].as_u64().unwrap() as u16;
        let pose = GroundPose {
            angles: [0.; 2],
            body_facing: body_step * 8,
            slope_blend: None,
            turret_offset: row["turret_offset"].as_i64().unwrap() as i32,
        };
        let matrices = ground_unit_pose_matrices(
            0,
            pose,
            [0.; 2],
            turret_step * 8,
            voxel_facing_step_u16(raw_pitch) as i8 - 8,
        );
        for (matrix, field) in
            matrices
                .into_iter()
                .zip(["body_draw_bits", "turret_draw_bits", "barrel_draw_bits"])
        {
            assert_eq!(
                serde_json::json!(bits(matrix)),
                row[field],
                "body{body_step} turret{turret_step} pitch{raw_pitch:x} offset{} {field}",
                pose.turret_offset
            );
        }
    }
}

pub(crate) fn native_ship_corpus() -> serde_json::Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/naval_draw_squid_tilt.json",
    ))
    .unwrap()
}

pub(crate) fn native_ground_pose(input: &serde_json::Value) -> GroundPose {
    let timer = &input["slope_timer"];
    let total = timer["total"].as_i64().unwrap() as i32;
    let remaining = crate::sim::timer::CdTimer::from_raw(
        timer["start"].as_i64().unwrap() as i32,
        timer["duration"].as_i64().unwrap() as i32,
    )
    .remaining(input["frame"].as_i64().unwrap() as i32);
    GroundPose {
        angles: std::array::from_fn(|i| {
            f32::from_bits(input["angle_bits"][i].as_u64().unwrap() as u32)
        }),
        body_facing: voxel_facing_step_u16(input["facing"].as_u64().unwrap() as u16) * 8,
        slope_blend: (total != 0 && remaining > 0).then(|| VxlSlopeBlend {
            from_slope: input["previous_slope"].as_u64().unwrap() as u8,
            to_slope: input["slope"].as_u64().unwrap() as u8,
            phase_num: total.wrapping_sub(remaining),
            phase_den: total as u8,
        }),
        turret_offset: 0,
    }
}

#[test]
fn original_ship_matrices_loaded_half_sizes_and_physical_hull_rasters_match() {
    assert_native_ground_matrices_and_parts(native_ship_corpus(), "DEST", "ship");
}

pub(crate) fn native_drive_corpus() -> serde_json::Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/drive_draw_drone_tilt.json",
    ))
    .unwrap()
}

#[test]
fn original_drive_matrices_and_physical_hull_turret_barrel_rasters_match() {
    assert_native_ground_matrices_and_parts(native_drive_corpus(), "MTNK", "drive");
}

/// Reconstruct only the declared component input. Native owns expected bits,
/// model postpass and every retained raster pixel.
pub(crate) fn native_ground_part(
    type_id: &str,
    row: &serde_json::Value,
    layer: VxlLayer,
) -> (GroundPose, UnitSpriteKey) {
    let mut pose = native_ground_pose(&row["input"]);
    let mut facing = pose.body_facing;
    let mut barrel_pitch = 0;
    if layer != VxlLayer::Body {
        let gun = &row["gun_component"]["input"];
        pose.turret_offset = gun["turret_offset"].as_i64().unwrap() as i32;
        facing = gun["turret_step"].as_u64().unwrap() as u8 * 8;
        if layer == VxlLayer::Barrel {
            barrel_pitch =
                voxel_facing_step_u16(gun["barrel_raw"].as_u64().unwrap() as u16) as i8 - 8;
        }
    }
    (
        pose,
        UnitSpriteKey {
            type_id: type_id.into(),
            turret_index: 0,
            facing,
            layer,
            frame: 0,
            slope_type: row["input"]["slope"].as_u64().unwrap() as u8,
            barrel_pitch,
        },
    )
}

fn assert_native_ground_matrices_and_parts(
    corpus: serde_json::Value,
    type_id: &str,
    locomotor: &str,
) {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    for (name, file) in corpus["files"].as_object().unwrap() {
        let data = assets.get_ref(name).unwrap();
        assert_eq!(data.len() as u64, file["bytes"]);
        assert_eq!(crate::util::sha256::sha256_hex(data), file["sha256"]);
    }
    let model = UnitModel::load(&assets, type_id, Some(&battle.rules)).unwrap();
    let image = crate::rules::voxel_frame_catalog::voxel_image_id(type_id, Some(&battle.rules));
    let vxl = VxlFile::from_bytes(assets.get_ref(&format!("{image}.VXL")).unwrap()).unwrap();
    let vpl = VplFile::from_bytes(assets.get_ref("VOXELS.VPL").unwrap()).unwrap();
    let bits = |matrix: Mat4| {
        (0..3)
            .flat_map(|r| (0..4).map(move |c| matrix.col(c)[r].to_bits()))
            .collect::<Vec<_>>()
    };
    let rows = corpus["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let pose = native_ground_pose(input);
        let slope = input["slope"].as_u64().unwrap() as u8;
        assert_eq!(row[locomotor]["is_voxel"], true);
        let half_sizes = std::array::from_fn(|i| {
            f64::from_bits(
                u64::from_str_radix(row[locomotor]["half_size_bits"][i].as_str().unwrap(), 16)
                    .unwrap(),
            ) as f32
        });
        assert_eq!(
            voxel_tilt_half_sizes(&vxl).unwrap(),
            half_sizes,
            "{name}: native loaded-section postpass"
        );
        assert_eq!(
            serde_json::json!(bits(ground_locomotor_matrix(
                slope,
                pose.slope_blend,
                voxel_facing_step(pose.body_facing),
                pose.angles,
                half_sizes
            ))),
            row[locomotor]["matrix_bits"],
            "{name}: full original locomotor matrix"
        );
        assert_eq!(
            row[locomotor]["key_out"] == u32::MAX,
            pose.slope_blend.is_some() || voxel_tilt_applies(pose.angles),
            "{name}: native cached/dynamic gate"
        );
        assert_eq!(
            serde_json::json!(bits(
                ground_unit_pose_matrices(slope, pose, half_sizes, pose.body_facing, 0)[0]
            )),
            row["body_draw_bits"],
            "{name}: native camera product"
        );
        if let Some(gun) = row.get("gun_component") {
            let control = &gun["input"];
            let pose = GroundPose {
                turret_offset: control["turret_offset"].as_i64().unwrap() as i32,
                ..pose
            };
            let matrices = ground_unit_pose_matrices(
                slope,
                pose,
                half_sizes,
                control["turret_step"].as_u64().unwrap() as u8 * 8,
                voxel_facing_step_u16(control["barrel_raw"].as_u64().unwrap() as u16) as i8 - 8,
            );
            assert_eq!(
                serde_json::json!(bits(matrices[1])),
                gun["turret_draw_bits"],
                "{name}: supplied gun-component turret"
            );
            assert_eq!(
                serde_json::json!(bits(matrices[2])),
                gun["barrel_draw_bits"],
                "{name}: supplied gun-component barrel"
            );
        }
        for (part_name, layer) in [
            ("body", VxlLayer::Body),
            ("turret", VxlLayer::Turret),
            ("barrel", VxlLayer::Barrel),
        ] {
            let Some(raster) = row
                .get("rasters")
                .and_then(|rasters| rasters.get(part_name))
            else {
                continue;
            };
            let (pose, key) = native_ground_part(type_id, row, layer);
            let (sprite, bounds) = model
                .render_pose(&key, Some(&vpl), PoseTilt::Ground(pose))
                .unwrap();
            let rect: [i32; 6] =
                std::array::from_fn(|i| raster["rect"][i].as_i64().unwrap() as i32);
            assert_eq!(
                bounds,
                Some([rect[0], rect[1], rect[4], rect[5]]),
                "{name}: native destination bounds"
            );
            let mut actual = vec![0; 65536];
            let left = sprite.offset_x as i32 + rect[2] - rect[0];
            let top = sprite.offset_y as i32 + rect[3] - rect[1];
            for y in 0..sprite.height as i32 {
                for x in 0..sprite.width as i32 {
                    let value =
                        sprite.palette_indices[(y as u32 * sprite.width + x as u32) as usize];
                    if value != 0 {
                        actual[((top + y) * 256 + left + x) as usize] = value;
                    }
                }
            }
            let mut expected = vec![0; 65536];
            for pixel in raster["pixels"].as_array().unwrap() {
                expected[pixel[0].as_u64().unwrap() as usize] = pixel[1].as_u64().unwrap() as u8;
            }
            assert_eq!(
                actual, expected,
                "{type_id} {name}/{part_name}: all native physical part visibility bytes"
            );
        }
    }
}
