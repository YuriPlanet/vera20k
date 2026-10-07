use super::*;
use serde_json::Value;

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks_exact(2)
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
        )
        .unwrap();
        if case["masked"].as_bool().unwrap() {
            mask_body(
                &mut sprite.palette_indices,
                sprite.width,
                [sprite.offset_x as i32, sprite.offset_y as i32],
                &body,
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
            }
        )
        .is_none()
    );
    assert!(
        render(
            &vxl,
            Some(&hva),
            &VxlRenderParams {
                slope_type: 1,
                ..Default::default()
            }
        )
        .is_none()
    );
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
