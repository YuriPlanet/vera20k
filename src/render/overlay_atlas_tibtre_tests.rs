//! Actual retail source decoding and paired atlas binding. Native source hashes
//! come from original CC_Draw_Shape, not another SHP decoder.
use super::*;
use crate::render::terrain_draw_gpu_tests::Gpu;

#[test]
#[ignore = "requires all retail theater archives and GPU; original stock SHP decoding and atlas binding"]
fn retail_tibtre_decode_and_atlas_pairs_match_original_shape_draw() {
    let (root, mut assets) =
        crate::rules::retail_ini_fixture::retail_assets().expect("RA2_DIR supplies retail assets");
    assert!(root.is_dir());
    let mut retail =
        crate::rules::retail_ini_fixture::retail_battle_rules().expect("retail Battle rules");
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/terrain_render.json",
    ))
    .unwrap();
    let stock = &native["stock_shapes"];
    assert_eq!(stock["assets"].as_array().unwrap().len(), 18);
    let gpu = Gpu::new();
    let batch = BatchRenderer::new_with_device(
        &gpu.device,
        &gpu.queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    );
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    for name in ["TEMPERATE", "SNOW", "URBAN", "NEWURBAN", "DESERT", "LUNAR"] {
        let theater = crate::map::theater::load_theater(&mut assets, name).unwrap();
        retail.rules.bind_terrain_spawner_assets(
            &retail.processed_rules,
            &assets,
            theater.extension,
            name,
        );
        let objects: Vec<_> = (1..=3)
            .map(|number| TerrainObject {
                rx: 0,
                ry: 0,
                name: format!("TIBTRE{number:02}"),
            })
            .collect();
        let atlas = build_overlay_atlas_on_device(
            &gpu.device,
            &gpu.queue,
            &batch,
            &[],
            &objects,
            &[],
            &assets,
            &theater.iso_palette,
            &theater.unit_palette,
            &theater.tiberium_palette,
            theater.extension,
            name,
            &registry,
            &retail.rules.tiberium_types,
            &retail.rules.crate_rules,
            &retail.processed_rules,
            retail.rules.art(),
            None,
        )
        .unwrap();
        for asset in stock["assets"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|asset| asset["theater"] == name)
        {
            let file = asset["name"].as_str().unwrap();
            let bytes = assets.get_ref(file).unwrap();
            let sha = crate::util::sha256::sha256_hex(bytes);
            assert_eq!(
                sha,
                asset["shp_sha256"].as_str().unwrap(),
                "{file}: original physical retail bytes"
            );
            let shape = stock["decoded_shapes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|shape| shape["shp_sha256"] == sha)
                .unwrap();
            let shp = ShpFile::from_bytes(bytes).unwrap();
            let type_name = file.split('.').next().unwrap().to_ascii_uppercase();
            assert_eq!(
                retail.rules.terrain_spawner_frame_count(&type_name),
                Some(22)
            );
            let terrain_type = retail
                .rules
                .terrain_object_type_case_insensitive(&type_name)
                .unwrap();
            assert_eq!(terrain_type.animation_rate, 3);
            assert_eq!(terrain_type.animation_probability.bits(), 0x3b44_9ba6);
            assert!(
                terrain_type.spawns_tiberium && terrain_type.is_animated && terrain_type.immune
            );
            for frame in shape["frames"].as_array().unwrap() {
                let index = frame["frame"].as_u64().unwrap() as usize;
                let actual = &shp.frames[index];
                assert_eq!(actual.format, 3);
                assert_eq!(
                    serde_json::json!([
                        actual.frame_x,
                        actual.frame_y,
                        actual.frame_width,
                        actual.frame_height
                    ]),
                    frame["native_rect"]
                );
                assert_eq!(
                    crate::util::sha256::sha256_hex(&actual.pixels),
                    frame["indices_sha256"].as_str().unwrap(),
                    "{file} frame{index}: original RLE source"
                );
                if index < 11 {
                    let (body, shadow) =
                        atlas.native_terrain_pair(&type_name, index as u8).unwrap();
                    for (entry, i) in [(body, index), (shadow, index + 11)] {
                        let source = &shp.frames[i];
                        assert_eq!(
                            entry.pixel_size,
                            [
                                f32::from(source.frame_width),
                                f32::from(source.frame_height)
                            ]
                        );
                        assert_eq!(
                            [entry.offset_x, entry.offset_y],
                            [
                                f32::from(source.frame_x) - 42.0,
                                f32::from(source.frame_y) - 28.0
                            ]
                        );
                    }
                }
            }
            assert!(atlas.native_terrain_pair(&type_name, 11).is_none());
        }
    }
    gpu.queue.submit(std::iter::empty());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
}
