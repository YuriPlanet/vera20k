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
#[ignore = "requires GPU; actual atlas packing/upload, mask first fill/hit and growth"]
fn shadow_first_fill_hits_and_atlas_growth_keep_payloads() {
    let gpu = crate::render::terrain_draw_gpu_tests::Gpu::new();
    let batch = BatchRenderer::new_with_device(
        &gpu.device,
        &gpu.queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    );
    let make = |layer, pixels: Vec<u8>, width, height, offset: [f32; 2]| CachedUnitSprite {
        key: UnitSpriteKey {
            type_id: "generated".into(),
            facing: 0,
            layer,
            frame: 0,
            slope_type: 0,
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
    let mut atlas =
        super::super::pack_sprites_on_device(&gpu.device, &gpu.queue, &batch, &cache).unwrap();
    atlas.rendered_cache = cache;
    let body = *atlas.get(&body_key).unwrap();
    assert!(atlas.prepare_native_shadow(&gpu.queue, &shadow_key, [(body, [0., 0.])]));
    let expected = vec![
        1, 1, 1, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1,
    ];
    assert_eq!(sample_indices(&gpu, &atlas, &shadow_key), expected);
    let bomb = std::iter::from_fn(|| -> Option<(UnitSpriteEntry, [f32; 2])> {
        panic!("cache hit read new body")
    });
    assert!(atlas.prepare_native_shadow(&gpu.queue, &shadow_key, bomb));
    // Growth appends to a new page and moves nothing already resident, so the
    // uploaded mask stays where queued draws reference it.
    let resident_shadow = *atlas.get(&shadow_key).unwrap();
    let turret = make(VxlLayer::Turret, vec![57; 320], 40, 8, [-20., -4.]);
    let turret_key = turret.key.clone();
    atlas.append_sprites(&gpu.device, &gpu.queue, &batch, vec![turret]);
    let grown_shadow = *atlas.get(&shadow_key).unwrap();
    assert_eq!(
        (grown_shadow.page, grown_shadow.uv_origin),
        (resident_shadow.page, resident_shadow.uv_origin)
    );
    assert_ne!(atlas.get(&turret_key).unwrap().page, resident_shadow.page);
    assert!(atlas.prepare_native_shadow(&gpu.queue, &shadow_key, []));
    assert_eq!(sample_indices(&gpu, &atlas, &shadow_key), expected);
    assert_eq!(
        sample_indices(&gpu, &atlas, &body_key),
        vec![33, 0, 33, 0, 0, 33, 0, 33]
    );
    assert_eq!(sample_indices(&gpu, &atlas, &turret_key), vec![57; 320]);
    let start = std::time::Instant::now();
    for _ in 0..20_000 {
        assert!(atlas.prepare_native_shadow(&gpu.queue, &shadow_key, []));
    }
    eprintln!(
        "20k stable shadow cache-hit CPU lookup: {:?}; no mask reads/uploads",
        start.elapsed()
    );
}
