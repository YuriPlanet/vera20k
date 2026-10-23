//! Physical CRATER11 through the ordinary atlas, instance, pool and world blit.
//! Original Cell480350/Smudge6B55F0 captures supply the expected color/Z stores.
use super::build_visible_instances;
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::map::lighting::{LightingProfileUnits, build_cell_light_grid_from_heights_and_units};
use crate::map::map_file::MapSmudgeEntry;
use crate::map::terrain::{TilePlacement, iso_to_screen};
use crate::render::batch::{BatchRenderer, CameraUniform, InstanceBufferPool};
use crate::render::overlay_atlas::{OverlayAtlas, build_overlay_atlas_on_device, smudge_key};
use crate::render::shroud_buffer::ShroudBuffer;
use crate::render::terrain_draw_gpu_tests::{
    Gpu, assert_native_color_and_depth, camera, clear, encoded, extent,
};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::smudge_grid::SmudgeGrid;
use serde_json::Value;

fn coords(value: &Value) -> (u16, u16) {
    (
        value[0].as_u64().unwrap() as u16,
        value[1].as_u64().unwrap() as u16,
    )
}

fn scene_profile(input: &Value) -> LightingProfileUnits {
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

fn view(row: &Value, size: [u32; 2]) -> CameraUniform {
    let input = &row["input"];
    let (rx, ry) = coords(&input["coords"]);
    let point = iso_to_screen(rx, ry, 0);
    let mut camera = camera(size);
    camera.camera_pos = [
        point.0 - input["cell_point"][0].as_i64().unwrap() as f32,
        point.1
            - input["cell_point"][1].as_i64().unwrap() as f32
            - input["viewport_y"].as_i64().unwrap() as f32,
    ];
    if let Some(full) = input.get("full_view") {
        // Original Tactical versus VERA world projection has the shared +15Y
        // bias. The native camera is independently captured, not fitted here.
        camera.camera_pos = [
            full["view_origin"][0].as_i64().unwrap() as f32,
            full["view_origin"][1].as_i64().unwrap() as f32 + 15.0,
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
        );
    }
    camera
}

fn read_raster(
    gpu: &Gpu,
    batch: &mut BatchRenderer,
    atlas: &OverlayAtlas,
    instances: &[crate::render::batch::SpriteInstance],
    row: &Value,
    format: wgpu::TextureFormat,
    size: [u32; 2],
) -> Vec<Vec<u8>> {
    let input = &row["input"];
    let camera = view(row, size);
    batch.write_camera(&gpu.queue, camera);
    let a: Vec<_> = if let Some(hex) = input["a_hex"].as_str() {
        hex.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect()
    } else {
        vec![input["a"].as_u64().unwrap() as u8; (size[0] * size[1]) as usize]
    };
    assert_eq!(a.len(), (size[0] * size[1]) as usize);
    let a = ShroudBuffer::fixture(&gpu.device, &gpu.queue, size, camera.camera_pos, &a);
    batch.bind_shroud(&gpu.device, Some(&a));
    let mut pool = InstanceBufferPool::new();
    pool.upload_on_device(&gpu.device, &gpu.queue, "smudge", instances);
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
        if let Some((buffer, count)) = pool.get("smudge") {
            for _ in 0..input["repeat"].as_u64().unwrap() {
                batch.draw_passthrough_range(&mut pass, &atlas.texture, buffer, 0, count);
            }
        }
    }
    let reads = [
        gpu.read(&mut encoder, &color),
        gpu.read(&mut encoder, &depth),
    ];
    gpu.finish(encoder, &reads, size)
}

#[test]
#[ignore = "requires retail assets and GPU; original CRATER11 Cell color/Z through ordinary rendering"]
fn retail_crater11_matches_original_cell_raster() {
    let packet: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/smudge_render.json",
    ))
    .unwrap();
    let root = std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("physical retail root"));
    let mut assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let mode =
        crate::rules::ini_parser::IniFile::from_bytes(assets.get_ref("MPBattleMD.ini").unwrap())
            .unwrap();
    let (mut rules, rules_ini, art, _) =
        crate::app::loading::init_helpers::load_rules_with_merged_ini(&assets, Some(&mode), None)
            .unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    rules.bind_terrain_assets(
        &assets,
        theater.extension,
        crate::rules::terrain_asset_catalog::TerrainAssetLoadPhase::Fresh,
    );
    let registry = OverlayTypeRegistry::from_ini(&rules_ini, Some(&art));
    for source in packet["inputs"]["sources"].as_array().unwrap() {
        let name = source["name"].as_str().unwrap();
        if let Some(outer) = source["outer"]
            .as_str()
            .filter(|name| name.ends_with(".mmx"))
        {
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
        if name.to_ascii_lowercase().ends_with(".map") {
            let loaded =
                crate::map::source::load_map_by_name_or_path_with_assets(&root, name, &assets)
                    .unwrap();
            assert_eq!(
                serde_json::to_value(loaded.source).unwrap()["source_sha256"],
                source["sha256"],
                "physical map {name}"
            );
            continue;
        }
        assert_eq!(
            crate::util::sha256::sha256_hex(
                assets
                    .get_ref(name)
                    .unwrap_or_else(|| panic!("physical source {name}"))
            ),
            source["sha256"].as_str().unwrap(),
            "physical source {name}"
        );
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
        let atlas = build_overlay_atlas_on_device(
            &gpu.device,
            &gpu.queue,
            &batch,
            &[],
            &[],
            &[],
            &assets,
            &theater.iso_palette,
            &theater.unit_palette,
            &theater.tiberium_palette,
            theater.extension,
            "TEMPERATE",
            &registry,
            &rules,
            &rules_ini,
            Some(&rules.smudge_types),
            &[],
        )
        .expect("ordinary stock resource catalog includes smudge art");
        for row in packet["rows"].as_array().unwrap() {
            let input = &row["input"];
            let origin = coords(&input["coords"]);
            let cells = row["cells"].as_array().unwrap();
            let level = input["level"].as_u64().unwrap() as u8;
            let terrain =
                crate::map::resolved_terrain::test_grid(origin.0 + 2, origin.1 + 2, |x, y| {
                    crate::map::resolved_terrain::ResolvedTerrainCell {
                        level,
                        ..crate::map::resolved_terrain::test_smudge_cell(x, y)
                    }
                });
            let overlay =
                crate::sim::overlay_grid::OverlayGrid::new(terrain.width(), terrain.height());
            // Declared placement component only. The joined fatal producer
            // supplies its own live grid in the lifecycle witness.
            let grid = SmudgeGrid::from_map_entries(
                &[MapSmudgeEntry {
                    type_name: "CRATER11".into(),
                    rx: origin.0,
                    ry: origin.1,
                }],
                &rules.smudge_types,
                &terrain,
                &overlay,
                terrain.width(),
                terrain.height(),
            );
            assert_eq!(grid.iter_occupied().count(), cells.len());
            let lights = build_cell_light_grid_from_heights_and_units(
                cells.iter().map(|cell| {
                    (
                        coords(&cell["coords"]),
                        cell["level"].as_u64().unwrap() as u8,
                    )
                }),
                scene_profile(input),
            );
            for native in cells {
                let at = coords(&native["coords"]);
                let stored = grid.cell(at.0, at.1);
                assert_eq!(
                    u64::from(stored.type_id.unwrap()),
                    native["type_index"].as_u64().unwrap()
                );
                assert_eq!(
                    u64::from(stored.frame_offset),
                    native["piece"].as_u64().unwrap()
                );
                let light = lights.cell_light_at(at).unwrap();
                assert_eq!(
                    serde_json::json!([light.common_scalar, light.top_scalar, light.bottom_scalar]),
                    serde_json::json!([
                        native["common_scalar"],
                        native["top_scalar"],
                        native["bottom_scalar"]
                    ])
                );
                assert_eq!(serde_json::json!(light.rgb_key), native["convert_rgb_key"]);
            }
            let lookup = |type_id: u16, _frame: u8| {
                let def = rules.smudge_types.get(type_id)?;
                let entry = atlas.get(&smudge_key(&def.name))?;
                Some(TilePlacement {
                    uv_origin: entry.uv_origin,
                    uv_size: entry.uv_size,
                    pixel_size: entry.pixel_size,
                    draw_offset: [entry.offset_x, entry.offset_y],
                })
            };
            let camera = view(row, size);
            let instances = build_visible_instances(
                &grid,
                &rules.smudge_types,
                &terrain,
                &lights,
                &lookup,
                camera.camera_pos[0],
                camera.camera_pos[1],
                size[0] as f32,
                size[1] as f32,
            );
            assert_eq!(instances.len(), 1, "flat uniform composite emits once");
            let output = read_raster(&gpu, &mut batch, &atlas, &instances, row, format, size);
            assert_native_color_and_depth(&output, row, format, size);
        }
    }
}

/// Ordinary loaded match commands establish the producer and later PUT. This
/// is a Rust production-integration witness; the separate component above and
/// the core fatal fixture carry their bounded original-executable comparisons.
#[test]
#[ignore = "requires retail assets and GPU; paid building destruction and replacement through live smudge rendering"]
fn retail_paid_destruction_and_replacement_reach_smudge_rendering() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::production;
    use crate::sim::world::{FrameEffects, TickLane};
    use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};

    let root = std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("retail root"));
    let mut launch = crate::headless_scenario::battle_session(
        "Hills.mmx",
        &[(LaunchCountry::France, AiDifficulty::Easy)],
    );
    launch.local_player_mut().name = "VERA-OBSERVER".into();
    launch.local_slot_mut().country = LaunchCountry::America;
    launch.local_slot_mut().start_position = LaunchStartPosition::Position(0);
    launch.opponents[0].start_position = LaunchStartPosition::Position(1);
    launch.options.starting_credits = 50_000;
    launch.options.unit_count = 1;
    launch.options.tech_level = 10;
    launch.options.shroud = false;
    launch.options.fog_of_war = false;
    launch.options.short_game = false;
    let mut scene =
        crate::headless_scenario::load_session(&root, "Hills.mmx", 0x1234_5678, launch).unwrap();
    let owner = scene.sim().session().current_house().unwrap();
    let actor = |scene: &crate::headless_scenario::HeadlessScenario, name| {
        scene
            .sim()
            .entities()
            .iter_sorted()
            .find_map(|(id, entity)| {
                (entity.owner() == owner
                    && scene.sim().interner().resolve(entity.type_ref()) == name)
                    .then_some(id)
            })
            .unwrap()
    };
    let mcv = actor(&scene, "AMCV");
    let tank = actor(&scene, "MTNK");
    let advance = |scene: &mut crate::headless_scenario::HeadlessScenario,
                   command: Option<Command>| {
        let commands: Vec<_> = command
            .into_iter()
            .map(|payload| CommandEnvelope::new(owner, scene.sim().session().tick(), payload))
            .collect();
        scene
            .runtime
            .advance_frame(
                &commands,
                crate::app::types::SIM_TICK_MS,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
    };
    let power = scene.sim().interner().get("GAPOWR").unwrap();
    for step in 0..1100 {
        advance(
            &mut scene,
            match step {
                0 => Some(Command::DeployMcv { entity_id: mcv }),
                200 => Some(Command::QueueProduction { type_id: power }),
                _ => None,
            },
        );
    }
    let held = production::ready_building_entity_for_owner(
        scene.sim(),
        &scene.runtime.resources.rules,
        owner,
        power,
    )
    .expect("normal paid GAPOWR completion");
    let yard = actor(&scene, "GACNST");
    let yard = scene.sim().entities().get(yard).unwrap();
    let (base_x, base_y) = (yard.position.rx(), yard.position.ry());
    let terrain = scene.sim().resolved_terrain().unwrap();
    // Choose an experiment input through live loader facts and the real ready
    // placement query. This adds no placement policy or mutable game state.
    let mut candidates = Vec::new();
    for ry in base_y.saturating_sub(8)..base_y.saturating_add(9) {
        for rx in base_x.saturating_sub(8)..base_x.saturating_add(9) {
            let cells: Option<Vec<_>> = [(0, 0), (1, 0), (0, 1), (1, 1)]
                .into_iter()
                .map(|(dx, dy)| terrain.cell(rx + dx, ry + dy))
                .collect();
            let Some(cells) = cells else {
                continue;
            };
            if !cells.iter().all(|cell| {
                cell.accepts_smudge
                    && cell.slope_type == 0
                    && cell.level == cells[0].level
                    && !cell.has_bridge_deck
                    && terrain
                        .native_fixed_cell_index(cell.rx as i16, cell.ry as i16)
                        .is_some()
                    && scene
                        .sim()
                        .overlay_grid()
                        .unwrap()
                        .cell(cell.rx, cell.ry)
                        .overlay_id
                        .is_none()
                    && scene
                        .sim()
                        .smudge_grid()
                        .unwrap()
                        .cell(cell.rx, cell.ry)
                        .type_id
                        .is_none()
            }) {
                continue;
            }
            let Some(preview) = production::placement_preview_for_owner_with_overlays(
                scene.sim(),
                &scene.runtime.resources.rules,
                "VERA-OBSERVER",
                "GAPOWR",
                rx,
                ry,
                Some(&scene.runtime.resources.overlay_registry),
            ) else {
                continue;
            };
            if preview.valid
                && preview.width == 2
                && preview.height == 2
                && preview.in_build_area
                && preview.foundation_visible
                && preview.cells.len() == 4
                && preview.cells.iter().all(|cell| cell.valid)
            {
                candidates.push((rx.abs_diff(base_x) + ry.abs_diff(base_y), ry, rx));
            }
        }
    }
    candidates.sort_unstable();
    let (_, ry, rx) = *candidates
        .first()
        .expect("a legal flat morphable site near the normal base");
    eprintln!(
        "normal smudge site Hills.mmx seed0x12345678 origin({rx},{ry}), level{}",
        terrain.cell(rx, ry).unwrap().level
    );
    for step in 1100..4000 {
        advance(
            &mut scene,
            match step {
                1200 => Some(Command::PlaceReadyBuilding {
                    type_id: power,
                    rx,
                    ry,
                }),
                1500 => Some(Command::Attack {
                    attacker_id: tank,
                    target_id: held,
                }),
                _ => None,
            },
        );
    }
    assert!(
        !scene.sim().entities().contains(held),
        "actual fatal cleanup removes the paid building"
    );
    let grid = scene.sim().smudge_grid().unwrap();
    let mark = *grid.cell(rx, ry);
    let mark_id = mark
        .type_id
        .expect("the actual destruction leaves a persistent footprint");
    let mark_origin = mark.footprint_origin.unwrap();
    let mark_cells: Vec<_> = grid
        .iter_occupied()
        .filter(|(_, _, cell)| cell.footprint_origin == Some(mark_origin))
        .map(|(x, y, _)| (x, y))
        .collect();
    let rules = &scene.runtime.resources.rules;
    eprintln!(
        "normal produced {} at {:?}, {}cells",
        rules.smudge_types.get(mark_id).unwrap().name,
        mark_origin,
        mark_cells.len()
    );

    let mut assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let mode =
        crate::rules::ini_parser::IniFile::from_bytes(assets.get_ref("MPBattleMD.ini").unwrap())
            .unwrap();
    let (_, rules_ini, _, _) = crate::app::loading::init_helpers::load_rules_with_merged_ini(
        &assets,
        Some(&mode),
        Some(&scene.map.ini),
    )
    .unwrap();
    let gpu = Gpu::new();
    let size = [192, 160];
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    let atlas = build_overlay_atlas_on_device(
        &gpu.device,
        &gpu.queue,
        &batch,
        &[],
        &[],
        &[],
        &assets,
        &theater.iso_palette,
        &theater.unit_palette,
        &theater.tiberium_palette,
        theater.extension,
        "TEMPERATE",
        &scene.runtime.resources.overlay_registry,
        rules,
        &rules_ini,
        Some(&rules.smudge_types),
        &[],
    )
    .unwrap();
    let mut lights = crate::app::presentation::lighting::MatchLighting::default();
    lights.install(
        crate::map::lighting::CellLightGrid::new(),
        crate::map::lighting::parse_lighting(&scene.map.ini),
        2,
        Some((scene.sim().resolved_terrain().unwrap(), scene.sim())),
        None,
    );
    // Declared neutral-A viewport for the live producer, not a native whole-map
    // camera or a replacement for the actual App capture.
    let row = serde_json::json!({"name":"ordinary paid smudge", "input": {
        "coords":[mark_origin.0,mark_origin.1],"cell_point":[66,81],"viewport_y":0,
        "a":127,"background":65535,"old_z":2000,"repeat":1,"clip":[0,0,192,160]}});
    let camera = view(&row, size);
    let instances = |scene: &crate::headless_scenario::HeadlessScenario| {
        let lookup = |id, _| {
            let definition = scene.runtime.resources.rules.smudge_types.get(id)?;
            let entry = atlas.get(&smudge_key(&definition.name))?;
            Some(TilePlacement {
                uv_origin: entry.uv_origin,
                uv_size: entry.uv_size,
                pixel_size: entry.pixel_size,
                draw_offset: [entry.offset_x, entry.offset_y],
            })
        };
        build_visible_instances(
            scene.sim().smudge_grid().unwrap(),
            &scene.runtime.resources.rules.smudge_types,
            scene.sim().resolved_terrain().unwrap(),
            lights.grid(),
            &lookup,
            camera.camera_pos[0],
            camera.camera_pos[1],
            size[0] as f32,
            size[1] as f32,
        )
    };
    let hash = scene.sim().state_hash();
    let before = read_raster(
        &gpu,
        &mut batch,
        &atlas,
        &instances(&scene),
        &row,
        format,
        size,
    );
    assert_eq!(
        scene.sim().state_hash(),
        hash,
        "rendering cannot mutate the live producer"
    );
    let painted = |output: &[Vec<u8>]| {
        output[0]
            .chunks_exact(4)
            .filter(|pixel| *pixel != [255; 4])
            .count()
    };
    assert!(
        painted(&before) > 0,
        "the persistent live mark reaches the GPU"
    );
    for step in 4000..5000 {
        advance(
            &mut scene,
            match step {
                4000 => Some(Command::QueueProduction { type_id: power }),
                4800 => Some(Command::PlaceReadyBuilding {
                    type_id: power,
                    rx,
                    ry,
                }),
                _ => None,
            },
        );
    }
    assert!(
        scene
            .sim()
            .entities()
            .values()
            .any(|entity| entity.owner() == owner
                && entity.type_ref() == power
                && !entity.lifecycle.in_limbo()
                && (entity.position.rx(), entity.position.ry()) == (rx, ry)),
        "actual second paid placement succeeds"
    );
    assert!(
        mark_cells.iter().all(|&(x, y)| scene
            .sim()
            .smudge_grid()
            .unwrap()
            .cell(x, y)
            .type_id
            .is_none()),
        "normal PUT clears the whole intersected produced footprint"
    );
    let hash = scene.sim().state_hash();
    let after = read_raster(
        &gpu,
        &mut batch,
        &atlas,
        &instances(&scene),
        &row,
        format,
        size,
    );
    assert_eq!(scene.sim().state_hash(), hash);
    assert!(
        painted(&after) < painted(&before),
        "cleared smudge pixels disappear from the ordinary GPU path"
    );
}
