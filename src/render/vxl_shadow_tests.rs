use super::*;
use serde_json::Value;

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|v| u8::from_str_radix(std::str::from_utf8(v).unwrap(), 16).unwrap())
        .collect()
}

fn check(sprite: &VxlSprite, case: &Value, label: &str) {
    let rect: Vec<i32> = case["rect"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap() as i32)
        .collect();
    assert_eq!(
        [
            sprite.offset_x as i32,
            sprite.offset_y as i32,
            sprite.width as i32,
            sprite.height as i32
        ],
        [rect[0], rect[1], rect[4], rect[5]],
        "{label} crop/placement"
    );
    let mut full = vec![0; 65536];
    for p in case["pixels"].as_array().unwrap() {
        full[p[0].as_u64().unwrap() as usize] = p[1].as_u64().unwrap() as u8;
    }
    let mut expected = Vec::new();
    for y in rect[3]..rect[3] + rect[5] {
        let p = (y * 256 + rect[2]) as usize;
        expected.extend_from_slice(&full[p..p + rect[4] as usize]);
    }
    let errors = sprite
        .palette_indices
        .iter()
        .zip(&expected)
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(errors, 0, "{label} complete cropped stencil");
}

#[test]
fn native_generated_shadow_geometry_and_body_mask() {
    let fixture: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/shadow_fixtures.json",
    ))
    .unwrap();
    let vxl = VxlFile::from_bytes(&bytes(fixture["vxl"].as_str().unwrap())).unwrap();
    let hva = HvaFile::from_bytes(&bytes(fixture["hva"].as_str().unwrap())).unwrap();
    let body = bytes(fixture["body"].as_str().unwrap());
    for case in fixture["cases"].as_array().unwrap() {
        let step = case["step"].as_u64().unwrap() as u8;
        let mut sprite = render(
            &vxl,
            Some(&hva),
            &VxlRenderParams {
                facing: step * 8,
                ..Default::default()
            },
            0,
            false,
        )
        .unwrap();
        if case["masked"].as_bool().unwrap() {
            mask_body(
                &mut sprite.palette_indices,
                sprite.width,
                [sprite.offset_x as i32, sprite.offset_y as i32],
                &body,
                [0, 0],
            )
            .unwrap();
        }
        check(
            &sprite,
            case,
            &format!("synthetic step{step} masked{}", case["masked"]),
        );
    }
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 64);
    assert!(
        render(
            &vxl,
            Some(&hva),
            &VxlRenderParams {
                frame: 1,
                ..Default::default()
            },
            0,
            false,
        )
        .is_none()
    );
    assert!(
        render(
            &vxl,
            Some(&hva),
            &VxlRenderParams {
                slope_type: 17,
                ..Default::default()
            },
            0,
            false,
        )
        .is_none()
    );
}

fn aircraft_params(caller: &Value) -> VxlRenderParams {
    let input = &caller["input"];
    let fly = input["locomotor"] == "Fly";
    let facing = input[if fly { "secondary" } else { "primary" }]
        .as_u64()
        .unwrap() as u16;
    VxlRenderParams {
        facing: super::super::voxel_facing_step_u16(facing) * 8,
        slope_type: if fly && input["dropship"].as_bool().unwrap_or(false) {
            0
        } else {
            input["slope"].as_u64().unwrap_or(0) as u8
        },
        ..Default::default()
    }
}

#[test]
fn native_aircraft_shadow_half_viewer_selected_section_and_slope() {
    let fixture: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/aircraft_shadow.json",
    ))
    .unwrap();
    let data = &fixture["generated"];
    let vxl = VxlFile::from_bytes(&bytes(data["vxl"].as_str().unwrap())).unwrap();
    let hva = HvaFile::from_bytes(&bytes(data["hva"].as_str().unwrap())).unwrap();
    for case in data["cases"].as_array().unwrap() {
        let caller = &fixture["callers"][case["caller"].as_u64().unwrap() as usize];
        let sprite = render(
            &vxl,
            Some(&hva),
            &aircraft_params(caller),
            case["shadow_index"].as_i64().unwrap() as i32,
            case["considered_aircraft"].as_bool().unwrap(),
        )
        .unwrap();
        check(&sprite, case, &format!("aircraft generated {case}"));
    }
    assert!(render(&vxl, Some(&hva), &Default::default(), -1, true).is_none());
    assert!(render(&vxl, Some(&hva), &Default::default(), 4, true).is_none());
}

fn unit_shadow_params(caller: &Value) -> VxlRenderParams {
    VxlRenderParams {
        facing: super::super::voxel_facing_step_u16(
            caller["input"]["primary"].as_u64().unwrap() as u16
        ) * 8,
        slope_type: caller["input"]["slope"].as_u64().unwrap_or(0) as u8,
        ..Default::default()
    }
}

#[test]
fn native_jumpjet_unit_shadow_low_flight_mask_offsets() {
    let fixture = crate::test_fixture::json("tools/voxel_oracle/unit_shadow.json");
    let data = &fixture["generated"];
    let vxl = VxlFile::from_bytes(&bytes(data["vxl"].as_str().unwrap())).unwrap();
    let hva = HvaFile::from_bytes(&bytes(data["hva"].as_str().unwrap())).unwrap();
    let body = bytes(data["body"].as_str().unwrap());
    for case in data["cases"].as_array().unwrap() {
        let caller = &fixture["callers"][case["caller"].as_u64().unwrap() as usize];
        let mut sprite = render(&vxl, Some(&hva), &unit_shadow_params(caller), 0, true).unwrap();
        if caller["body_mask_present"].as_bool().unwrap() {
            let fractional_center =
                center(&vxl, Some(&hva), &unit_shadow_params(caller), 0, true).unwrap();
            let sample_offset = body_sample_offset(
                fractional_center,
                std::array::from_fn(|i| {
                    caller["renderer"]["mask_offset"][i].as_i64().unwrap() as i32
                }),
            )
            .unwrap();
            assert_eq!(
                std::array::from_fn::<_, 2, _>(|i| ftol(load(fractional_center[i]).unwrap())
                    .unwrap()
                    + sample_offset[i]),
                std::array::from_fn(|i| case["quad_origin_after"][i].as_i64().unwrap() as i32),
                "native final body-sample origin"
            );
            mask_body(
                &mut sprite.palette_indices,
                sprite.width,
                [sprite.offset_x as i32, sprite.offset_y as i32],
                &body,
                sample_offset,
            )
            .unwrap();
        }
        check(
            &sprite,
            case,
            &format!("Unit native caller {}", case["caller"]),
        );
    }
    assert_eq!(data["cases"].as_array().unwrap().len(), 7);
}

#[test]
fn retail_kirov_shadow_matches_original_unit_caller_and_stencil() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let fixture = crate::test_fixture::json("tools/voxel_oracle/unit_shadow.json");
    let data = &fixture["retail"];
    for name in ["ZEP.VXL", "ZEP.HVA", "VOXELS.VPL"] {
        assert_eq!(
            crate::util::sha256::sha256_hex(assets.get_ref(name).unwrap()),
            data["sha256"][name].as_str().unwrap(),
            "{name}"
        );
    }
    let vxl = VxlFile::from_bytes(assets.get_ref("ZEP.VXL").unwrap()).unwrap();
    let hva = HvaFile::from_bytes(assets.get_ref("ZEP.HVA").unwrap()).unwrap();
    for case in data["cases"].as_array().unwrap() {
        let caller = &fixture["callers"][case["caller"].as_u64().unwrap() as usize];
        let sprite = render(&vxl, Some(&hva), &unit_shadow_params(caller), 0, true).unwrap();
        check(
            &sprite,
            case,
            &format!("retail Unit ZEP caller {}", case["caller"]),
        );
    }
    assert_eq!(data["cases"].as_array().unwrap().len(), 4);
}

#[test]
fn retail_harrier_shadow_matches_original_caller_and_stencil() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let fixture: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/aircraft_shadow.json",
    ))
    .unwrap();
    let stock = &fixture["retail"];
    for name in ["FALC.VXL", "FALC.HVA", "VOXELS.VPL"] {
        let raw = assets.get_ref(name).expect(name);
        assert_eq!(
            crate::util::sha256::sha256_hex(raw),
            stock["sha256"][name].as_str().unwrap(),
            "{name}"
        );
    }
    let vxl = VxlFile::from_bytes(assets.get_ref("FALC.VXL").unwrap()).unwrap();
    let hva = HvaFile::from_bytes(assets.get_ref("FALC.HVA").unwrap()).unwrap();
    for case in stock["cases"].as_array().unwrap() {
        let caller = &fixture["callers"][case["caller"].as_u64().unwrap() as usize];
        let sprite = render(&vxl, Some(&hva), &aircraft_params(caller), 0, true).unwrap();
        check(
            &sprite,
            case,
            &format!("retail ORCA caller {}", case["caller"]),
        );
    }
}

/// `VoxelAnimClass::DrawIt`'s shadow (`0x00749CBE..0x00749CE8`): the bounce
/// drawing matrix itself, the identity viewer, no HVA and no mask.
#[test]
fn tumbling_piece_shadow_matches_original_execution() {
    let fixture: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/tumble_fixtures.json",
    ))
    .unwrap();
    let vxl = VxlFile::from_bytes(&bytes(fixture["vxl"].as_str().unwrap())).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    for case in cases {
        let shadow = &case["shadow"];
        let bits: Vec<u32> = shadow["input_matrix_bits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|word| word.as_u64().unwrap() as u32)
            .collect();
        let matrix =
            super::super::hva_to_mat4(&std::array::from_fn(|i| f32::from_bits(bits[i])), 1.0);
        let sprite = render_matrix(&vxl, matrix, 0, false).unwrap();
        check(
            &sprite,
            shadow,
            &format!("flight {} tick {}", case["flight"], case["tick"]),
        );
    }
    assert_eq!(cases.len(), 24);
}

#[test]
#[ignore = "requires original stock shadow evidence and extracted art in VERA20K_SHADOW_PROBE_DIR"]
fn stock_shadow_geometry_and_native_composed_body_masks() {
    let root = std::path::PathBuf::from(
        std::env::var_os("VERA20K_SHADOW_PROBE_DIR").expect("set rendering-parity root"),
    );
    let mut compared = 0;
    for (file, masked) in [
        ("shadow-geometry-native.json", false),
        ("shadow-mask-geometry-native.json", true),
    ] {
        let fixture: Value = serde_json::from_slice(
            &std::fs::read(root.join("voxel-shadow-proof").join(file)).unwrap(),
        )
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let model = case["model"].as_str().unwrap();
            let step = case["facing_step"].as_u64().unwrap() as u8;
            let (folder, prefix, ve, he) = if model == "GTNK" {
                ("grizzly-raster-proof/extract", "gtnk", "vxl", "hva")
            } else {
                ("grizzly-part-composition/extract", "HTNK", "VXL", "HVA")
            };
            let vxl = VxlFile::from_bytes(
                &std::fs::read(root.join(folder).join(format!("{prefix}.{ve}"))).unwrap(),
            )
            .unwrap();
            let hva = HvaFile::from_bytes(
                &std::fs::read(root.join(folder).join(format!("{prefix}.{he}"))).unwrap(),
            )
            .unwrap();
            let mut sprite = render(
                &vxl,
                Some(&hva),
                &VxlRenderParams {
                    facing: step * 8,
                    ..Default::default()
                },
                0,
                false,
            )
            .unwrap();
            if masked {
                let folder = if model == "GTNK" {
                    "aligned-native"
                } else {
                    "rhino-neutral-native"
                };
                let body = std::fs::read(
                    root.join("grizzly-part-composition")
                        .join(folder)
                        .join(format!("{step:02}.bin")),
                )
                .unwrap();
                mask_body(
                    &mut sprite.palette_indices,
                    sprite.width,
                    [sprite.offset_x as i32, sprite.offset_y as i32],
                    &body,
                    [0, 0],
                )
                .unwrap();
            }
            check(&sprite, case, &format!("{model} {step} masked{masked}"));
            compared += 1;
        }
    }
    assert_eq!(compared, 80);
    eprintln!("native stock shadow stencil/crop: 64 unmasked + 16 actual native body masks passed");
}
