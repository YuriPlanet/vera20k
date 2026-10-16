//! Physical ClearTile TMP through the production decoder, atlas, live cell
//! selection, common lighting, A binding, upload and ordinary terrain draw.
//! Expected RGB565 and stored Z are captured by original Cell480350/TMP547CF0.
use super::draw_pooled_zdepth;
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::map::lighting::{LightingProfileUnits, build_cell_light_grid_from_heights_and_units};
use crate::map::map_file::{MapCell, MapFile};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::terrain::{TilePlacement, build_terrain_grid_from_resolved, iso_to_screen};
use crate::map::theater::{TileKey, load_theater, load_tile_images};
use crate::map::tile_variant_selector::TileVariantSelectorCache;
use crate::render::batch::{BatchRenderer, CameraUniform, InstanceBufferPool};
use crate::render::shroud_buffer::ShroudBuffer;
use crate::render::terrain_draw_gpu_tests::{
    Gpu, assert_native_color_and_depth, camera, clear, encoded, extent,
};
use crate::render::tile_atlas::TileAtlas;
use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;

fn profile(input: &Value) -> LightingProfileUnits {
    let p = &input["scene_lighting"];
    LightingProfileUnits {
        ambient_percent: p["ambient_percent"].as_i64().unwrap() as i32,
        red_percent: p["red_percent"].as_i64().unwrap() as i32,
        green_percent: p["green_percent"].as_i64().unwrap() as i32,
        blue_percent: p["blue_percent"].as_i64().unwrap() as i32,
        ground_units: p["ground_units"].as_i64().unwrap() as i32,
        level_units: p["level_units"].as_i64().unwrap() as i32,
    }
}

fn a_pixels(input: &Value, size: [u32; 2]) -> Vec<u8> {
    if let Some(hex) = input["a_hex"].as_str() {
        hex.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    } else {
        vec![input["a"].as_u64().unwrap() as u8; (size[0] * size[1]) as usize]
    }
}

/// Normal Battle construction supplies all sight and Cell knowledge. The native
/// component independently constructs AMCV on physical GoldSt terrain; its eight
/// TMP variants share one live A plane, so the normal selector needs no override.
#[test]
#[ignore = "requires retail assets and GPU; normal AMCV source through live shroud upload and original TMP frontier pixels"]
fn retail_amcv_sight_reaches_the_live_shroud_and_tmp_raster() {
    use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};

    let packet: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/shroud_frontier.json",
    ))
    .unwrap();
    let root = PathBuf::from(std::env::var_os("RA2_DIR").expect("physical retail root"));
    let mut launch = crate::headless_scenario::battle_session(
        "GoldSt.mmx",
        &[(LaunchCountry::France, AiDifficulty::Easy)],
    );
    launch.player_name = "VERA-OBSERVER".into();
    launch.local.start_position = LaunchStartPosition::Position(3);
    launch.opponents[0].start_position = LaunchStartPosition::Position(0);
    launch.options.unit_count = 0;
    launch.options.starting_credits = 10_000;
    launch.options.tech_level = 10;
    launch.options.game_speed = 1;
    launch.options.default_ai_difficulty = 0;
    launch.options.short_game = false;
    launch.options.bases = true;
    launch.options.shroud = true;
    launch.options.fog_of_war = false;
    let mut scene =
        crate::headless_scenario::load_session(&root, "GoldSt.mmx", 0x1234_5678, launch).unwrap();
    assert_eq!(scene.sim().session.tick, 0);
    let owner = scene.sim().session.current_house.unwrap();
    let sources: Vec<_> = scene
        .sim()
        .entities()
        .iter_sorted()
        .filter(|(_, entity)| {
            entity.owner() == owner && scene.sim().interner.resolve(entity.type_ref()) == "AMCV"
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        sources.len(),
        1,
        "ordinary starting force supplies one AMCV"
    );
    let source_id = sources[0];
    let mut assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let theater = load_theater(&mut assets, "URBAN").unwrap();
    for source in packet["inputs"]["sources"].as_array().unwrap() {
        let name = source["name"].as_str().unwrap();
        assert_eq!(
            crate::util::sha256::sha256_hex(assets.get_ref(name).unwrap()),
            source["sha256"].as_str().unwrap(),
            "physical native/production input {name}"
        );
    }
    let tiles = load_tile_images(
        &assets,
        &theater.lookup,
        &theater.iso_palette,
        &HashSet::from([TileKey {
            tile_id: 0,
            sub_tile: 0,
            variant: 0,
        }]),
    );
    assert_eq!(tiles.len(), 8);
    let shp = crate::assets::shp_file::ShpFile::from_bytes(assets.get_ref("SHROUD.SHP").unwrap())
        .unwrap();
    let (frames, frame_w, frame_h) = crate::render::shroud_buffer::extract_shp_brightness(&shp);
    let history = &packet["histories"][0];
    let size: [u32; 2] = std::array::from_fn(|i| {
        packet["surface"][if i == 0 { "width" } else { "height" }]
            .as_u64()
            .unwrap() as u32
    });
    let expected_a = a_pixels(
        &serde_json::json!({"a_hex": history["output"]["alpha_hex"]}),
        size,
    );
    let uncached = packet["controls"]["uncached_reveal"]["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "elevated_goldst")
        .unwrap();
    let draw = &uncached["draw_output"];
    let uncached_output = if let Some(name) = draw["history"].as_str() {
        &packet["histories"]
            .as_array()
            .unwrap()
            .iter()
            .find(|history| history["name"] == name)
            .unwrap()["output"]
    } else {
        draw
    };
    let uncached_a = a_pixels(
        &serde_json::json!({"a_hex": uncached_output["alpha_hex"]}),
        size,
    );
    assert_eq!(
        crate::util::sha256::sha256_hex(&uncached_a),
        draw["alpha_sha256"].as_str().unwrap()
    );
    let restore_local_size = [
        scene.map.header.local_left as i32,
        scene.map.header.local_top as i32,
        scene.map.header.local_width as i32,
        scene.map.header.local_height as i32,
    ];
    let gpu = Gpu::new();
    let mut source = ShroudBuffer::new(
        &gpu.device,
        &gpu.queue,
        size,
        [scene.sim().fog.width, scene.sim().fog.height],
        frames,
        [frame_w, frame_h],
        crate::render::shroud_buffer::SHROUD_EDGE_LUT,
    );
    let mut lighting = crate::app::presentation::lighting::MatchLighting::default();
    lighting.install(
        crate::map::lighting::CellLightGrid::new(),
        crate::map::lighting::parse_lighting(&scene.map.ini),
        2,
        Some((
            scene.sim().resolved_terrain.as_ref().unwrap(),
            scene.sim(),
            &scene.runtime.resources.rules,
        )),
    );
    let mut renderers = [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ]
    .map(|format| {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let atlas = crate::render::tile_atlas::build_atlas(&gpu.device, &gpu.queue, &batch, &tiles);
        (format, batch, atlas, InstanceBufferPool::new())
    });
    for step in 0..=3 {
        if (1..=2).contains(&step) {
            scene.tick();
        } else if step == 3 {
            // Actual Action40 owner: false->true membership invokes70ADC0.
            // An equal LocalSize writer would not exercise that callback.
            // No frame between demotion and restoration releases the source.
            let runtime = &mut scene.runtime;
            assert!(
                runtime
                    .simulation
                    .change_visible_map_area([2, 2, 1, 1], Some(&runtime.resources.rules),)
            );
            assert!(
                !runtime
                    .simulation
                    .entities()
                    .get(source_id)
                    .unwrap()
                    .in_playfield()
            );
            assert!(
                runtime
                    .simulation
                    .change_visible_map_area(restore_local_size, Some(&runtime.resources.rules),)
            );
            assert!(
                runtime
                    .simulation
                    .entities()
                    .get(source_id)
                    .unwrap()
                    .in_playfield()
            );
            assert_eq!(runtime.simulation.session.tick, 2);
        }
        let entity = scene.sim().entities().get(source_id).unwrap();
        assert!(entity.lifecycle.object_alive() && !entity.lifecycle.in_limbo());
        assert!(entity.lifecycle.cell_marked() && entity.in_playfield());
        let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        let xyz = [
            coord.x,
            coord.y,
            crate::sim::movement::ground_pose::object_world_z_leptons(
                entity,
                scene.sim().resolved_terrain.as_ref(),
            ),
        ];
        assert_eq!(
            serde_json::json!(xyz),
            packet["inputs"]["source"]["native_xyz"]
        );
        assert!(
            scene
                .runtime
                .simulation
                .prepare_fog_view_for("VERA-OBSERVER")
        );
        let hash = scene.sim().state_hash();
        let terrain = scene.sim().resolved_terrain.as_ref().unwrap();
        let coords = [111, 22];
        let live = terrain.cell(coords[0], coords[1]).unwrap();
        assert_eq!(terrain.presentation_tile(live), (0, 0));
        assert_eq!(live.level, 6);
        let rows = packet["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 8);
        let row = rows
            .iter()
            .find(|row| {
                row["output"]["selected_file_index"].as_u64() == Some(u64::from(live.variant))
            })
            .unwrap();
        let input = &row["input"];
        assert_eq!(profile(input), scene.sim().session.lighting.normal.into());
        let mut camera = camera(size);
        camera.camera_pos = [
            input["full_view"]["view_origin"][0].as_i64().unwrap() as f32,
            input["full_view"]["view_origin"][1].as_i64().unwrap() as f32 + 15.0,
        ];
        source.rebuild_if_needed(
            &gpu.device,
            &gpu.queue,
            &scene.sim().fog,
            owner,
            camera.camera_pos[0],
            camera.camera_pos[1],
            size[0],
            size[1],
            1.0,
        );
        let actual_a: Vec<_> = gpu
            .read_float_texels(source.gpu_source().0, [0, 0], size)
            .into_iter()
            .map(|pixel| (pixel[0] * 255.0).round() as u8)
            .collect();
        assert_eq!(
            &actual_a,
            if step == 3 { &uncached_a } else { &expected_a },
            "phase{step}: original full A after starting sight or explicit Action40 restoration"
        );
        assert!(row["output"]["sampled_a_values"].as_array().unwrap().len() > 1);
        let light = lighting
            .grid()
            .cell_light_at((coords[0], coords[1]))
            .unwrap();
        assert_eq!(
            serde_json::json!(light.rgb_key),
            row["output"]["convert_rgb_key"]
        );
        assert_eq!(
            i64::from(light.top_scalar),
            row["output"]["top_scalar"].as_i64().unwrap()
        );
        assert_eq!(
            i64::from(light.bottom_scalar),
            row["output"]["bottom_scalar"].as_i64().unwrap()
        );
        assert_eq!(
            i64::from(light.common_scalar),
            row["output"]["common_scalar"].as_i64().unwrap()
        );
        let mut grid = build_terrain_grid_from_resolved(terrain, None, None);
        grid.cells.retain(|cell| [cell.rx, cell.ry] == coords);
        assert_eq!(grid.cells.len(), 1);
        for (format, batch, atlas, pool) in &mut renderers {
            let lookup = |tile_id, sub_tile, variant| {
                atlas
                    .get_uv(TileKey {
                        tile_id,
                        sub_tile,
                        variant,
                    })
                    .map(|uv| TilePlacement {
                        uv_origin: uv.uv_origin,
                        uv_size: uv.uv_size,
                        pixel_size: uv.pixel_size,
                        draw_offset: uv.draw_offset,
                    })
            };
            let instances = crate::render::terrain_instances::build_visible_instances(
                &grid,
                Some(lighting.grid()),
                camera.camera_pos[0],
                camera.camera_pos[1],
                size[0] as f32,
                size[1] as f32,
                Some(&lookup),
                Some(terrain),
            );
            assert_eq!(instances.normal.len(), 1);
            pool.upload_on_device(
                &gpu.device,
                &gpu.queue,
                "tmp-clear-native",
                &instances.normal,
            );
            assert_tmp_raster(
                &gpu, batch, pool, atlas, &source, camera, row, *format, size,
            );
        }
        assert_eq!(
            scene.sim().state_hash(),
            hash,
            "presentation retains simulation state"
        );
    }
}

/// Submit the ordinary terrain batch with its actual A source. Both the
/// prepared leaf controls and the live source frontier use these same upload,
/// bind, pass and readback calls.
fn assert_tmp_raster(
    gpu: &Gpu,
    batch: &mut BatchRenderer,
    pool: &InstanceBufferPool,
    atlas: &TileAtlas,
    a: &ShroudBuffer,
    camera: CameraUniform,
    row: &Value,
    format: wgpu::TextureFormat,
    size: [u32; 2],
) {
    let input = &row["input"];
    batch.bind_shroud(&gpu.device, Some(a));
    batch.write_camera(&gpu.queue, camera);
    let color = gpu.target(size, format);
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let cv = color.create_view(&Default::default());
    let dv = depth.create_view(&Default::default());
    let background = input["background"].as_u64().unwrap() as u16;
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &color,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &encoded(background, format).repeat((size[0] * size[1]) as usize),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * 4),
            rows_per_image: Some(size[1]),
        },
        extent(size),
    );
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    clear(
        &mut encoder,
        &cv,
        &dv,
        input["old_z"].as_u64().unwrap() as u16,
        wgpu::LoadOp::Load,
    );
    {
        let mut pass =
            crate::app::presentation::sidebar_render::begin_main_load_pass(&mut encoder, &cv, &dv);
        let clip: [u32; 4] = std::array::from_fn(|i| input["clip"][i].as_u64().unwrap() as u32);
        pass.set_scissor_rect(clip[0], clip[1], clip[2], clip[3]);
        for _ in 0..input["repeat"].as_u64().unwrap() {
            draw_pooled_zdepth(&mut pass, batch, pool, Some(atlas), "tmp-clear-native");
        }
    }
    let reads = [
        gpu.read(&mut encoder, &color),
        gpu.read(&mut encoder, &depth),
    ];
    let output = gpu.finish(encoder, &reads, size);
    assert_native_color_and_depth(&output, row, format, size);
}

#[test]
#[ignore = "requires retail assets and GPU; original Cell480350/TMP547CF0 color and depth through production terrain owners"]
fn retail_clear_tmp_matches_original_cell_raster() {
    let packet: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/tmp_clear_render.json",
    ))
    .unwrap();
    let full_view_rows: Vec<_> = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["input"].get("full_view").is_some())
        .collect();
    assert_eq!(full_view_rows.len(), 1);
    assert_eq!(
        full_view_rows[0]["input"]["coords"],
        serde_json::json!([64, 31])
    );
    let root = PathBuf::from(std::env::var_os("RA2_DIR").expect("physical retail root"));
    let mut assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let theater = load_theater(&mut assets, "TEMPERATE").unwrap();
    assert_eq!(theater.rmg_tiles.clear_tile, Some(0));
    assert_eq!(theater.lookup.total_file_count(0), 8);
    for source in packet["inputs"]["sources"].as_array().unwrap() {
        let name = source["name"].as_str().unwrap();
        let lower = name.to_ascii_lowercase();
        if !((lower.starts_with("clear01") && lower.ends_with(".tem"))
            || matches!(
                lower.as_str(),
                "isotem.pal" | "temperatmd.ini" | "xmp03t4.map" | "shroud.shp"
            ))
        {
            continue;
        }
        assert_eq!(
            crate::util::sha256::sha256_hex(assets.get_ref(name).unwrap()),
            source["sha256"].as_str().unwrap(),
            "physical native/production input {name}"
        );
    }
    let tiles = load_tile_images(
        &assets,
        &theater.lookup,
        &theater.iso_palette,
        &HashSet::from([TileKey {
            tile_id: 0,
            sub_tile: 0,
            variant: 0,
        }]),
    );
    assert_eq!(tiles.len(), 8);
    // The same physical map also crosses the normal bound scenario loader,
    // then the app's live first-frame lighting publication owner.
    let scene = crate::headless_scenario::load(&root, "XMP03T4.MAP", 0).unwrap();
    let mut match_lighting = crate::app::presentation::lighting::MatchLighting::default();
    match_lighting.install(
        crate::map::lighting::CellLightGrid::new(),
        crate::map::lighting::parse_lighting(&scene.map.ini),
        2,
        Some((
            scene.sim().resolved_terrain.as_ref().unwrap(),
            scene.sim(),
            &scene.runtime.resources.rules,
        )),
    );
    let gpu = Gpu::new();
    let size: [u32; 2] = std::array::from_fn(|i| {
        packet["surface"][if i == 0 { "width" } else { "height" }]
            .as_u64()
            .unwrap() as u32
    });
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let atlas = crate::render::tile_atlas::build_atlas(&gpu.device, &gpu.queue, &batch, &tiles);
        let mut pool = InstanceBufferPool::new();
        for row in packet["rows"].as_array().unwrap() {
            let input = &row["input"];
            let coords: [u16; 2] =
                std::array::from_fn(|i| input["coords"][i].as_u64().unwrap() as u16);
            let level = input["level"].as_i64().unwrap() as u8;
            // A deliberately admitted flat-cell component. It uses the real
            // theater/metadata/selector owners; it is not a native map-loader claim.
            let mut map = MapFile::from_bytes(assets.get_ref("XMP03T4.MAP").unwrap()).unwrap();
            // An admitted component cell needs a containing allocation;
            // unrelated authored cells and gameplay objects are not enrolled.
            map.header.width = u32::from(coords[0].max(coords[1]));
            map.header.height = map.header.width;
            map.cells = vec![MapCell {
                rx: coords[0],
                ry: coords[1],
                tile_index: 0,
                sub_tile: 0,
                z: level,
            }];
            map.iso_map_pack_lookups.clear();
            map.overlays.clear();
            map.terrain_objects.clear();
            let table = input
                .get("selector_table")
                .unwrap_or(&packet["inputs"]["selector_table"]);
            let table: [u8; 64] = std::array::from_fn(|i| table[i].as_u64().unwrap() as u8);
            let mut cache = TileVariantSelectorCache::initialized_for_test(table);
            let mut unexpected_main_draw =
                || panic!("warm process selector must not consume Main RNG");
            let mut selector = cache.begin_load(&mut unexpected_main_draw);
            let mut scenario_fill = |_low, _high| 0;
            let component_terrain = ResolvedTerrainGrid::build_with_variant_selector(
                &map,
                Some(&theater),
                Some(&assets),
                None,
                None,
                None,
                false,
                0,
                &mut scenario_fill,
                &mut selector,
            );
            let actual_map = input.get("full_view").is_some();
            let terrain = if actual_map {
                assert_eq!(scene.map.header.theater, "TEMPERATE");
                assert_eq!(profile(input), scene.sim().session.lighting.normal.into());
                scene.sim().resolved_terrain.as_ref().unwrap()
            } else {
                &component_terrain
            };
            let live = terrain.cell(coords[0], coords[1]).unwrap();
            assert_eq!(terrain.presentation_tile(live), (0, 0));
            assert_eq!(
                live.variant as u64,
                row["output"]["selected_file_index"].as_u64().unwrap()
            );
            assert_eq!(live.level, level);
            let mut grid = build_terrain_grid_from_resolved(terrain, None, None);
            grid.cells.retain(|cell| [cell.rx, cell.ry] == coords);
            assert_eq!(grid.cells.len(), 1);
            let lights = if actual_map {
                match_lighting.grid().clone()
            } else {
                build_cell_light_grid_from_heights_and_units(
                    [((coords[0], coords[1]), level)],
                    profile(input),
                )
            };
            let light = lights.cell_light_at((coords[0], coords[1])).unwrap();
            let native_rgb: [i32; 3] = std::array::from_fn(|i| {
                row["output"]["convert_rgb_key"][i].as_i64().unwrap() as i32
            });
            assert_eq!(light.rgb_key, native_rgb, "{} common Cell hue", row["name"]);
            assert_eq!(
                i64::from(light.top_scalar),
                row["output"]["top_scalar"].as_i64().unwrap()
            );
            assert_eq!(
                i64::from(light.bottom_scalar),
                row["output"]["bottom_scalar"].as_i64().unwrap()
            );
            assert_eq!(
                i64::from(light.common_scalar),
                row["output"]["common_scalar"].as_i64().unwrap()
            );
            let point = iso_to_screen(coords[0], coords[1], 0);
            let mut camera = camera(size);
            camera.camera_pos = [
                point.0 - input["cell_point"][0].as_i64().unwrap() as f32,
                point.1
                    - input["cell_point"][1].as_i64().unwrap() as f32
                    - input["viewport_y"].as_i64().unwrap() as f32,
            ];
            if let Some(view) = input.get("full_view") {
                // Native Tactical pixels and VERA's world pixels differ only
                // by the documented +15 Y bias at zoom1. Use the independent
                // caller camera, rather than absorbing a projection error.
                camera.camera_pos = [
                    view["view_origin"][0].as_i64().unwrap() as f32,
                    view["view_origin"][1].as_i64().unwrap() as f32 + 15.0,
                ];
                assert_eq!(
                    [
                        point.0 - camera.camera_pos[0],
                        point.1 - camera.camera_pos[1]
                    ],
                    [
                        input["cell_point"][0].as_i64().unwrap() as f32,
                        input["cell_point"][1].as_i64().unwrap() as f32
                    ],
                    "original full-view projection reaches the same tile point"
                );
            }
            let lookup = |tile_id, sub_tile, variant| {
                atlas
                    .get_uv(TileKey {
                        tile_id,
                        sub_tile,
                        variant,
                    })
                    .map(|uv| TilePlacement {
                        uv_origin: uv.uv_origin,
                        uv_size: uv.uv_size,
                        pixel_size: uv.pixel_size,
                        draw_offset: uv.draw_offset,
                    })
            };
            let instances = crate::render::terrain_instances::build_visible_instances(
                &grid,
                Some(&lights),
                camera.camera_pos[0],
                camera.camera_pos[1],
                size[0] as f32,
                size[1] as f32,
                Some(&lookup),
                Some(terrain),
            );
            assert_eq!(instances.normal.len(), 1);
            pool.upload_on_device(
                &gpu.device,
                &gpu.queue,
                "tmp-clear-native",
                &instances.normal,
            );
            let a = ShroudBuffer::fixture(
                &gpu.device,
                &gpu.queue,
                size,
                camera.camera_pos,
                &a_pixels(input, size),
            );
            assert_tmp_raster(
                &gpu, &mut batch, &pool, &atlas, &a, camera, row, format, size,
            );
        }
    }
}
