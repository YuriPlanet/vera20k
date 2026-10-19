//! Physical TMP through the production decoder, atlas, live cell
//! selection, common lighting, A binding, upload and ordinary terrain draw.
//! Expected RGB565 and stored Z are captured by original Cell480350/TMP547CF0.
use super::draw_pooled_zdepth;
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::map::lighting::{LightingProfileUnits, build_cell_light_grid_from_heights_and_units};
use crate::map::map_file::{MapCell, MapFile};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::terrain::{TilePlacement, build_terrain_grid_from_resolved, iso_to_screen};
use crate::map::theater::{TheaterData, TileImage, TileKey, load_theater, load_tile_images};
use crate::map::tile_variant_selector::TileVariantSelectorCache;
use crate::render::batch::{BatchRenderer, CameraUniform, InstanceBufferPool};
use crate::render::shroud_buffer::ShroudBuffer;
use crate::render::terrain_draw_gpu_tests::{
    Gpu, assert_native_color_and_depth, camera, clear, encoded, extent,
};
use crate::render::tile_atlas::TileAtlas;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
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

/// The paid opening and admitted Deploy commands run in the shared scenario
/// visitor. MatchLighting consumes every ordinary frame publication, without
/// reconstructing sources after installation. Native source/Cell/TMP evidence:
/// RadSite65B580/65B800 -> LightSource554AF0 -> Cell483E30 -> TMP547CF0.
/// Native selector-input controls cover each physical material; comparison
/// follows the ordinary World's variant without overriding its selector.
#[test]
#[ignore = "requires retail assets and GPU; paid DESO lifecycle through ordered lighting events and native TMP pixels"]
fn paid_deso_light_lifecycle_reaches_native_cell_and_tmp_pixels() {
    assert_paid_deso_light_lifecycle(
        "tools/spatial_oracle/radiation_light_lifecycle.json",
        "tools/map_observation.radiation-light.example.json",
    );
}

#[test]
#[ignore = "requires retail assets and GPU; paid Snow DESO lifecycle through ordered lighting events and native TMP pixels"]
fn paid_snow_deso_light_lifecycle_reaches_native_cell_and_tmp_pixels() {
    assert_paid_deso_light_lifecycle(
        "tools/spatial_oracle/radiation_snow_light_lifecycle.json",
        "tools/map_observation.radiation-snow.example.json",
    );
}

fn assert_paid_deso_light_lifecycle(packet_path: &'static str, profile_path: &'static str) {
    use crate::app::presentation::lighting::MatchLighting;
    use crate::headless_scenario::radiation_test_evidence::visit_paid_deso_light_lifecycle;

    let packet = crate::test_fixture::json(packet_path);
    let history = packet["histories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|history| history["name"] == "single_lifetime")
        .unwrap();
    let setup = &history["input"]["setup"];
    let coords: [u16; 2] =
        std::array::from_fn(|i| history["input"]["center"][i].as_u64().unwrap() as u16);
    let physical = setup["geometry"]["allocated"]
        .as_array()
        .unwrap()
        .iter()
        .find(|cell| cell["cell"] == serde_json::json!(coords))
        .unwrap();
    let tile_id = physical["tile"].as_u64().unwrap() as u16;
    let sub_tile = physical["subtile"].as_u64().unwrap() as u8;
    let root = PathBuf::from(std::env::var_os("RA2_DIR").expect("physical retail root"));
    let mut assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let theater_name = setup["theater"]["source_value"].as_str().unwrap();
    let theater = load_theater(&mut assets, theater_name).unwrap();
    let extension = theater.extension;
    let mapping = setup["tmp"]["mappings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|mapping| mapping["tile_id"].as_u64() == Some(u64::from(tile_id)))
        .unwrap();
    let native_filename = format!(
        "{}{:02}.{extension}",
        mapping["file_name"].as_str().unwrap(),
        u64::from(tile_id) - mapping["base"].as_u64().unwrap() + 1
    );
    assert!(
        theater
            .lookup
            .filename(i32::from(tile_id))
            .unwrap()
            .eq_ignore_ascii_case(&native_filename),
        "production binding must select the original theater reader's material"
    );
    for source in setup["sources"].as_array().unwrap() {
        let name = source["name"].as_str().unwrap();
        assert_eq!(
            crate::util::sha256::sha256_hex(assets.get_ref(name).unwrap()),
            source["sha256"].as_str().unwrap(),
            "native/production physical input {name}"
        );
    }
    for phase in ["baseline", "activated", "first_light_decay", "expired"] {
        let mut variants: Vec<_> = history["rasters"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["phase"] == phase)
            .map(|row| row["material"]["file_index"].as_u64().unwrap())
            .collect();
        variants.sort_unstable();
        assert_eq!(
            variants,
            (0..u64::from(theater.lookup.total_file_count(tile_id))).collect::<Vec<_>>(),
            "{phase}: original draws must cover the complete physical family"
        );
    }
    let tiles = load_tile_images(
        &assets,
        &theater.lookup,
        &theater.iso_palette,
        &HashSet::from([TileKey {
            tile_id,
            sub_tile,
            variant: 0,
        }]),
    );
    assert!(!tiles.is_empty());
    let shp = crate::assets::shp_file::ShpFile::from_bytes(assets.get_ref("SHROUD.SHP").unwrap())
        .unwrap();
    let (frames, frame_w, frame_h) = crate::render::shroud_buffer::extract_shp_brightness(&shp);
    let size: [u32; 2] = std::array::from_fn(|i| {
        packet["surface"][if i == 0 { "width" } else { "height" }]
            .as_u64()
            .unwrap() as u32
    });
    let gpu = Gpu::new();
    let mut renderers = [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ]
    .map(|format| {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let atlas = crate::render::tile_atlas::build_atlas(&gpu.device, &gpu.queue, &batch, &tiles);
        (format, batch, atlas, InstanceBufferPool::new())
    });
    let mut lighting = MatchLighting::default();
    let mut a = None;
    let mut compared = Vec::new();
    visit_paid_deso_light_lifecycle(profile_path, |phase, scene, output| {
        if output.is_none() {
            assert_eq!(phase, Some("loaded"));
            assert_eq!(scene.sim().session.theater, theater_name);
            assert!(scene.sim().radiation.is_empty());
            lighting.install(
                crate::map::lighting::CellLightGrid::new(),
                crate::map::lighting::parse_lighting(&scene.map.ini),
                2,
                Some((scene.sim().resolved_terrain.as_ref().unwrap(), scene.sim())),
            );
            a = Some(ShroudBuffer::new(
                &gpu.device,
                &gpu.queue,
                size,
                [scene.sim().fog.width, scene.sim().fog.height],
                frames.clone(),
                [frame_w, frame_h],
                crate::render::shroud_buffer::SHROUD_EDGE_LUT,
            ));
        } else {
            lighting.apply_events(
                scene.sim().resolved_terrain.as_ref().unwrap(),
                &output.unwrap().lighting_events,
            );
        }
        let Some(phase) = phase else {
            return;
        };
        let name = match phase {
            "loaded" => "baseline",
            "active" => "activated",
            "faded" => "first_light_decay",
            "expired" | "settled" => "expired",
            other => panic!("unexpected paid radiation phase {other}"),
        };
        let hash = scene.sim().state_hash();
        assert!(
            scene
                .runtime
                .simulation
                .prepare_fog_view_for("VERA-OBSERVER")
        );
        let terrain = scene.sim().resolved_terrain.as_ref().unwrap();
        let live = terrain.cell(coords[0], coords[1]).unwrap();
        assert_eq!(terrain.presentation_tile(live), (tile_id, sub_tile));
        assert_eq!(u64::from(live.level), physical["level"].as_u64().unwrap());
        assert_eq!(
            u64::from(live.slope_type),
            physical["slope"].as_u64().unwrap()
        );
        let rows: Vec<_> = history["rasters"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| {
                row["phase"] == name
                    && row["material"]["file_index"].as_u64() == Some(u64::from(live.variant))
            })
            .collect();
        assert_eq!(rows.len(), 1, "{phase}: native physical variant coverage");
        let row = rows[0];
        let input = &row["input"];
        assert!(
            theater
                .lookup
                .filename_for_variant(tile_id, live.variant)
                .unwrap()
                .eq_ignore_ascii_case(row["material"]["filename"].as_str().unwrap()),
            "{phase}: production/native selected physical material"
        );
        assert_eq!(profile(input), scene.sim().session.lighting.normal.into());
        let light = lighting
            .grid()
            .cell_light_at((coords[0], coords[1]))
            .unwrap();
        assert_eq!(
            serde_json::json!(light.rgb_key),
            row["output"]["convert_rgb_key"],
            "{phase}"
        );
        for (actual, key) in [
            (light.top_scalar, "top_scalar"),
            (light.bottom_scalar, "bottom_scalar"),
            (light.common_scalar, "common_scalar"),
        ] {
            assert_eq!(
                i64::from(actual),
                row["output"][key].as_i64().unwrap(),
                "{phase} {key}"
            );
        }
        // The oracle declares a component draw point, rather than a Tactical
        // camera. Use that same explicit seam and the existing projection.
        assert!(input.get("full_view").is_none());
        let point = iso_to_screen(coords[0], coords[1], 0);
        let mut camera = camera(size);
        camera.camera_pos = [
            point.0 - input["cell_point"][0].as_i64().unwrap() as f32,
            point.1
                - input["cell_point"][1].as_i64().unwrap() as f32
                - input["viewport_y"].as_i64().unwrap() as f32,
        ];
        let a = a.as_mut().unwrap();
        a.rebuild_if_needed(
            &gpu.device,
            &gpu.queue,
            &scene.sim().fog,
            scene.sim().session.current_house.unwrap(),
            camera.camera_pos[0],
            camera.camera_pos[1],
            size[0],
            size[1],
            1.0,
        );
        let actual_a: Vec<_> = gpu
            .read_float_texels(a.gpu_source().0, [0, 0], size)
            .into_iter()
            .map(|pixel| (pixel[0] * 255.0).round() as u8)
            .collect();
        assert_eq!(actual_a, a_pixels(input, size), "{phase}: actual live A");
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
            assert_tmp_raster(&gpu, batch, pool, atlas, a, camera, row, *format, size);
        }
        assert_eq!(
            scene.sim().state_hash(),
            hash,
            "{phase}: presentation is read-only"
        );
        compared.push(phase.to_owned());
    });
    assert_eq!(
        compared,
        ["loaded", "active", "faded", "expired", "settled"]
    );
}

/// Normal Battle construction supplies all sight and Cell knowledge. The native
/// component independently constructs AMCV on physical GoldSt terrain; its eight
/// TMP variants share one live A plane, so the normal selector needs no override.
#[test]
#[ignore = "requires retail assets and GPU; normal AMCV source through live shroud upload and original TMP frontier pixels"]
fn retail_amcv_sight_reaches_the_live_shroud_and_tmp_raster() {
    assert_retail_amcv_sight_raster(false);
}

#[test]
#[ignore = "requires retail assets and GPU; ordinary moving AMCV through native arrival shroud and physical TMP pixels"]
fn moving_amcv_sight_reaches_the_live_shroud_and_tmp_raster() {
    assert_retail_amcv_sight_raster(true);
}

fn assert_retail_amcv_sight_raster(moving: bool) {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::movement::track_process::TrackFamily;
    use crate::sim::world::{FrameEffects, TickLane};
    use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};

    let packet: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/shroud_frontier.json",
    ))
    .unwrap();
    let moving_packet = moving.then(|| {
        crate::test_fixture::json("tools/procedural_drawing_oracle/shroud_frontier_moving.json")
    });
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
        Some((scene.sim().resolved_terrain.as_ref().unwrap(), scene.sim())),
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
    for step in 0..=if moving { 4 } else { 3 } {
        if moving && step > 0 {
            let command = match step {
                1 => Some(Command::Move {
                    entity_id: source_id,
                    target_rx: 111,
                    target_ry: 27,
                }),
                3 => Some(Command::Stop {
                    entity_id: source_id,
                }),
                _ => None,
            }
            .map(|command| CommandEnvelope::new(owner, scene.sim().session.tick + 1, command));
            let mut reached = false;
            for frame in 0..if step >= 3 { 8 } else { 160 } {
                let commands = if frame == 0 { command.as_slice() } else { &[] };
                scene
                    .runtime
                    .advance_frame(commands, 22, TickLane::Ordinary, FrameEffects::default())
                    .expect("ordinary Move/Stop frame");
                let entity = scene.sim().entities().get(source_id).unwrap();
                let loco = entity.locomotor.as_ref().unwrap();
                reached = match step {
                    // The next head is published by actual Unit arrival,
                    // after the physical cell crossing. No pose is injected.
                    1 => {
                        (entity.position.rx(), entity.position.ry()) == (110, 27)
                            && loco
                                .track_head(TrackFamily::Drive)
                                .is_some_and(|head| head.x == 28544)
                    }
                    2 => {
                        (entity.position.rx(), entity.position.ry()) == (111, 27)
                            && loco.track_head(TrackFamily::Drive).is_none()
                            && loco.track_destination(TrackFamily::Drive).is_none()
                    }
                    _ => frame == 7,
                };
                if reached {
                    break;
                }
            }
            assert!(
                reached,
                "moving shroud phase{step} did not reach its ordinary event"
            );
        } else if (1..=2).contains(&step) {
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
        let native_history = if step == 1 { "arrival_1" } else { "arrival_2" };
        let moving_history = moving_packet.filter(|_| step > 0).map(|packet| {
            packet["histories"]
                .as_array()
                .unwrap()
                .iter()
                .find(|history| history["name"] == native_history)
                .unwrap()
        });
        assert_eq!(
            &serde_json::json!(xyz),
            moving_history.map_or(&packet["inputs"]["source"]["native_xyz"], |history| {
                &history["state"]["actor"]["position"]
            })
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
        let rows = moving_packet.filter(|_| step > 0).unwrap_or(&packet)["rows"]
            .as_array()
            .unwrap();
        assert_eq!(
            rows.iter()
                .filter(|row| !moving || step == 0 || row["history"] == native_history)
                .count(),
            8
        );
        let row = rows
            .iter()
            .find(|row| {
                row["output"]["selected_file_index"].as_u64() == Some(u64::from(live.variant))
                    && (!moving || step == 0 || row["history"] == native_history)
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
        let moving_a = moving_history.map(|history| {
            a_pixels(
                &serde_json::json!({"a_hex": history["output"]["alpha_hex"]}),
                size,
            )
        });
        assert_eq!(
            &actual_a,
            moving_a
                .as_ref()
                .unwrap_or_else(|| if step == 3 { &uncached_a } else { &expected_a }),
            "phase{step}: original full A after starting sight or explicit Action40 restoration"
        );
        if !moving || step == 0 {
            assert!(row["output"]["sampled_a_values"].as_array().unwrap().len() > 1);
        }
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
    assert_tmp_pass(
        gpu,
        batch,
        pool,
        atlas,
        (&color, &depth),
        row,
        format,
        size,
        false,
    );
}

/// The retained-surface comparison and existing leaf controls share the same
/// terrain submission/readback responsibility. Only the actual App pass seam
/// differs: frame reconstruction clears, while individual TMP controls load.
fn assert_tmp_pass(
    gpu: &Gpu,
    batch: &BatchRenderer,
    pool: &InstanceBufferPool,
    atlas: &TileAtlas,
    targets: (&wgpu::Texture, &wgpu::Texture),
    row: &Value,
    format: wgpu::TextureFormat,
    size: [u32; 2],
    reconstruct: bool,
) {
    let (color, depth) = targets;
    let cv = color.create_view(&Default::default());
    let dv = depth.create_view(&Default::default());
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    if !reconstruct {
        clear(
            &mut encoder,
            &cv,
            &dv,
            row["input"]["old_z"].as_u64().unwrap() as u16,
            wgpu::LoadOp::Load,
        );
    }
    {
        let mut pass = if reconstruct {
            crate::app::presentation::sidebar_render::begin_main_pass(&mut encoder, &cv, &dv)
        } else {
            crate::app::presentation::sidebar_render::begin_main_load_pass(&mut encoder, &cv, &dv)
        };
        let input = &row["input"];
        let clip: [u32; 4] = std::array::from_fn(|i| input["clip"][i].as_u64().unwrap() as u32);
        pass.set_scissor_rect(clip[0], clip[1], clip[2], clip[3]);
        for _ in 0..input["repeat"].as_u64().unwrap() {
            draw_pooled_zdepth(&mut pass, batch, pool, Some(atlas), "tmp-clear-native");
        }
    }
    let reads = [gpu.read(&mut encoder, color), gpu.read(&mut encoder, depth)];
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
    assert_retail_tmp_rasters(&packet, "clear01.tem", 0);
}

#[test]
#[ignore = "requires retail assets and GPU; original Cell480350 cliff diamond and extra face through production terrain owners"]
fn retail_cliff_extra_matches_original_cell_raster() {
    let packet: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/cliff_render.json",
    ))
    .unwrap();
    let retail = &packet["rows"][0];
    assert_eq!(retail["input"]["coords"], serde_json::json!([18, 69]));
    assert_eq!(retail["input"]["level"], 4);
    assert!(retail["input"].get("full_view").is_some());
    let hills = packet["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["scene_map"] == "Hills.mmx")
        .expect("the normal App-visible Hills cliff has original coverage");
    assert_eq!(hills["input"]["coords"], serde_json::json!([103, 64]));
    assert_eq!(hills["input"]["level"], 2);
    assert!(hills["input"].get("full_view").is_some());
    assert_retail_tmp_rasters(&packet, "cliff01.tem", 2);
}

/// Keep the original TMP controls on one production decoder/atlas/light/pool
/// path. Only the selected physical file and subtile differ between fixtures.
struct RetailTmpMaterials {
    root: PathBuf,
    assets: AssetManager,
    theater: TheaterData,
    tile_id: u16,
    tiles: HashMap<TileKey, TileImage>,
}

/// Bind the same physical theater/material closure for individual TMP leaves
/// and the retained-surface terrain continuation.
fn retail_tmp_materials(packet: &Value, filename: &str, sub_tile: u8) -> RetailTmpMaterials {
    let root = PathBuf::from(std::env::var_os("RA2_DIR").expect("physical retail root"));
    let mut assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let theater = load_theater(&mut assets, "TEMPERATE").unwrap();
    let tile_id = (0..theater.lookup.len())
        .find(|index| {
            theater
                .lookup
                .filename(*index as i32)
                .is_some_and(|name| name.eq_ignore_ascii_case(filename))
        })
        .expect("stock theater must resolve the selected physical TMP") as u16;
    if filename == "clear01.tem" {
        assert_eq!(theater.rmg_tiles.clear_tile, Some(tile_id));
        assert_eq!(tile_id, 0);
        assert_eq!(theater.lookup.total_file_count(tile_id), 8);
    }
    if let Some(binding) = packet["inputs"].get("binding") {
        assert_eq!(u64::from(tile_id), binding["mapping"]["tile_id"]);
        assert_eq!(filename, binding["name"]);
        assert_eq!(
            u64::from(theater.lookup.total_file_count(tile_id)),
            binding["file_count"]
        );
    }
    for source in packet["inputs"]["sources"].as_array().unwrap() {
        let name = source["name"].as_str().unwrap();
        if let Some(outer) = source["outer"]
            .as_str()
            .filter(|name| name.ends_with(".mmx"))
        {
            // The normal Hills source is a physical MIX wrapper. Its inner
            // map is not a directly registered AssetManager file.
            let bytes = std::fs::read(root.join(outer)).unwrap();
            assert_eq!(
                crate::util::sha256::sha256_hex(&bytes),
                source["outer_sha256"]
            );
            let wrapper = crate::assets::mix_archive::MixArchive::from_bytes(bytes).unwrap();
            assert_eq!(
                crate::util::sha256::sha256_hex(wrapper.get_by_name(name).unwrap()),
                source["sha256"],
                "physical map entry {name}"
            );
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if !(lower.ends_with(".tem")
            || matches!(
                lower.as_str(),
                "isotem.pal" | "temperat.pal" | "temperatmd.ini" | "xmp03t4.map" | "shroud.shp"
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
            tile_id,
            sub_tile,
            variant: 0,
        }]),
    );
    assert_eq!(
        tiles.len(),
        usize::from(theater.lookup.total_file_count(tile_id))
    );
    RetailTmpMaterials {
        root,
        assets,
        theater,
        tile_id,
        tiles,
    }
}

fn assert_retail_tmp_rasters(packet: &Value, filename: &str, sub_tile: u8) {
    let RetailTmpMaterials {
        root,
        assets,
        theater,
        tile_id,
        tiles,
    } = retail_tmp_materials(packet, filename, sub_tile);
    if let Some(planes) = packet["rows"][0]["output"].get("plane_writes") {
        // The original two passes have no shared written destination in this
        // physical subtile. Compare their actual union with decoded coverage;
        // arbitrary overlapping Extra/diamond data remains a separate path.
        assert_eq!(planes["overlap_pixels"], 0);
        let tile = &tiles[&TileKey {
            tile_id,
            sub_tile,
            variant: 0,
        }];
        let opaque = tile.rgba.chunks_exact(4).filter(|p| p[3] != 0).count();
        assert_eq!(
            opaque as u64,
            planes["diamond_pixels"].as_u64().unwrap() + planes["extra_pixels"].as_u64().unwrap()
        );
    }
    // Each physical map crosses the normal bound scenario loader once, then
    // the App's live first-frame lighting publication owner. Component rows
    // remain explicitly separate from these immutable normal-map witnesses.
    let mut scenes = BTreeMap::new();
    for row in packet["rows"].as_array().unwrap() {
        let name = row["input"]["scene_map"].as_str().unwrap_or("XMP03T4.MAP");
        scenes.entry(name.to_owned()).or_insert_with(|| {
            let scene = crate::headless_scenario::load(&root, name, 0).unwrap();
            let hash = scene.sim().state_hash();
            let mut lighting = crate::app::presentation::lighting::MatchLighting::default();
            lighting.install(
                crate::map::lighting::CellLightGrid::new(),
                crate::map::lighting::parse_lighting(&scene.map.ini),
                2,
                Some((scene.sim().resolved_terrain.as_ref().unwrap(), scene.sim())),
            );
            (scene, lighting, hash)
        });
    }
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
            let scene_name = input["scene_map"].as_str().unwrap_or("XMP03T4.MAP");
            let (scene, match_lighting, _) = &scenes[scene_name];
            let coords: [u16; 2] =
                std::array::from_fn(|i| input["coords"][i].as_u64().unwrap() as u16);
            let level = input["level"].as_i64().unwrap() as u8;
            let actual_map = input.get("full_view").is_some();
            let component_terrain = (!actual_map).then(|| {
                // A declared one-cell component uses real metadata/selector
                // owners, separately from the normal loaded-map witness.
                let mut map = MapFile::from_bytes(assets.get_ref("XMP03T4.MAP").unwrap()).unwrap();
                map.header.width = u32::from(coords[0].max(coords[1]));
                map.header.height = map.header.width;
                map.cells = vec![MapCell {
                    rx: coords[0],
                    ry: coords[1],
                    tile_index: i32::from(tile_id),
                    sub_tile,
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
                ResolvedTerrainGrid::build_with_variant_selector(
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
                )
            });
            let terrain = if actual_map {
                assert_eq!(scene.map.header.theater, "TEMPERATE");
                assert_eq!(profile(input), scene.sim().session.lighting.normal.into());
                scene.sim().resolved_terrain.as_ref().unwrap()
            } else {
                component_terrain.as_ref().unwrap()
            };
            let live = terrain.cell(coords[0], coords[1]).unwrap();
            assert_eq!(terrain.presentation_tile(live), (tile_id, sub_tile));
            if let Some(selected) = row["output"]["selected_file_index"].as_u64() {
                assert_eq!(u64::from(live.variant), selected);
            } else {
                // The original full-view away control never calls this Cell.
                // It supplies no invented image selection or projected point.
                assert!(input.get("full_view").is_some());
                assert!(row["draws"].as_array().unwrap().is_empty());
                assert!(row["output"]["runs"].as_array().unwrap().is_empty());
            }
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
            if let Some(view) = input.get("full_view") {
                // Native Tactical pixels and VERA's world pixels differ only
                // by the documented +15 Y bias at zoom1. Use the independent
                // caller camera, rather than absorbing a projection error.
                camera.camera_pos = [
                    view["view_origin"][0].as_i64().unwrap() as f32,
                    view["view_origin"][1].as_i64().unwrap() as f32 + 15.0,
                ];
                if input["cell_point"].is_array() {
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
            } else {
                camera.camera_pos = [
                    point.0 - input["cell_point"][0].as_i64().unwrap() as f32,
                    point.1
                        - input["cell_point"][1].as_i64().unwrap() as f32
                        - input["viewport_y"].as_i64().unwrap() as f32,
                ];
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
            if row["output"]["selected_file_index"].as_u64().is_some() {
                assert_eq!(instances.normal.len(), 1);
            }
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
    for (scene, _, hash) in scenes.values() {
        assert_eq!(scene.sim().state_hash(), *hash);
    }
}

/// Original dirty enqueue6DA7D0 -> maintenance6D2B60/Cell480180 -> Shroud
/// 6D3660 -> terrain6D2DE0/Cell480350 -> isolated cleanup6D4571/6D4B37,
/// on retained nonuniform RGB/Z/A.
/// This compares the existing App full reconstruction with the native joined
/// destination, not with a clear-only or a hand-computed replacement.
///
/// The native component explicitly supplies its registered flat cells,
/// Tactical camera, warm selector, and SHROUD-produced A. Rust consumes those
/// inputs through the ordinary material/terrain/instance owners. It does not
/// establish map/bootstrap or reveal lifecycle equivalence, arbitrary terrain,
/// custom/no-Z paths, or equivalence of the intermediate maintenance Z plane.
#[test]
#[ignore = "requires retail assets and GPU; original joined dirty-cell histories against retained App reconstruction"]
fn retail_partial_redraw_continuation_matches_full_reconstruction() {
    let packet = crate::test_fixture::json("tools/spatial_oracle/tmp_partial_redraw.json");
    assert_eq!(packet["schema_version"], 2);
    assert_eq!(packet["original_executable_sections_unchanged"], true);
    let RetailTmpMaterials {
        assets,
        theater,
        tile_id,
        tiles,
        ..
    } = retail_tmp_materials(&packet, "clear01.tem", 0);
    let scene = &packet["scene"];
    assert_eq!(scene["every_draw_receiver_real"], true);
    assert_eq!(
        scene["registration_bounds"],
        serde_json::json!([0, 0, 32, 32])
    );
    let inputs = scene["cells"].as_array().unwrap();
    assert_eq!(
        inputs.len(),
        1024,
        "all enrolled native cells are scene inputs"
    );
    let mut map = MapFile::from_bytes(assets.get_ref("XMP03T4.MAP").unwrap()).unwrap();
    map.header.width = scene["map_size"][0].as_u64().unwrap() as u32;
    map.header.height = scene["map_size"][1].as_u64().unwrap() as u32;
    map.header.fill = "Clear".into();
    map.header.level = 0;
    map.header.local_left = 0;
    map.header.local_top = 0;
    map.header.local_width = map.header.width;
    map.header.local_height = map.header.height;
    map.cells = inputs
        .iter()
        .map(|cell| MapCell {
            rx: cell["coords"][0].as_u64().unwrap() as u16,
            ry: cell["coords"][1].as_u64().unwrap() as u16,
            tile_index: cell["tile_id"].as_i64().unwrap() as i32,
            sub_tile: cell["sub_tile"].as_u64().unwrap() as u8,
            z: cell["level"].as_u64().unwrap() as u8,
        })
        .collect();
    let coords: HashSet<_> = map.cells.iter().map(|cell| (cell.rx, cell.ry)).collect();
    assert_eq!(coords.len(), inputs.len());
    for y in 0..32 {
        for x in 0..32 {
            assert!(
                coords.contains(&(x, y)),
                "complete declared component rectangle"
            );
        }
    }
    // This is an explicit component map, not XMP03T4's authored map load.
    // Retain the native Size16 admission instead of making a Size32 map or
    // selecting only the Cells the native observer happened to draw.
    map.iso_map_pack_lookups.clear();
    map.entities.clear();
    map.overlays.clear();
    map.smudges.clear();
    map.terrain_objects.clear();
    let table = &scene["selector_table"];
    let table = std::array::from_fn(|i| table[i].as_u64().unwrap() as u8);
    let mut cache = TileVariantSelectorCache::initialized_for_test(table);
    let mut unexpected_main_draw = || panic!("retained warm selector must not consume Main RNG");
    let mut selector = cache.begin_load(&mut unexpected_main_draw);
    let mut scenario_fill = |low, high| {
        assert_eq!((low, high), (0, 0), "declared flat Clear fill");
        0
    };
    let terrain = ResolvedTerrainGrid::build_with_variant_selector(
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
    for cell in terrain.iter() {
        assert!(coords.contains(&(cell.rx, cell.ry)));
        assert_eq!(terrain.presentation_tile(cell), (tile_id, 0));
        assert_eq!(cell.level, 0);
    }
    // Calls are admission witnesses only. They never choose Rust membership,
    // variant selection, culling, or instance order.
    for history in packet["histories"].as_array().unwrap() {
        for call in history["calls"].as_array().unwrap() {
            if let Some(cell) = call["cell"].as_array() {
                let x = cell[0].as_u64().unwrap() as u16;
                let y = cell[1].as_u64().unwrap() as u16;
                assert!(coords.contains(&(x, y)));
                assert!(
                    terrain.cell(x, y).is_some(),
                    "original drawn Cell passes canonical admission"
                );
            }
        }
    }
    let mut sim = crate::sim::world::Simulation::with_seed(0x480180);
    sim.install_playfield_from_map_header(&map.header);
    sim.install_resolved_terrain_for_new_map(terrain);
    let hash = sim.state_hash();
    let rng = sim.rng_state();
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let grid = build_terrain_grid_from_resolved(terrain, None, None);
    let lights = build_cell_light_grid_from_heights_and_units(
        terrain.iter().map(|cell| ((cell.rx, cell.ry), cell.level)),
        profile(&serde_json::json!({"scene_lighting": scene["lighting"]})),
    );
    let size = [
        packet["surface"]["width"].as_u64().unwrap() as u32,
        packet["surface"]["height"].as_u64().unwrap() as u32,
    ];
    // begin_main_pass's actual clear agrees with this native component premise.
    assert_eq!(packet["surface"]["background"], 0);
    assert_eq!(packet["surface"]["old_z"], 65535);
    let mut camera = camera(size);
    camera.camera_pos = [
        scene["camera"][0].as_i64().unwrap() as f32,
        // Existing production coordinate convention; no capture fitting.
        scene["camera"][1].as_i64().unwrap() as f32 + 15.0,
    ];
    let gpu = Gpu::new();
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let atlas = crate::render::tile_atlas::build_atlas(&gpu.device, &gpu.queue, &batch, &tiles);
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
        assert!(!instances.normal.is_empty());
        let mut pool = InstanceBufferPool::new();
        pool.upload_on_device(
            &gpu.device,
            &gpu.queue,
            "tmp-clear-native",
            &instances.normal,
        );
        // Reuse the actual attachments across prior and reconstructed phases;
        // no native expected pixels are uploaded as the prior RGB/Z image.
        let color = gpu.target(size, format);
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        for history in packet["histories"].as_array().unwrap() {
            let phases = history["phases"].as_array().unwrap();
            let prior = &phases[0];
            let native_rng = |phase: &Value, stream: &str| {
                let index = phase["rng"][stream].as_u64().unwrap() as usize;
                packet["rng_streams"][index].as_str().unwrap()
            };
            assert!(history["rng_requests"].as_array().unwrap().is_empty());
            for phase in phases {
                for stream in ["main", "scenario", "mapgen"] {
                    assert_eq!(native_rng(phase, stream), native_rng(prior, stream));
                }
                assert_eq!(phase["alpha_ref"], prior["alpha_ref"]);
            }
            let alpha = &packet["alpha_planes"][prior["alpha_ref"].as_u64().unwrap() as usize];
            let pixels = a_pixels(&serde_json::json!({"a_hex": alpha["hex"]}), size);
            assert_eq!(pixels.len(), (size[0] * size[1]) as usize);
            assert_eq!(crate::util::sha256::sha256_hex(&pixels), alpha["sha256"]);
            assert!(
                pixels.windows(2).any(|pair| pair[0] != pair[1]),
                "native A is nonuniform"
            );
            let a =
                ShroudBuffer::fixture(&gpu.device, &gpu.queue, size, camera.camera_pos, &pixels);
            batch.bind_shroud(&gpu.device, Some(&a));
            batch.write_camera(&gpu.queue, camera);
            for phase in phases.iter().filter(|phase| {
                matches!(
                    phase["name"].as_str().unwrap(),
                    "prior" | "after_terrain" | "after_cleanup"
                )
            }) {
                let state =
                    &packet["surface_states"][phase["surface_ref"].as_u64().unwrap() as usize];
                assert!(
                    state["runs"].is_array(),
                    "complete native destination is required"
                );
                let row = serde_json::json!({
                    "name": format!("{}:{}", history["name"].as_str().unwrap(), phase["name"].as_str().unwrap()),
                    "input": {
                        "background": packet["surface"]["background"],
                        "old_z": packet["surface"]["old_z"],
                        "clip": [0, 0, size[0], size[1]], "repeat": 1,
                    },
                    "output": state,
                });
                assert_tmp_pass(
                    &gpu,
                    &batch,
                    &pool,
                    &atlas,
                    (&color, &depth),
                    &row,
                    format,
                    size,
                    true,
                );
                let actual_a: Vec<_> = gpu
                    .read_float_texels(a.gpu_source().0, [0, 0], size)
                    .into_iter()
                    .map(|pixel| (pixel[0] * 255.0).round() as u8)
                    .collect();
                assert_eq!(
                    actual_a, pixels,
                    "terrain reconstruction preserves supplied native A"
                );
                assert_eq!(
                    sim.state_hash(),
                    hash,
                    "presentation preserves actual simulation"
                );
                assert_eq!(
                    sim.rng_state(),
                    rng,
                    "presentation preserves all three actual RNGs"
                );
            }
        }
    }
}
