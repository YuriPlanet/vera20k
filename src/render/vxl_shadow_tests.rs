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

/// Phobos `ShadowIndices=`'s loop (`Ext/TechnoType/Hooks.MatrixOp.cpp:793-800`)
/// draws one stencil per listed section; a list naming only the type's own
/// `ShadowIndex` must answer exactly that single-section stencil, and an
/// index the voxel has no section for draws nothing. The shared anchor is the
/// primary section's frame-0 center: native builds `shadowCenter` before
/// `ChooseFrame` (`:726-727`), so the path takes no single-value frame.
#[test]
fn shadow_indices_compose_around_the_primary_sections_center() {
    let fixture: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/shadow_fixtures.json",
    ))
    .unwrap();
    let vxl = VxlFile::from_bytes(&bytes(fixture["vxl"].as_str().unwrap())).unwrap();
    let hva = HvaFile::from_bytes(&bytes(fixture["hva"].as_str().unwrap())).unwrap();
    assert_eq!(vxl.limbs.len(), 1, "the oracle fixture has one section");
    for step in [0u8, 7, 15] {
        let params = VxlRenderParams {
            facing: step * 8,
            ..Default::default()
        };
        let single = render(&vxl, Some(&hva), &params, 0, 0, false).unwrap();
        let listed = render_indices(&vxl, Some(&hva), &params, 0, &[0], &[], false).unwrap();
        // Coverage is what the surrounding surface sees: every stencil pixel
        // at the same tactical position, whatever crop the two paths picked.
        let coverage = |sprite: &VxlSprite| {
            let mut set = std::collections::BTreeSet::new();
            for y in 0..sprite.height as i32 {
                for x in 0..sprite.width as i32 {
                    if sprite.palette_indices[(y * sprite.width as i32 + x) as usize] != 0 {
                        set.insert((sprite.offset_x as i32 + x, sprite.offset_y as i32 + y));
                    }
                }
            }
            set
        };
        assert_eq!(coverage(&listed), coverage(&single), "step {step}");
        // A section the voxel does not have contributes no stencil.
        assert_eq!(
            coverage(&render_indices(&vxl, Some(&hva), &params, 0, &[0, 4], &[], false).unwrap()),
            coverage(&single),
            "unreachable section is skipped"
        );
        assert!(render_indices(&vxl, Some(&hva), &params, 0, &[4], &[], false).is_none());
        // A malformed primary index must not discard a drawable listed
        // section; the first listed section's frame-0 center stands in.
        assert_eq!(
            coverage(&render_indices(&vxl, Some(&hva), &params, 4, &[0], &[], false).unwrap()),
            coverage(&single),
            "a missing primary still draws the listed section (step {step})"
        );
    }
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
            0,
            case["considered_aircraft"].as_bool().unwrap(),
        )
        .unwrap();
        check(&sprite, case, &format!("aircraft generated {case}"));
    }
    assert!(render(&vxl, Some(&hva), &Default::default(), -1, 0, true).is_none());
    assert!(render(&vxl, Some(&hva), &Default::default(), 4, 0, true).is_none());
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
        let mut sprite = render(&vxl, Some(&hva), &unit_shadow_params(caller), 0, 0, true).unwrap();
        if caller["body_mask_present"].as_bool().unwrap() {
            let fractional_center =
                center(&vxl, Some(&hva), &unit_shadow_params(caller), 0, 0, true).unwrap();
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
        let sprite = render(&vxl, Some(&hva), &unit_shadow_params(caller), 0, 0, true).unwrap();
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
        let sprite = render(&vxl, Some(&hva), &aircraft_params(caller), 0, 0, true).unwrap();
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

/// `0x7072A1`'s `ChooseFrame` (`Ext/TechnoType/Hooks.MatrixOp.cpp:1074-1104`):
/// `ShadowIndex.Frame` selects the shadow section's HVA frame, modulo the
/// file's frame count. A cleared `-1` falls back to `WalkedFramesSoFar`;
/// the shadow atlas is keyed by facing and slope with no walked-frame
/// dimension, so this pass uses frame 0, which retail also reaches because the
/// constructor value 0 never takes the `-1` arm.
#[test]
fn shadow_index_frame_selects_the_sections_hva_frame() {
    let fixture: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/shadow_fixtures.json",
    ))
    .unwrap();
    let vxl = VxlFile::from_bytes(&bytes(fixture["vxl"].as_str().unwrap())).unwrap();
    let one = HvaFile::from_bytes(&bytes(fixture["hva"].as_str().unwrap())).unwrap();
    // The oracle HVA holds one frame; a second frame translates that section
    // so the selected frame moves the stencil's placement.
    let mut shifted = one.transforms[0];
    shifted[3] += 32.0;
    shifted[7] += 32.0;
    let hva = HvaFile {
        frame_count: 2,
        section_count: one.section_count,
        section_names: one.section_names.clone(),
        transforms: vec![one.transforms[0], shifted],
    };
    let params = VxlRenderParams {
        facing: 0,
        ..Default::default()
    };
    let frame0 = render(&vxl, Some(&hva), &params, 0, 0, false).unwrap();
    let frame1 = render(&vxl, Some(&hva), &params, 0, 1, false).unwrap();
    assert_ne!(
        [frame0.offset_x as i32, frame0.offset_y as i32],
        [frame1.offset_x as i32, frame1.offset_y as i32],
        "frame 1 draws the translated section"
    );
    // Frame indices wrap by the frame count.
    let wrapped = render(&vxl, Some(&hva), &params, 0, 2, false).unwrap();
    assert_eq!(
        [wrapped.offset_x as i32, wrapped.offset_y as i32],
        [frame0.offset_x as i32, frame0.offset_y as i32],
        "frame 2 wraps to frame 0"
    );
    // `ShadowIndex.Frame=-1` is cleared: native falls through to the walked
    // frame, which the atlas cannot key; retail's constructor value 0 never
    // takes this arm, so the pass keeps frame 0.
    let cleared = render(&vxl, Some(&hva), &params, 0, -1, false).unwrap();
    assert_eq!(
        [cleared.offset_x as i32, cleared.offset_y as i32],
        [frame0.offset_x as i32, frame0.offset_y as i32],
        "a cleared frame stays at frame 0"
    );
}

/// Phobos `ShadowIndices.Frame=` (`Ext/TechnoType/Body.cpp:1327-1337`) gives
/// every listed section its own `ChooseFrame` result: `0x7072A1` multiplies
/// that section's HVA transform into the shared `&mtx`, so two entries at
/// different frames must compose to the union of the two single-frame stencils.
/// The `-1` the native resize fills in draws the walked frame, which the atlas
/// cannot key and holds at frame 0.
#[test]
fn shadow_indices_frames_draw_each_listed_section_at_its_own_hva_frame() {
    let fixture: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/shadow_fixtures.json",
    ))
    .unwrap();
    let vxl = VxlFile::from_bytes(&bytes(fixture["vxl"].as_str().unwrap())).unwrap();
    assert_eq!(vxl.limbs.len(), 1, "the oracle fixture has one section");
    // A two-frame HVA for that section: frame 1 moves it along X, the way a
    // `ShadowIndices.Frame=` pose differs from the walked frame.
    let mut hva_bytes = vec![0u8; 16];
    hva_bytes[..9].copy_from_slice(b"ShadowHVA");
    hva_bytes.extend_from_slice(&2u32.to_le_bytes());
    hva_bytes.extend_from_slice(&1u32.to_le_bytes());
    let mut section = [0u8; 16];
    section[..5].copy_from_slice(b"BODY0");
    hva_bytes.extend_from_slice(&section);
    let identity: [f32; 12] = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0.];
    for value in identity {
        hva_bytes.extend_from_slice(&value.to_le_bytes());
    }
    let mut moved = identity;
    moved[3] = 64.;
    for value in moved {
        hva_bytes.extend_from_slice(&value.to_le_bytes());
    }
    let hva = HvaFile::from_bytes(&hva_bytes).unwrap();
    assert_eq!(hva.frame_count, 2, "two frames");

    let coverage = |sprite: &VxlSprite| {
        let mut set = std::collections::BTreeSet::new();
        for y in 0..sprite.height as i32 {
            for x in 0..sprite.width as i32 {
                if sprite.palette_indices[(y * sprite.width as i32 + x) as usize] != 0 {
                    set.insert((sprite.offset_x as i32 + x, sprite.offset_y as i32 + y));
                }
            }
        }
        set
    };
    for step in [0u8, 7] {
        let params = VxlRenderParams {
            facing: step * 8,
            ..Default::default()
        };
        let frame_0 = render(&vxl, Some(&hva), &params, 0, 0, false).unwrap();
        let frame_1 = render(&vxl, Some(&hva), &params, 0, 1, false).unwrap();
        assert_ne!(
            coverage(&frame_0),
            coverage(&frame_1),
            "the fixture's second frame must move the stencil (step {step})"
        );
        // Each listed section runs its own `ChooseFrame`: the entry at frame 1
        // must reach that section's HVA transform, so the composed stencil
        // differs from two frame-0 entries. Both entries share the primary's
        // projected center (`Hooks.MatrixOp.cpp:793-800` passes one `bnd`), so
        // the comparison is against the same composition, not two single
        // renders whose crops carry their own centers.
        let two_at_frame_0 =
            render_indices(&vxl, Some(&hva), &params, 0, &[0, 0], &[0, 0], false).unwrap();
        let second_at_frame_1 =
            render_indices(&vxl, Some(&hva), &params, 0, &[0, 0], &[0, 1], false).unwrap();
        assert_ne!(
            coverage(&two_at_frame_0),
            coverage(&second_at_frame_1),
            "the second entry's frame reaches that section's matrix (step {step})"
        );
        assert_eq!(
            coverage(&two_at_frame_0),
            coverage(&frame_0),
            "two frame-0 entries keep the single-section stencil (step {step})"
        );
        // `-1` (the native resize placeholder) and a missing entry both draw
        // the walked frame, which the atlas cannot key and holds at 0.
        assert_eq!(
            coverage(
                &render_indices(&vxl, Some(&hva), &params, 0, &[0, 0], &[0, -1], false).unwrap()
            ),
            coverage(&two_at_frame_0),
            "the resize placeholder draws the walked frame (step {step})"
        );
        assert_eq!(
            coverage(&render_indices(&vxl, Some(&hva), &params, 0, &[0, 0], &[0], false).unwrap()),
            coverage(&two_at_frame_0),
            "a short frame list keeps frame 0 (step {step})"
        );
    }
}
