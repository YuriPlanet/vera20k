//! Ordinary paid Soviet opening and live ore depletion through the retained
//! minimap's actual queue upload, batch texture consumer and runtime ack.
//! Native Cell colors and weighted stores are separate consumed comparisons;
//! this does not certify a whole native GoldSt minimap/bootstrap.
use super::*;
use crate::headless_scenario::HeadlessScenario;
use crate::render::batch::InstanceBufferPool;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::world::{FrameEffects, TickLane};
use serde_json::Value;

const ORE: (u16, u16) = (123, 33);

fn ordinary_frame(scene: &mut HeadlessScenario, payload: Option<Command>) {
    let owner = scene.sim().session().current_house().unwrap();
    let commands: Vec<_> = payload
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
}

fn one_owned(scene: &HeadlessScenario, name: &str) -> u64 {
    let owner = scene.sim().session().current_house().unwrap();
    let ids: Vec<_> = scene
        .sim()
        .entities()
        .iter_sorted()
        .filter_map(|(id, entity)| {
            (entity.owner() == owner
                && scene.sim().interner().resolve(entity.type_ref()) == name
                && !entity.lifecycle.in_limbo())
            .then_some(id)
        })
        .collect();
    assert_eq!(ids.len(), 1, "one normal live {name}");
    ids[0]
}

fn paid_soviet_opening(root: &std::path::Path) -> HeadlessScenario {
    use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};
    let mut launch = crate::headless_scenario::battle_session(
        "GoldSt.mmx",
        &[(LaunchCountry::France, AiDifficulty::Easy)],
    );
    launch.local_slot_mut().country = LaunchCountry::Russia;
    launch.local_slot_mut().start_position = LaunchStartPosition::Position(3);
    launch.options.starting_credits = 50_000;
    launch.options.unit_count = 0;
    launch.options.tech_level = 10;
    launch.options.shroud = false;
    launch.options.fog_of_war = false;
    let mut scene =
        crate::headless_scenario::load_session(root, "GoldSt.mmx", 305419896, launch).unwrap();
    let mcv = one_owned(&scene, "SMCV");
    for frame in 0..4200 {
        let payload = match frame {
            0 => Some(Command::Move {
                entity_id: mcv,
                target_rx: 108,
                target_ry: 30,
            }),
            300 => Some(Command::DeployMcv { entity_id: mcv }),
            400 | 1200 | 3000 => {
                let name = match frame {
                    400 => "NAPOWR",
                    1200 => "NAREFN",
                    _ => "NARADR",
                };
                Some(Command::QueueProduction {
                    type_id: scene.sim().interner().get(name).unwrap(),
                })
            }
            1100 | 2800 | 4000 => {
                let (name, rx, ry) = match frame {
                    1100 => ("NAPOWR", 110, 33),
                    2800 => ("NAREFN", 112, 35),
                    _ => ("NARADR", 110, 37),
                };
                let owner = scene.sim().session().current_house().unwrap();
                let type_id = scene.sim().interner().get(name).unwrap();
                assert!(
                    crate::sim::production::ready_building_entity_for_owner(
                        scene.sim(),
                        &scene.runtime.resources.rules,
                        owner,
                        type_id,
                    )
                    .is_some(),
                    "normal paid {name} ready before the fixture click"
                );
                Some(Command::PlaceReadyBuilding { type_id, rx, ry })
            }
            2900 => Some(Command::Stop {
                entity_id: one_owned(&scene, "HARV"),
            }),
            _ => None,
        };
        ordinary_frame(&mut scene, payload);
    }
    for name in ["NACNST", "NAPOWR", "NAREFN", "NARADR", "HARV"] {
        one_owned(&scene, name);
    }
    let owner = scene.sim().session().current_house().unwrap();
    assert!(crate::sim::radar::has_radar_for_owner(
        scene.sim(),
        &scene.runtime.resources.rules,
        scene.sim().interner().resolve(owner),
    ));
    scene
}

struct Presented {
    format: wgpu::TextureFormat,
    batch: BatchRenderer,
    minimap: MinimapRenderer,
}

fn new_presentation(gpu: &Gpu, scene: &HeadlessScenario, format: wgpu::TextureFormat) -> Presented {
    let overlay_colors = HashMap::new();
    let overlay_data = crate::app::loading::transitions::build_minimap_overlay_data(
        &scene.map.overlays,
        &scene.map.terrain_objects,
        Some(&scene.runtime.resources.overlay_registry),
        Some(&scene.runtime.resources.rules),
    );
    let view = scene.runtime.view();
    let grid = crate::map::terrain::build_terrain_grid_from_resolved(
        view.resolved_terrain().unwrap(),
        None,
    );
    let (bounds, revision) = view.playfield_authority();
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera([MINIMAP_WIDTH, MINIMAP_HEIGHT]));
    let minimap = MinimapRenderer::new(
        &gpu.device,
        &gpu.queue,
        &batch,
        &grid,
        Some(&scene.runtime),
        &overlay_data,
        &overlay_colors,
        &scene.map.header.theater,
        bounds,
        revision,
    );
    Presented {
        format,
        batch,
        minimap,
    }
}

/// Sample the uploaded production texture through the ordinary UI batch. This
/// needs neither COPY_SRC on the original texture nor a fake window context.
fn read_uploaded(gpu: &Gpu, presented: &Presented) -> Vec<u8> {
    let size = [MINIMAP_WIDTH, MINIMAP_HEIGHT];
    let instance = SpriteInstance {
        position: [0.0, 0.0],
        size: size.map(|v| v as f32),
        uv_size: [1.0; 2],
        alpha: 1.0,
        tint: [1.0; 3],
        ..Default::default()
    };
    let mut pool = InstanceBufferPool::new();
    pool.upload_on_device(&gpu.device, &gpu.queue, "minimap-upload", &[instance]);
    let (buffer, count) = pool.get("minimap-upload").unwrap();
    let color = gpu.target(size, presented.format);
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let cv = color.create_view(&Default::default());
    let dv = depth.create_view(&Default::default());
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    clear(
        &mut encoder,
        &cv,
        &dv,
        u16::MAX,
        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
    );
    {
        let mut pass =
            crate::app::presentation::sidebar_render::begin_main_load_pass(&mut encoder, &cv, &dv);
        presented.batch.draw_with_buffer_ui_passthrough(
            &mut pass,
            presented.minimap.map_texture(),
            buffer,
            count,
        );
    }
    let read = gpu.read(&mut encoder, &color);
    gpu.finish(encoder, &[read], size).remove(0)
}

fn present(
    gpu: &Gpu,
    scene: &mut HeadlessScenario,
    presentations: &mut [Presented],
) -> crate::app::presentation::render::minimap_transaction::MinimapPresentationCommit {
    let before = scene.sim().state_hash();
    let commit = crate::app::presentation::render::minimap_transaction::present_minimap_frame(
        &mut scene.runtime,
        |runtime| {
            let view = runtime.view();
            let (cells, generation) = view.radar_terrain_dirty();
            let colors = HouseColorMap::new();
            let overlay_colors = HashMap::new();
            let mut consumed = None;
            for (index, presented) in presentations.iter_mut().enumerate() {
                let current = presented.minimap.update_unit_dots(
                    &gpu.queue,
                    view.entities(),
                    view.logic_order(),
                    view.houses(),
                    &colors,
                    view.session().tick(),
                    view.session().binary_frame(),
                    view.session().current_house(),
                    view.fog(),
                    view.house_alliances(),
                    true,
                    view.session().game_mode_nonzero(),
                    Some(&runtime.resources.rules),
                    Some(view.interner()),
                    view.overlay_grid(),
                    Some(&runtime.resources.overlay_registry),
                    &overlay_colors,
                    view.resolved_terrain(),
                    cells,
                    generation,
                );
                if index == 0 {
                    consumed = current;
                } else {
                    assert_eq!(
                        current, consumed,
                        "both target formats consume the same batch"
                    );
                }
            }
            Ok::<_, std::convert::Infallible>(consumed)
        },
    )
    .unwrap();
    assert_eq!(
        scene.sim().state_hash(),
        before,
        "presentation/ack preserves simulation/RNG"
    );
    commit
}

#[test]
#[ignore = "requires retail XMP03T4 and GPU; natural first-frame ore, texture publication and retained acknowledgement"]
fn retail_authored_ore_spread_reaches_uploaded_minimap() {
    let root = std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("physical retail root"));
    let profile = crate::test_fixture::json("tools/map_observation.ore-spread.example.json");
    let launch = serde_json::from_value(profile["launch"].clone()).unwrap();
    let mut scene =
        crate::headless_scenario::load_session(&root, "XMP03T4.MAP", 0x12345678, launch).unwrap();
    let targets = [(71, 24), (72, 24), (73, 24), (71, 25), (71, 26)];
    for &(rx, ry) in &targets {
        assert!(
            scene
                .runtime
                .view()
                .overlay_grid()
                .unwrap()
                .cell(rx, ry)
                .overlay_id
                .is_none()
        );
    }
    let gpu = Gpu::new();
    let mut presentations: Vec<_> = [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ]
    .into_iter()
    .map(|format| new_presentation(&gpu, &scene, format))
    .collect();
    // This exercises the terrain texture owner, whose presenter is enabled here.
    // The normal MCV-only opening does not yet have an online radar building.
    present(&gpu, &mut scene, &mut presentations);
    let before = presentations[0]
        .minimap
        .native_radar_terrain
        .as_ref()
        .unwrap()
        .raw_rgb()
        .to_vec();
    ordinary_frame(&mut scene, None);
    let grown: Vec<_> = targets
        .into_iter()
        .filter(|&(rx, ry)| {
            scene
                .runtime
                .view()
                .overlay_grid()
                .unwrap()
                .cell(rx, ry)
                .overlay_id
                .is_some()
        })
        .collect();
    assert!(
        !grown.is_empty(),
        "ordinary first frame creates ore without supplied queues/RNG/timers"
    );
    let (dirty, generation) = scene.runtime.view().radar_terrain_dirty();
    for cell in &grown {
        assert!(
            dirty.contains(cell),
            "new resource membership publishes {cell:?}"
        );
    }
    let commit = present(&gpu, &mut scene, &mut presentations);
    assert_eq!(commit.consumed_generation, Some(generation));
    assert!(commit.acknowledged);
    assert!(scene.runtime.view().radar_terrain_dirty().0.is_empty());
    let hash = scene.sim().state_hash();
    for presented in &presentations {
        let surface = presented.minimap.native_radar_terrain.as_ref().unwrap();
        let geometry = surface.geometry();
        for &cell in &grown {
            let (x, y) = geometry.cell_to_raw_pixel(cell);
            let index = (y * geometry.raw_size().0 + x) as usize;
            assert_ne!(
                &surface.raw_rgb()[index..index + 2],
                &before[index..index + 2]
            );
        }
        let mut expected = presented.minimap.composed_rgba().0.to_vec();
        if presented.format == wgpu::TextureFormat::Bgra8UnormSrgb {
            for pixel in expected.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
        }
        assert_eq!(
            read_uploaded(&gpu, presented),
            expected,
            "uploaded natural ore/{:?}",
            presented.format
        );
        // Original initialization654650/Fill654EA0 consumes current Cell47C060.
        // Retaining authored map overlays must not hide naturally added cells.
        let fresh = new_presentation(&gpu, &scene, presented.format);
        assert_eq!(
            fresh
                .minimap
                .native_radar_terrain
                .as_ref()
                .unwrap()
                .raw_rgb(),
            surface.raw_rgb(),
            "reconstruction and incremental publication share live Cell authority"
        );
        let mut expected = fresh.minimap.base_terrain_rgba.clone();
        if fresh.format == wgpu::TextureFormat::Bgra8UnormSrgb {
            for pixel in expected.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
        }
        assert_eq!(
            read_uploaded(&gpu, &fresh),
            expected,
            "constructor uploads current terrain"
        );
    }
    assert_eq!(scene.sim().state_hash(), hash);
    let again = present(&gpu, &mut scene, &mut presentations);
    assert_eq!(again.consumed_generation, None);
    assert!(!again.acknowledged);
}

#[test]
#[ignore = "requires retail GoldSt and GPU; paid radar, real HARV depletion, texture upload and acknowledgement"]
fn retail_ore_depletion_reaches_uploaded_minimap_and_acknowledges_once() {
    let root = std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("physical retail root"));
    let mut scene = paid_soviet_opening(&root);
    let gpu = Gpu::new();
    let mut presentations: Vec<_> = [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ]
    .into_iter()
    .map(|format| new_presentation(&gpu, &scene, format))
    .collect();
    present(&gpu, &mut scene, &mut presentations);
    let miner = one_owned(&scene, "HARV");
    ordinary_frame(
        &mut scene,
        Some(Command::HarvestCell {
            entity_id: miner,
            target_rx: ORE.0,
            target_ry: ORE.1,
        }),
    );
    let packet: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/ore_render.json",
    ))
    .unwrap();
    let native = &packet["radar"]["depletion"];
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..2400 {
        let cell = *scene
            .runtime
            .view()
            .overlay_grid()
            .unwrap()
            .cell(ORE.0, ORE.1);
        let name = if cell.overlay_id.is_none() {
            "empty"
        } else if cell.overlay_data == 0 {
            "density_0"
        } else if cell.overlay_data == 1 {
            "density_1"
        } else {
            "other"
        };
        if name != "other" && seen.insert(name) {
            let row = native["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["name"] == name)
                .unwrap();
            let view = scene.runtime.view();
            let terrain = view.resolved_terrain().unwrap().cell(ORE.0, ORE.1).unwrap();
            assert_eq!(
                serde_json::json!([
                    terrain.final_tile_index,
                    terrain.final_sub_tile as u16,
                    terrain.level as u16,
                    terrain.slope_type as u16,
                ]),
                serde_json::json!([
                    row["state"]["tile"],
                    row["state"]["subtile"],
                    row["state"]["level"],
                    row["state"]["slope"],
                ]),
                "actual current terrain at {name}"
            );
            let entity = view.entities().get(miner).unwrap();
            assert_eq!((entity.position.rx(), entity.position.ry()), ORE);
            let (cells, generation) = scene.runtime.view().radar_terrain_dirty();
            // Partial reduction itself is tactical-only, but fresh sight
            // notifications (original5865F0) independently enqueue radar cells
            // during this unpresented HARV journey. Consume that real pending
            // batch too; only full removal requires ORE to be in it.
            if name == "empty" {
                assert!(cells.contains(&ORE), "identity removal dirties this Cell");
            }
            let pending = !cells.is_empty();
            let commit = present(&gpu, &mut scene, &mut presentations);
            if pending {
                assert_eq!(commit.consumed_generation, Some(generation));
                assert!(
                    commit.acknowledged,
                    "the pending terrain upload is acknowledged"
                );
                assert!(
                    scene.runtime.view().radar_terrain_dirty().0.is_empty(),
                    "the successful upload retires the runtime's actual batch"
                );
            } else {
                assert_eq!(commit.consumed_generation, None);
                assert!(!commit.acknowledged);
            }
            for presented in &presentations {
                let surface = presented.minimap.native_radar_terrain.as_ref().unwrap();
                let geometry = surface.geometry();
                let (x, y) = geometry.cell_to_raw_pixel(ORE);
                let index = (y * geometry.raw_size().0 + x) as usize;
                assert_eq!(
                    serde_json::json!(&surface.raw_rgb()[index..index + 2]),
                    row["color_pair"]
                );
                let mut expected = presented.minimap.composed_rgba().0.to_vec();
                if presented.format == wgpu::TextureFormat::Bgra8UnormSrgb {
                    for pixel in expected.as_chunks_mut::<4>().0 {
                        pixel.swap(0, 2);
                    }
                }
                assert_eq!(
                    read_uploaded(&gpu, presented),
                    expected,
                    "actual uploaded minimap {name}/{:?}",
                    presented.format
                );
            }
            let again = present(&gpu, &mut scene, &mut presentations);
            assert_eq!(again.consumed_generation, None);
            assert!(!again.acknowledged);
        }
        if seen.contains("empty") {
            break;
        }
        ordinary_frame(&mut scene, None);
    }
    assert_eq!(
        seen,
        std::collections::BTreeSet::from(["density_1", "density_0", "empty"])
    );
    assert_eq!(one_owned(&scene, "HARV"), miner);

    // Native full initialization/rebuild654650 -> Fill654EA0 calls current
    // Cell47C060 at654F99. Reconstruct from the actually depleted world while
    // retaining authored map overlays: a stale startup snapshot revives ore.
    // This tests reconstruction authority, not a native allocation on harvest.
    let before = scene.sim().state_hash();
    let empty = native["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "empty")
        .unwrap();
    for presented in &presentations {
        let fresh = new_presentation(&gpu, &scene, presented.format);
        let surface = fresh.minimap.native_radar_terrain.as_ref().unwrap();
        let geometry = surface.geometry();
        let (x, y) = geometry.cell_to_raw_pixel(ORE);
        let index = (y * geometry.raw_size().0 + x) as usize;
        assert_eq!(
            serde_json::json!(&surface.raw_rgb()[index..index + 2]),
            empty["color_pair"],
            "fresh constructor uses current Cell colors"
        );
        let mut expected = fresh.minimap.base_terrain_rgba.clone();
        if fresh.format == wgpu::TextureFormat::Bgra8UnormSrgb {
            for pixel in expected.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
        }
        assert_eq!(
            read_uploaded(&gpu, &fresh),
            expected,
            "constructor uploads its current terrain before a dirty update"
        );
    }
    assert_eq!(scene.sim().state_hash(), before);
}
