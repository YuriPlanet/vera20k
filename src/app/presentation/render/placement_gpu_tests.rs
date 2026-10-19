//! Original Cell47EC90/6D5030 PLACE stores through the production resource,
//! current-cell preview, centered SHP emitter and neutral-A/no-Z draw path.
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::map::terrain::iso_to_screen;
use crate::render::batch::{BatchRenderer, InstanceBufferPool, SpriteInstance};
use crate::render::selection_overlay::CenteredShapeStrip;
use crate::render::shroud_buffer::ShroudBuffer;
use crate::render::terrain_draw_gpu_tests::{
    Gpu, assert_native_color_and_depth, camera, clear, encoded,
};
use crate::sim::production::{self, BuildingPlacementCell};
use crate::sim::world::{FrameEffects, TickLane};
use serde_json::Value;

fn ordinary_frame(
    scene: &mut crate::headless_scenario::HeadlessScenario,
    payload: Option<crate::sim::command::Command>,
) {
    let owner = scene.sim().session.current_house().unwrap();
    let commands: Vec<_> = payload
        .into_iter()
        .map(|payload| {
            crate::sim::command::CommandEnvelope::new(owner, scene.sim().session.tick(), payload)
        })
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
}

/// A normal loaded starting MCV, movement/deploy and paid factory completion.
/// No supplied base, ready flag, slope mutation or preview admission enters
/// this integration witness. Native loading/production cadence is not claimed.
fn paid_goldst(root: &std::path::Path) -> crate::headless_scenario::HeadlessScenario {
    use crate::sim::command::Command;
    use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};
    let mut launch = crate::headless_scenario::battle_session(
        "GoldSt.mmx",
        &[
            (LaunchCountry::France, AiDifficulty::Easy),
            (LaunchCountry::America, AiDifficulty::Easy),
        ],
    );
    launch.local_slot_mut().start_position = LaunchStartPosition::Position(3);
    launch.options.starting_credits = 50_000;
    launch.options.unit_count = 0;
    launch.options.tech_level = 10;
    launch.options.shroud = false;
    launch.options.fog_of_war = false;
    let mut scene =
        crate::headless_scenario::load_session(root, "GoldSt.mmx", 305419896, launch).unwrap();
    let owner = scene.sim().session.current_house().unwrap();
    let mcv = scene
        .sim()
        .entities()
        .iter_sorted()
        .find_map(|(id, entity)| {
            (entity.owner() == owner && scene.sim().interner.resolve(entity.type_ref()) == "AMCV")
                .then_some(id)
        })
        .expect("normal starting MCV");
    for frame in 0..1600 {
        let payload = match frame {
            0 => Some(Command::Move {
                entity_id: mcv,
                target_rx: 108,
                target_ry: 30,
            }),
            300 => {
                let entity = scene.sim().entities().get(mcv).unwrap();
                assert_eq!((entity.position.rx(), entity.position.ry()), (108, 30));
                Some(Command::DeployMcv { entity_id: mcv })
            }
            400 => Some(Command::QueueProduction {
                type_id: scene.sim().interner.get("GAPOWR").unwrap(),
            }),
            _ => None,
        };
        ordinary_frame(&mut scene, payload);
    }
    assert_eq!(scene.map.header.theater, "URBAN");
    let sim = scene.sim();
    let rules = &scene.runtime.resources.rules;
    let owner_name = sim.interner.resolve(owner);
    let held = production::ready_building_entity_for_owner(
        sim,
        rules,
        owner,
        sim.interner.get("GAPOWR").unwrap(),
    )
    .expect("ordinary paid completion");
    assert!(sim.entities().get(held).unwrap().lifecycle.in_limbo());
    assert_eq!(production::credits_for_owner(sim, owner_name), 49_200);
    assert!(sim.entities().values().any(|entity| {
        entity.owner() == owner
            && sim.interner.resolve(entity.type_ref()) == "GACNST"
            && (entity.position.rx(), entity.position.ry()) == (107, 29)
    }));
    scene
}

#[test]
#[ignore = "requires retail assets and GPU; original Cell placement and full foundation color/Z stores"]
fn retail_placement_matches_original_cell_and_foundation_raster() {
    let packet: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/placement_render.json",
    ))
    .unwrap();
    assert_eq!(packet["rows"].as_array().unwrap().len(), 14);
    assert_eq!(packet["scenes"].as_array().unwrap().len(), 1);
    let root = std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("retail root"));
    let mut assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
    crate::map::theater::load_theater(&mut assets, "URBAN").unwrap();
    for source in packet["inputs"]["sources"].as_array().unwrap() {
        let name = source["name"].as_str().unwrap();
        if name.eq_ignore_ascii_case("GoldSt.mmx") {
            let loaded =
                crate::map::source::load_map_by_name_or_path_with_assets(&root, name, &assets)
                    .unwrap();
            assert_eq!(
                serde_json::to_value(loaded.source).unwrap()["source_sha256"],
                source["sha256"]
            );
            continue;
        }
        assert_eq!(
            crate::util::sha256::sha256_hex(assets.get_ref(name).unwrap()),
            source["sha256"].as_str().unwrap(),
            "physical input {name}"
        );
    }
    let mut scene = paid_goldst(&root);
    let sim = scene.sim();
    // Consume the original full selected Recalc/LAT outputs against the live
    // loader result, rather than treating authored tile bytes as final state.
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let recalc = &packet["inputs"]["map"]["native_recalc"];
    assert_eq!(recalc["rows"].as_array().unwrap().len(), 2);
    for row in recalc["rows"].as_array().unwrap() {
        let coord = &row["coords"];
        let cell = terrain
            .cell(
                coord[0].as_u64().unwrap() as u16,
                coord[1].as_u64().unwrap() as u16,
            )
            .unwrap();
        let output = &row["output"];
        assert_eq!(
            serde_json::json!({
                "tile": cell.final_tile_index, "subtile": cell.final_sub_tile,
                "level": cell.level, "slope": cell.slope_type, "land": cell.land_type,
            }),
            *output
        );
        for neighbor in row["neighbors"].as_array().unwrap() {
            let coord = &neighbor["coords"];
            let cell = terrain
                .cell(
                    coord[0].as_u64().unwrap() as u16,
                    coord[1].as_u64().unwrap() as u16,
                )
                .unwrap();
            assert_eq!(u64::from(cell.level), neighbor["level"].as_u64().unwrap());
            if let Some(slope) = neighbor.get("slope") {
                assert_eq!(u64::from(cell.slope_type), slope.as_u64().unwrap());
            }
        }
    }
    let rules = &scene.runtime.resources.rules;
    let owner = sim.session.current_house().unwrap();
    let preview = production::placement_preview_for_owner_with_overlays(
        sim,
        rules,
        sim.interner.resolve(owner),
        "GAPOWR",
        106,
        34,
        Some(&scene.runtime.resources.overlay_registry),
    )
    .unwrap();
    assert!(!preview.valid);
    assert_eq!(
        preview.reason,
        Some(production::BuildingPlacementError::BlockedTerrain)
    );
    assert_eq!(
        preview
            .cells
            .iter()
            .map(|cell| (cell.valid, cell.level, cell.slope))
            .collect::<Vec<_>>(),
        vec![(false, 5, 1), (true, 6, 0), (false, 5, 1), (true, 6, 0)]
    );
    let gpu = Gpu::new();
    let size = [
        packet["surface"]["width"].as_u64().unwrap() as u32,
        packet["surface"]["height"].as_u64().unwrap() as u32,
    ];
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let shape = CenteredShapeStrip::from_assets_on_device(
            &gpu.device,
            &gpu.queue,
            &batch,
            &assets,
            "place.shp",
            "palette.pal",
        )
        .unwrap();
        let mut pool = InstanceBufferPool::new();
        for row in packet["rows"]
            .as_array()
            .unwrap()
            .iter()
            .chain(packet["scenes"].as_array().unwrap())
        {
            let input = &row["input"];
            let mut cam = camera(size);
            let instances: Vec<SpriteInstance> = if let Some(view) = input.get("view_origin") {
                cam.camera_pos = [
                    view[0].as_i64().unwrap() as f32,
                    view[1].as_i64().unwrap() as f32 + 15.0,
                ];
                // Independent original caller camera, actual loaded map/paid
                // preview, and production X-outer traversal, with no fitted
                // cell positions or native-admission input in this scene.
                let instances = shape.build_building_preview(&preview);
                assert_eq!(instances.len(), 4);
                assert_eq!(
                    row["native_order"],
                    serde_json::json!([[106, 34], [106, 35], [107, 34], [107, 35]])
                );
                instances
            } else {
                let coord = (
                    input["coords"][0].as_u64().unwrap() as u16,
                    input["coords"][1].as_u64().unwrap() as u16,
                );
                let point = iso_to_screen(coord.0, coord.1, 0);
                cam.camera_pos = [
                    point.0 - input["cell_point"][0].as_i64().unwrap() as f32,
                    point.1 - input["cell_point"][1].as_i64().unwrap() as f32,
                ];
                let cell = BuildingPlacementCell {
                    coord,
                    valid: row["admitted"][0].as_u64().unwrap() != 0,
                    level: input["level"].as_i64().unwrap() as i8,
                    slope: input["slope"].as_u64().unwrap() as u8,
                };
                match input["flags"].as_u64().unwrap() {
                    0 => Vec::new(),
                    4 => {
                        // Pending producer remains outside the active chain.
                        // Consume this original component as frame1 resource,
                        // geometry and material coverage, not pending parity.
                        assert_eq!(row["draws"][0]["frame"], 1);
                        let anchor = &row["draws"][0]["point"];
                        let mut instance = shape
                            .instance(
                                1,
                                (
                                    anchor[0].as_i64().unwrap() as f32 + cam.camera_pos[0],
                                    anchor[1].as_i64().unwrap() as f32 + cam.camera_pos[1],
                                ),
                                0.0,
                            )
                            .unwrap();
                        instance.palette_light =
                            crate::render::palette_light::PaletteLight::plain(53, 1000);
                        vec![instance]
                    }
                    2 => vec![
                        shape.building_placement_instance(cell).unwrap();
                        input["repeat"].as_u64().unwrap() as usize
                    ],
                    other => panic!("uncovered marker input {other}"),
                }
            };
            let a = ShroudBuffer::fixture(
                &gpu.device,
                &gpu.queue,
                size,
                cam.camera_pos,
                &vec![input["a"].as_u64().unwrap() as u8; (size[0] * size[1]) as usize],
            );
            batch.bind_shroud(&gpu.device, Some(&a));
            batch.write_camera(&gpu.queue, cam);
            pool.upload_on_device(&gpu.device, &gpu.queue, "placement", &instances);
            let color = gpu.target(size, format);
            let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
            let cv = color.create_view(&Default::default());
            let dv = depth.create_view(&Default::default());
            let background = input["background"].as_u64().unwrap() as u16;
            let bytes = encoded(background, format).repeat((size[0] * size[1]) as usize);
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &color,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size[0] * 4),
                    rows_per_image: Some(size[1]),
                },
                crate::render::terrain_draw_gpu_tests::extent(size),
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
                let mut pass = crate::app::presentation::sidebar_render::begin_main_load_pass(
                    &mut encoder,
                    &cv,
                    &dv,
                );
                let clip = &input["clip"];
                pass.set_scissor_rect(
                    clip[0].as_u64().unwrap() as u32,
                    clip[1].as_u64().unwrap() as u32,
                    clip[2].as_u64().unwrap() as u32,
                    clip[3].as_u64().unwrap() as u32,
                );
                if let Some((buffer, count)) = pool.get("placement") {
                    batch.draw_with_buffer_passthrough(&mut pass, shape.texture(), buffer, count);
                }
            }
            let reads = [
                gpu.read(&mut encoder, &color),
                gpu.read(&mut encoder, &depth),
            ];
            let output = gpu.finish(encoder, &reads, size);
            assert_native_color_and_depth(&output, row, format, size);
        }
    }
    paid_refinery_centering_and_free_unit(&mut scene, &packet["centering"]);
}

/// Continue the same normal opening through POWER -> paid GAREFN -> opening
/// CMIN. This consumes native mark coordinates, not a second placement model.
/// Full production cadence/FreeUnit native bootstrap is not a new claim here.
fn paid_refinery_centering_and_free_unit(
    scene: &mut crate::headless_scenario::HeadlessScenario,
    centering: &Value,
) {
    use crate::rules::foundation::placement_origin_at_cursor;
    use crate::sim::command::Command;
    let owner = scene.sim().session.current_house().unwrap();
    let power = scene.sim().interner.get("GAPOWR").unwrap();
    let power_origin = placement_origin_at_cursor("2x2", (110, 33));
    ordinary_frame(
        scene,
        Some(Command::PlaceReadyBuilding {
            type_id: power,
            rx: power_origin.0,
            ry: power_origin.1,
        }),
    );
    // A fixture wait, not a golden native construction duration.
    for _ in 0..200 {
        ordinary_frame(scene, None);
    }
    let name = centering["building"]["name"].as_str().unwrap();
    let refinery = scene.sim().interner.get(name).unwrap();
    let rules = &scene.runtime.resources.rules;
    let foundation = rules.object(name).unwrap().foundation.clone();
    assert_eq!(crate::rules::foundation::foundation_id(&foundation), 12);
    assert_eq!(rules.building_free_unit(name), Some("CMIN"));
    assert_eq!(
        serde_json::json!(crate::rules::foundation::foundation_cell_offsets(
            &foundation
        )),
        centering["building"]["offsets"]
    );
    ordinary_frame(scene, Some(Command::QueueProduction { type_id: refinery }));
    let ready = |scene: &crate::headless_scenario::HeadlessScenario| {
        production::ready_building_entity_for_owner(
            scene.sim(),
            &scene.runtime.resources.rules,
            owner,
            refinery,
        )
    };
    for _ in 0..4096 {
        if ready(scene).is_some() {
            break;
        }
        ordinary_frame(scene, None);
    }
    let held = ready(scene).expect("normal paid refinery completion");
    assert_eq!(
        production::credits_for_owner(scene.sim(), scene.sim().interner.resolve(owner)),
        47_200
    );
    let miners = |scene: &crate::headless_scenario::HeadlessScenario| {
        scene
            .sim()
            .entities()
            .iter_sorted()
            .filter_map(|(id, entity)| {
                (entity.owner() == owner
                    && scene.sim().interner.resolve(entity.type_ref()) == "CMIN")
                    .then_some(id)
            })
            .collect::<Vec<_>>()
    };
    assert!(
        miners(scene).is_empty(),
        "limbo paid refinery has no opening miner"
    );
    let preview = |scene: &crate::headless_scenario::HeadlessScenario, cursor| {
        let origin = placement_origin_at_cursor(&foundation, cursor);
        production::placement_preview_for_owner_with_overlays(
            scene.sim(),
            &scene.runtime.resources.rules,
            scene.sim().interner.resolve(owner),
            name,
            origin.0,
            origin.1,
            Some(&scene.runtime.resources.overlay_registry),
        )
        .unwrap()
    };
    let before = scene.sim().state_hash();
    for step in centering["lifecycle"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .take(2)
    {
        let center = &step["output"]["center"];
        let current = preview(
            scene,
            (
                center[0].as_u64().unwrap() as u16,
                center[1].as_u64().unwrap() as u16,
            ),
        );
        let mut coords: Vec<_> = current.cells.iter().map(|cell| cell.coord).collect();
        coords.sort();
        assert_eq!(serde_json::json!(coords), step["output"]["marked"]);
    }
    assert_eq!(
        scene.sim().state_hash(),
        before,
        "moving markers cannot change simulation/RNG"
    );

    let refused = preview(scene, (106, 34)); // Native origin105,33 crosses the ramp.
    assert!(!refused.valid);
    assert!(
        refused.in_build_area && refused.foundation_visible,
        "blocked footprint must pass the independent input gates: {refused:?}"
    );
    ordinary_frame(
        scene,
        Some(Command::PlaceReadyBuilding {
            type_id: refinery,
            rx: refused.rx,
            ry: refused.ry,
        }),
    );
    assert_eq!(ready(scene), Some(held));
    assert!(
        scene
            .sim()
            .entities()
            .get(held)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    assert!(miners(scene).is_empty());

    let clear = preview(scene, (113, 35));
    assert_eq!((clear.rx, clear.ry), (112, 34));
    assert!(clear.valid);
    assert_eq!(clear.cells.len(), 12);
    assert!(
        clear
            .cells
            .iter()
            .all(|cell| cell.valid && cell.level == 6 && cell.slope == 0)
    );
    ordinary_frame(
        scene,
        Some(Command::PlaceReadyBuilding {
            type_id: refinery,
            rx: clear.rx,
            ry: clear.ry,
        }),
    );
    let placed = scene.sim().entities().get(held).unwrap();
    assert!(!placed.lifecycle.in_limbo());
    assert_eq!((placed.position.rx(), placed.position.ry()), (112, 34));
    for cell in &clear.cells {
        assert_eq!(
            scene.sim().occupancy().first_building_on_layer(
                cell.coord.0,
                cell.coord.1,
                crate::sim::movement::locomotor::MovementLayer::Ground,
            ),
            Some(held),
            "same paid parent occupies {:?}",
            cell.coord
        );
    }
    for _ in 0..512 {
        if !miners(scene).is_empty() {
            break;
        }
        ordinary_frame(scene, None);
    }
    let opening_miners = miners(scene);
    assert_eq!(
        opening_miners.len(),
        1,
        "normal GrandOpening produces one CMIN"
    );
    for _ in 0..128 {
        ordinary_frame(scene, None);
    }
    assert_eq!(
        miners(scene),
        opening_miners,
        "completion never duplicates the free unit"
    );
    assert_eq!(
        production::credits_for_owner(scene.sim(), scene.sim().interner.resolve(owner)),
        47_200
    );
    assert!(
        ready(scene).is_none(),
        "success consumes the existing held product"
    );
}
