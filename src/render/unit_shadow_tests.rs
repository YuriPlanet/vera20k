use super::super::{BatchRenderer, CachedUnitSprite, UnitAtlas, VxlLayer};
use super::*;

#[test]
#[ignore = "requires extracted original parts in VERA20K_SHADOW_PROBE_DIR"]
fn stock_shadow_mask_uses_production_part_pixels_and_offsets() {
    use crate::assets::{hva_file::HvaFile, vpl_file::VplFile, vxl_file::VxlFile};
    use crate::render::vxl_raster::{self, VxlRenderParams};
    let root = std::path::PathBuf::from(
        std::env::var_os("VERA20K_SHADOW_PROBE_DIR").expect("set rendering-parity root"),
    );
    let vpl = VplFile::from_bytes(
        &std::fs::read(root.join("grizzly-raster-proof/extract/voxels.vpl")).unwrap(),
    )
    .unwrap();
    for (model, folder) in [("GTNK", "aligned-native"), ("HTNK", "rhino-neutral-native")] {
        for step in (0..32u8).step_by(4) {
            let mut body = vec![0; 65536];
            for suffix in ["", "TUR", "BARL"] {
                let base = if model == "GTNK" && suffix.is_empty() {
                    root.join("grizzly-raster-proof/extract/gtnk")
                } else {
                    root.join("grizzly-part-composition/extract")
                        .join(format!("{model}{suffix}"))
                };
                let vxl = VxlFile::from_bytes(&std::fs::read(base.with_extension("VXL")).unwrap())
                    .unwrap();
                let hva = HvaFile::from_bytes(&std::fs::read(base.with_extension("HVA")).unwrap())
                    .unwrap();
                let sprite = vxl_raster::render_vxl(
                    &vxl,
                    Some(&hva),
                    &VxlRenderParams {
                        facing: step * 8,
                        ..Default::default()
                    },
                    Some(&vpl),
                );
                // Retail GTNK/HTNK TurretOffset is zero in these static scenes.
                // Each production part's own native/padded offset is its anchor.
                composite_mask_part(
                    &mut body,
                    &sprite.palette_indices,
                    sprite.width,
                    sprite.height,
                    [128 + sprite.offset_x as i32, 128 + sprite.offset_y as i32],
                );
            }
            let native = std::fs::read(
                root.join("grizzly-part-composition")
                    .join(folder)
                    .join(format!("{step:02}.bin")),
            )
            .unwrap();
            assert_eq!(
                body.iter()
                    .zip(native)
                    .filter(|(a, b)| (**a != 0) != (*b != 0))
                    .count(),
                0,
                "{model} {step} all body mask pixels"
            );
        }
    }
    eprintln!(
        "16 original Unit composed masks match current production part offsets/nonzero pixels"
    );
}

fn sample_indices(
    gpu: &crate::render::terrain_draw_gpu_tests::Gpu,
    atlas: &UnitAtlas,
    key: &UnitSpriteKey,
) -> Vec<u8> {
    let entry = atlas.get(key).unwrap();
    let texture = &atlas.pages[entry.page].texture;
    let origin = [
        (entry.uv_origin[0] * texture.width as f32).round() as u32,
        (entry.uv_origin[1] * texture.height as f32).round() as u32,
    ];
    gpu.read_uint_texels(&texture.view, origin, entry.pixel_size.map(|v| v as u32))
}

#[test]
#[ignore = "requires GPU; native Jumpjet mask controls through atlas and pose-page upload"]
fn jumpjet_native_masks_keep_ground_flight_and_frame_payloads_separate() {
    use crate::assets::{hva_file::HvaFile, vxl_file::VxlFile};
    use crate::render::unit_atlas::GROUNDED_SHADOW_FRAME_FLAG;
    use crate::render::unit_pose_cache::VxlPoseFrameCache;
    use crate::render::vxl_raster::{self, VxlRenderParams};
    let gpu = crate::render::terrain_draw_gpu_tests::Gpu::new();
    let batch = BatchRenderer::new_with_device(
        &gpu.device,
        &gpu.queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    );
    let fixture = crate::test_fixture::json("tools/voxel_oracle/unit_shadow.json");
    let data = &fixture["generated"];
    let decode = |s: &str| {
        s.as_bytes()
            .chunks_exact(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect::<Vec<_>>()
    };
    let vxl = VxlFile::from_bytes(&decode(data["vxl"].as_str().unwrap())).unwrap();
    let hva = HvaFile::from_bytes(&decode(data["hva"].as_str().unwrap())).unwrap();
    let body_key = UnitSpriteKey {
        type_id: "native-unit".into(),
        turret_index: 0,
        facing: 0,
        layer: VxlLayer::Body,
        frame: 0,
        slope_type: 0,
        barrel_pitch: 0,
    };
    let mut cached = vec![CachedUnitSprite {
        key: body_key.clone(),
        pixels: decode(data["body"].as_str().unwrap()),
        width: 256,
        height: 256,
        offset_x: -128.,
        offset_y: -128.,
        native_draw_bounds: Some([-128, -128, 256, 256]),
        shadow_center: None,
        pool_bytes: 0,
    }];
    let key_for = |caller: &serde_json::Value| UnitSpriteKey {
        type_id: "native-unit".into(),
        turret_index: 0,
        facing: vxl_raster::voxel_facing_step_u16(
            caller["input"]["primary"].as_u64().unwrap() as u16
        ) * 8,
        layer: VxlLayer::Shadow,
        frame: 0,
        slope_type: caller["input"]["slope"].as_u64().unwrap_or(0) as u8,
        barrel_pitch: 0,
    };
    for case in data["cases"].as_array().unwrap() {
        let caller = &fixture["callers"][case["caller"].as_u64().unwrap() as usize];
        let key = key_for(caller);
        if cached.iter().any(|s| s.key == key) {
            continue;
        }
        let params = VxlRenderParams {
            facing: key.facing,
            slope_type: key.slope_type,
            ..Default::default()
        };
        let sprite = vxl_raster::shadow::render(&vxl, Some(&hva), &params, 0, true).unwrap();
        let center = vxl_raster::shadow::center(&vxl, Some(&hva), &params, 0, true);
        for frame in [0, GROUNDED_SHADOW_FRAME_FLAG] {
            let mut key = key.clone();
            key.frame = frame;
            cached.push(CachedUnitSprite {
                key,
                pixels: sprite.palette_indices.clone(),
                width: sprite.width,
                height: sprite.height,
                offset_x: sprite.offset_x,
                offset_y: sprite.offset_y,
                native_draw_bounds: Some([
                    sprite.offset_x as i32,
                    sprite.offset_y as i32,
                    sprite.width as i32,
                    sprite.height as i32,
                ]),
                shadow_center: center,
                pool_bytes: 0,
            });
        }
    }
    let mut atlas = UnitAtlas::from_test_sprites(cached);
    atlas.upload(&gpu.device, &gpu.queue, &batch);
    let expected = |case: &serde_json::Value| {
        let r = &case["rect"];
        let [x, y, w, h] = [2, 3, 4, 5].map(|i| r[i].as_u64().unwrap() as usize);
        let mut full = vec![0; 65536];
        for p in case["pixels"].as_array().unwrap() {
            full[p[0].as_u64().unwrap() as usize] = p[1].as_u64().unwrap() as u8;
        }
        (y..y + h)
            .flat_map(|row| full[row * 256 + x..row * 256 + x + w].iter().copied())
            .collect::<Vec<_>>()
    };
    let parts = [UnitShadowPart {
        entry: atlas.get(&body_key).unwrap(),
        point: [0, 0],
        posed: false,
    }];
    let mut poses = VxlPoseFrameCache::default();
    // The body page fills first: shadow upload must select its own later page.
    let opaque = poses.fill_test_page();
    let mut fresh_first = None;
    for case in data["cases"].as_array().unwrap() {
        let caller = &fixture["callers"][case["caller"].as_u64().unwrap() as usize];
        let key = key_for(caller);
        if !caller["body_mask_present"].as_bool().unwrap() {
            assert_eq!(sample_indices(&gpu, &atlas, &key), expected(case));
            continue;
        }
        let offset =
            std::array::from_fn(|i| caller["renderer"]["mask_offset"][i].as_i64().unwrap() as i32);
        let entry = poses
            .render_unit_shadow(&atlas, &key, offset, &parts)
            .unwrap();
        fresh_first.get_or_insert(entry);
        poses.submit(&gpu.device, &gpu.queue, &batch);
        let tex = poses.page_texture(entry.page).unwrap();
        assert_eq!(
            gpu.read_uint_texels(
                &tex.view,
                entry
                    .uv_origin
                    .map(|v| (v * tex.width as f32).round() as u32),
                entry.pixel_size.map(|v| v as u32)
            ),
            expected(case)
        );
        let repeated = poses
            .render_unit_shadow(&atlas, &key, offset, &parts)
            .unwrap();
        assert_eq!(
            (repeated.page, repeated.uv_origin),
            (entry.page, entry.uv_origin)
        );
    }
    let key = key_for(&fixture["callers"][0]);
    let mut grounded = key.clone();
    grounded.frame |= GROUNDED_SHADOW_FRAME_FLAG;
    assert!(atlas.prepare_native_shadow_with(&grounded, 0, |body| {
        poses.stamp_shadow_parts(&atlas, &parts, body)
    }));
    atlas.upload(&gpu.device, &gpu.queue, &batch);
    assert_eq!(
        sample_indices(&gpu, &atlas, &grounded),
        expected(&data["cases"][0])
    );
    assert_eq!(
        sample_indices(&gpu, &atlas, &key),
        expected(&data["cases"][4]),
        "ground upload cannot mutate the high-flight stencil"
    );
    assert!(
        atlas.prepare_native_shadow_with(&grounded, 0, |_| panic!("retained mask reread source"))
    );
    let posed = [UnitShadowPart {
        entry: opaque,
        point: [0, 0],
        posed: true,
    }];
    let masked = poses
        .render_unit_shadow(&atlas, &key, [0, 15], &posed)
        .unwrap();
    poses.submit(&gpu.device, &gpu.queue, &batch);
    let tex = poses.page_texture(masked.page).unwrap();
    assert!(
        gpu.read_uint_texels(
            &tex.view,
            masked
                .uv_origin
                .map(|v| (v * tex.width as f32).round() as u32),
            masked.pixel_size.map(|v| v as u32)
        )
        .iter()
        .all(|&p| p == 0),
        "frame-local body pixels must participate in the mask"
    );
    let first = fresh_first.unwrap();
    let started = std::time::Instant::now();
    for _ in 0..20_000 {
        let hit = poses
            .render_unit_shadow(&atlas, &key, [0, 0], &parts)
            .unwrap();
        assert_eq!((hit.page, hit.uv_origin), (first.page, first.uv_origin));
    }
    eprintln!(
        "20k shared same-frame low-flight masks: {:?}",
        started.elapsed()
    );
    poses.begin_frame();
    let next = poses
        .render_unit_shadow(&atlas, &key, [0, 0], &parts)
        .unwrap();
    assert_ne!(
        next.page, first.page,
        "begin_frame must discard old shadow-source identities"
    );
    poses.submit(&gpu.device, &gpu.queue, &batch);
    let tex = poses.page_texture(next.page).unwrap();
    assert_eq!(
        gpu.read_uint_texels(
            &tex.view,
            next.uv_origin
                .map(|v| (v * tex.width as f32).round() as u32),
            next.pixel_size.map(|v| v as u32)
        ),
        expected(&data["cases"][0])
    );
}

#[test]
#[ignore = "requires GPU; actual atlas placement/upload, mask first fill/hit and a later picture"]
fn shadow_first_fill_hits_and_later_pictures_keep_payloads() {
    let gpu = crate::render::terrain_draw_gpu_tests::Gpu::new();
    let batch = BatchRenderer::new_with_device(
        &gpu.device,
        &gpu.queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    );
    let make = |layer, pixels: Vec<u8>, width, height, offset: [f32; 2]| CachedUnitSprite {
        key: UnitSpriteKey {
            type_id: "generated".into(),
            turret_index: 0,
            facing: 0,
            layer,
            frame: 0,
            slope_type: 0,
            barrel_pitch: 0,
        },
        pixels,
        width,
        height,
        offset_x: offset[0],
        offset_y: offset[1],
        native_draw_bounds: Some([
            offset[0] as i32,
            offset[1] as i32,
            width as i32,
            height as i32,
        ]),
        shadow_center: None,
        pool_bytes: 0,
    };
    let cache = vec![
        make(
            VxlLayer::Body,
            vec![33, 0, 33, 0, 0, 33, 0, 33],
            4,
            2,
            [-2., -1.],
        ),
        make(VxlLayer::Shadow, vec![1; 24], 6, 4, [-3., -2.]),
    ];
    let shadow_key = cache[1].key.clone();
    let body_key = cache[0].key.clone();
    let mut atlas = UnitAtlas::from_test_sprites(cache);
    let body = atlas.get(&body_key).unwrap();
    assert!(atlas.prepare_native_shadow(&shadow_key, 0, [(body, [0., 0.])]));
    atlas.upload(&gpu.device, &gpu.queue, &batch);
    let expected = vec![
        1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1,
    ];
    assert_eq!(sample_indices(&gpu, &atlas, &shadow_key), expected);
    let bomb = std::iter::from_fn(|| -> Option<(UnitSpriteEntry, [f32; 2])> {
        panic!("cache hit read new body")
    });
    assert!(atlas.prepare_native_shadow(&shadow_key, 0, bomb));
    // A picture drawn later goes beside the resident ones on their shelf and
    // moves nothing, so the uploaded mask stays where queued draws reference
    // it, and its own upload overwrites none of their texels.
    let resident_shadow = atlas.get(&shadow_key).unwrap();
    let turret = make(VxlLayer::Turret, vec![57; 320], 40, 8, [-20., -4.]);
    let turret_key = turret.key.clone();
    atlas.place_test_sprite(turret);
    atlas.upload(&gpu.device, &gpu.queue, &batch);
    let grown_shadow = atlas.get(&shadow_key).unwrap();
    assert_eq!(
        (grown_shadow.page, grown_shadow.uv_origin),
        (resident_shadow.page, resident_shadow.uv_origin)
    );
    assert_eq!(atlas.get(&turret_key).unwrap().page, resident_shadow.page);
    assert!(atlas.prepare_native_shadow(&shadow_key, 0, []));
    assert_eq!(sample_indices(&gpu, &atlas, &shadow_key), expected);
    assert_eq!(
        sample_indices(&gpu, &atlas, &body_key),
        vec![33, 0, 33, 0, 0, 33, 0, 33]
    );
    assert_eq!(sample_indices(&gpu, &atlas, &turret_key), vec![57; 320]);
    let start = std::time::Instant::now();
    for _ in 0..20_000 {
        assert!(atlas.prepare_native_shadow(&shadow_key, 0, []));
    }
    eprintln!(
        "20k stable shadow cache-hit CPU lookup: {:?}; no mask reads/uploads",
        start.elapsed()
    );
}

/// Shadow composition finds a part's source by its atlas placement and a
/// template by key, through the index placement keeps, without walking every
/// resident sprite; an unknown rectangle stamps nothing.
#[test]
fn shadow_sources_are_found_by_placement_and_key() {
    let key = |layer| UnitSpriteKey {
        type_id: "indexed".into(),
        turret_index: 0,
        facing: 0,
        layer,
        frame: 0,
        slope_type: 0,
        barrel_pitch: 0,
    };
    let sprite = |layer, pixels: Vec<u8>| CachedUnitSprite {
        key: key(layer),
        pixels,
        width: 2,
        height: 2,
        offset_x: 0.,
        offset_y: 0.,
        native_draw_bounds: Some([0, 0, 2, 2]),
        shadow_center: None,
        pool_bytes: 0,
    };
    let atlas = UnitAtlas::from_test_sprites(vec![
        sprite(VxlLayer::Body, vec![7, 0, 0, 7]),
        sprite(VxlLayer::Shadow, vec![0, 9, 9, 0]),
    ]);
    let shadow = atlas.get(&key(VxlLayer::Shadow)).unwrap();

    let mut body = vec![0u8; 65536];
    assert!(atlas.stamp_shadow_part(&mut body, &shadow, [10, 20]));
    let stamped = [20 * 256 + 10, 20 * 256 + 11, 21 * 256 + 10, 21 * 256 + 11].map(|i| body[i]);
    assert_eq!(stamped, [0, 1, 1, 0]);
    let elsewhere = UnitSpriteEntry {
        page: shadow.page + 1,
        ..shadow
    };
    assert!(!atlas.stamp_shadow_part(&mut body, &elsewhere, [0, 0]));
    let resident = atlas.resident.borrow();
    let template = resident.rendered_by_key(&key(VxlLayer::Body)).unwrap();
    assert_eq!(template.pixels, [7, 0, 0, 7]);
}
