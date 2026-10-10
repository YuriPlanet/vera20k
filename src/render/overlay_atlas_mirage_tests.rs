//! Registered Mirage images must load even when the map contains no Terrain
//! objects. Native InitTheater71DCA0 loads the registered TerrainType images;
//! Unit73C5F0 then borrows those SHPs without constructing a Terrain object.
use super::*;
use crate::render::terrain_draw_gpu_tests::Gpu;

#[test]
#[ignore = "requires retail theater archives and GPU; Mirage source catalog closure"]
fn retail_mirage_catalog_loads_without_placed_terrain() {
    let (_, mut assets) =
        crate::rules::retail_ini_fixture::retail_assets().expect("RA2_DIR supplies retail assets");
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules()
        .expect("retail Battle rules and fixed ART");
    let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let names = &retail.rules.general.default_mirage_disguises;
    assert!(
        !names.is_empty(),
        "original General/Terrain readers provide the stock pool"
    );
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/mirage_disguise.json",
    ))
    .unwrap();
    let native_images = corpus["initialization"]["tree_images"].as_array().unwrap();
    assert_eq!(
        names.iter().map(String::as_str).collect::<Vec<_>>(),
        native_images
            .iter()
            .map(|row| row["name"].as_str().unwrap())
            .collect::<Vec<_>>()
    );
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let gpu = Gpu::new();
    let batch = BatchRenderer::new_with_device(
        &gpu.device,
        &gpu.queue,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    );
    let atlas = build_overlay_atlas_on_device(
        &gpu.device,
        &gpu.queue,
        &batch,
        &[],
        &[],
        names,
        &assets,
        &theater.iso_palette,
        &theater.unit_palette,
        &theater.tiberium_palette,
        theater.extension,
        "TEMPERATE",
        &registry,
        &retail.rules.tiberium_types,
        &retail.rules.crate_rules,
        &retail.processed_rules,
        retail.rules.art(),
        None,
    )
    .expect("registered terrain disguises bind without any placed Terrain object");
    for name in names {
        assert!(
            retail
                .rules
                .terrain_object_type_case_insensitive(name)
                .is_some()
        );
        let image = retail
            .rules
            .art()
            .resolve_overlay_image_id(name, &retail.processed_rules);
        let candidates = art_data::overlay_shp_candidates(
            Some(retail.rules.art()),
            name,
            &image,
            theater.extension,
            "TEMPERATE",
        );
        let bytes = candidates
            .iter()
            .find_map(|candidate| assets.get_ref(candidate))
            .unwrap_or_else(|| panic!("physical registered shape {name}"));
        let native_image = native_images
            .iter()
            .find(|row| row["name"] == name.as_str())
            .unwrap();
        assert_eq!(
            bytes[..8]
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            native_image["header8"].as_str().unwrap(),
            "{name}: actual original InitTheater-loaded header"
        );
        let shape = ShpFile::from_bytes(bytes).unwrap();
        let (body, shadow) = atlas
            .native_terrain_pair(name, 0)
            .unwrap_or_else(|| panic!("paired Unit body/stencil source for {name}"));
        for (entry, index) in [(body, 0), (shadow, shape.frames.len() / 2)] {
            let frame = &shape.frames[index];
            assert_eq!(
                entry.pixel_size,
                [f32::from(frame.frame_width), f32::from(frame.frame_height)]
            );
            let offset =
                stored_frame_offset(shape.width, shape.height, frame.frame_x, frame.frame_y, 0.0);
            assert_eq!([entry.offset_x, entry.offset_y], [offset.0, offset.1]);
        }
    }
    gpu.queue.submit(std::iter::empty());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
}
