//! Original flat resource Cell raster through the production ordinary overlay
//! atlas, live identity/frame owner and submission. The lifecycle check uses a
//! real retail map and the bound runtime's ordinary HarvestCell command.
use super::{CellOverlayInputs, build_cell_overlay_instances};
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::headless_scenario::HeadlessScenario;
use crate::map::lighting::CellLightGrid;
use crate::map::overlay::OverlayEntry;
use crate::render::batch::{BatchRenderer, InstanceBufferPool, SpriteInstance};
use crate::render::overlay_atlas::{OverlayAtlas, build_overlay_atlas_on_device};
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::render::terrain_draw_gpu_tests::{
    Gpu, assert_native_color_and_depth, camera, clear, encoded, extent,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

struct Probe {
    gpu: Gpu,
    batches: Vec<(wgpu::TextureFormat, BatchRenderer, OverlayAtlas)>,
    names: BTreeMap<u8, String>,
    size: [u32; 2],
}

impl Probe {
    fn new(scene: &HeadlessScenario, packet: &Value) -> Self {
        let root = PathBuf::from(std::env::var_os("RA2_DIR").expect("physical retail root"));
        let mut assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
        let mode = crate::rules::ini_parser::IniFile::from_bytes(
            assets.get_ref("MPBattleMD.ini").unwrap(),
        )
        .unwrap();
        let (_, rules_ini, _, _) = crate::app::loading::init_helpers::load_rules_with_merged_ini(
            &assets,
            Some(&mode),
            Some(&scene.map.ini),
        )
        .unwrap();
        let theater =
            crate::map::theater::load_theater(&mut assets, &scene.map.header.theater).unwrap();
        for source in packet["inputs"]["sources"].as_array().unwrap() {
            let name = source["name"].as_str().unwrap();
            if (name.to_ascii_lowercase().starts_with("tib")
                && name
                    .to_ascii_lowercase()
                    .ends_with(&format!(".{}", theater.extension)))
                || name.eq_ignore_ascii_case("urban.pal")
            {
                assert_eq!(
                    crate::util::sha256::sha256_hex(assets.get_ref(name).unwrap()),
                    source["sha256"].as_str().unwrap(),
                    "physical native/GPU source {name}"
                );
            }
        }
        let gpu = Gpu::new();
        let registry = &scene.runtime.resources.overlay_registry;
        let rules = &scene.runtime.resources.rules;
        let entries: Vec<_> = scene
            .map
            .overlays
            .iter()
            .filter(|e| (e.rx, e.ry) == (123, 33))
            .cloned()
            .collect();
        assert_eq!(entries.len(), 1);
        let batches = [
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ]
        .into_iter()
        .map(|format| {
            let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
            let atlas = build_overlay_atlas_on_device(
                &gpu.device,
                &gpu.queue,
                &batch,
                &entries,
                &[],
                &[],
                &assets,
                &theater.iso_palette,
                &theater.unit_palette,
                &theater.tiberium_palette,
                theater.extension,
                &scene.map.header.theater,
                registry,
                &rules.tiberium_types,
                &rules.crate_rules,
                &rules_ini,
                rules.art(),
                None,
                &[],
            )
            .unwrap();
            (format, batch, atlas)
        })
        .collect();
        Self {
            gpu,
            batches,
            names: crate::render::overlay_assets::render_overlay_names(registry),
            size: [
                packet["surface"]["width"].as_u64().unwrap() as u32,
                packet["surface"]["height"].as_u64().unwrap() as u32,
            ],
        }
    }

    fn compare(&self, scene: &HeadlessScenario, entries: &[OverlayEntry], row: &Value) {
        let input = &row["input"];
        let rx = input["coords"][0].as_u64().unwrap() as u16;
        let ry = input["coords"][1].as_u64().unwrap() as u16;
        let point = crate::map::terrain::iso_to_screen(rx, ry, 0);
        let mut camera = camera(self.size);
        camera.camera_pos = [
            point.0 - input["cell_point"][0].as_i64().unwrap() as f32,
            point.1
                - input["cell_point"][1].as_i64().unwrap() as f32
                - input["viewport_y"].as_i64().unwrap() as f32,
        ];
        let terrain = scene.sim().resolved_terrain.as_ref().unwrap();
        let heights = terrain.build_height_map();
        let lighting = CellLightGrid::new();
        for (format, batch, atlas) in &self.batches {
            let mut instances: Vec<SpriteInstance> = Vec::new();
            let mut policies: Vec<RenderZPolicy> = Vec::new();
            build_cell_overlay_instances(
                &CellOverlayInputs {
                    entries,
                    atlas,
                    names: &self.names,
                    live_grid: scene.sim().overlay_grid.as_ref(),
                    registry: Some(&scene.runtime.resources.overlay_registry),
                    rules: Some(&scene.runtime.resources.rules),
                    interner: Some(&scene.sim().interner),
                    tiberium_types: Some(&scene.runtime.resources.rules.tiberium_types),
                    terrain: Some(terrain),
                    heights: &heights,
                    lighting: &lighting,
                    scenario_lighting: &scene.sim().session.lighting,
                    camera: camera.camera_pos,
                    viewport: self.size.map(|v| v as f32),
                    origin_y: camera.world_origin_y,
                    world_height: camera.world_height,
                },
                &mut instances,
                &mut policies,
            );
            let mut pool = InstanceBufferPool::new();
            pool.upload_on_device(&self.gpu.device, &self.gpu.queue, "ore-native", &instances);
            batch.write_camera(&self.gpu.queue, camera);
            let color = self.gpu.target(self.size, *format);
            let depth = self
                .gpu
                .target(self.size, wgpu::TextureFormat::Depth32Float);
            let cv = color.create_view(&Default::default());
            let dv = depth.create_view(&Default::default());
            let background = input["background"].as_u64().unwrap() as u16;
            let initial =
                encoded(background, *format).repeat((self.size[0] * self.size[1]) as usize);
            self.gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &color,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &initial,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.size[0] * 4),
                    rows_per_image: Some(self.size[1]),
                },
                extent(self.size),
            );
            let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
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
                let clip: [u32; 4] =
                    std::array::from_fn(|i| input["clip"][i].as_u64().unwrap() as u32);
                pass.set_scissor_rect(clip[0], clip[1], clip[2], clip[3]);
                crate::app::presentation::render::draw_pooled_overlay_bodies(
                    &mut pass,
                    batch,
                    &pool,
                    Some(atlas),
                    "ore-native",
                    &policies,
                );
            }
            let reads = [
                self.gpu.read(&mut encoder, &color),
                self.gpu.read(&mut encoder, &depth),
            ];
            let output = self.gpu.finish(encoder, &reads, self.size);
            assert_native_color_and_depth(&output, row, *format, self.size);
        }
    }
}

fn packet() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/ore_render.json",
    ))
    .unwrap()
}

#[test]
#[ignore = "requires retail GoldSt and GPU; native flat ore through actual harvest, frame publication and ordinary body submission"]
fn retail_ore_harvest_publication_matches_original_cell_raster() {
    use crate::app::presentation::overlay_index::OverlayRenderIndex;
    use crate::sim::command::{Command, CommandEnvelope};
    let root = PathBuf::from(std::env::var_os("RA2_DIR").expect("physical retail root"));
    let mut scene = crate::headless_scenario::load(&root, "GoldSt.mmx", 0x12345678).unwrap();
    let packet = packet();
    let probe = Probe::new(&scene, &packet);
    let mut entries = OverlayRenderIndex::default();
    entries.replace_from_source(
        scene
            .map
            .overlays
            .iter()
            .filter(|e| (e.rx, e.ry) == (123, 33))
            .cloned()
            .collect(),
    );
    // Use the existing player's starting-base owner. An added house with
    // only a miner is correctly defeated by the ordinary short-game gate.
    let owner = scene.sim().session.current_house.unwrap();
    let owner_name = scene.sim().interner.resolve(owner).to_owned();
    let dock = scene
        .runtime
        .resources
        .rules
        .object("HARV")
        .unwrap()
        .dock
        .first()
        .expect("retail HARV Dock reader")
        .clone();
    let base = scene.sim().houses[&owner]
        .base_center
        .expect("starting house base center");
    let candidates: Vec<_> = scene
        .sim()
        .resolved_terrain
        .as_ref()
        .unwrap()
        .iter()
        .filter(|cell| {
            let distance = cell.rx.abs_diff(base.0) + cell.ry.abs_diff(base.1);
            (8..=16).contains(&distance)
        })
        .map(|cell| (cell.rx, cell.ry))
        .collect();
    // Supply an initially placed owned refinery through its actual constructor/
    // Unlimbo/house tracking. Native Harvest73E67F..73E6BC requires Dock ownership
    // before any state can cut; no mission, Stage or tracking fields are forged.
    let refinery = candidates
        .into_iter()
        .find_map(|at| {
            scene.runtime.simulation.spawn_object_with_overlay_registry(
                &dock,
                &owner_name,
                at.0,
                at.1,
                0,
                &scene.runtime.resources.rules,
                &scene.runtime.resources.overlay_registry,
            )
        })
        .expect("an accepted map-placed refinery near the starting base");
    assert!(scene.sim().substrate.entities.get(refinery).is_some());
    assert!(
        scene.sim().houses[&owner].tracking.owns_any_building([scene
            .sim()
            .interner
            .get(&dock)
            .unwrap()])
    );
    let miner = scene
        .runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "HARV",
            &owner_name,
            123,
            33,
            0,
            &scene.runtime.resources.rules,
            &scene.runtime.resources.overlay_registry,
        )
        .expect("retail War Miner Unlimbo on the ore cell");
    let order = CommandEnvelope::new(
        owner,
        scene.sim().session.tick,
        Command::HarvestCell {
            entity_id: miner,
            target_rx: 123,
            target_ry: 33,
        },
    );
    let mut seen = std::collections::BTreeSet::new();
    let mut zero_cargo = None;
    for tick in 0..240 {
        let live = scene.sim().overlay_grid.as_ref().unwrap().cell(123, 33);
        let key = if live.overlay_id.is_some() {
            format!("density_{}", live.overlay_data)
        } else {
            "empty".into()
        };
        if let Some(row) = packet["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == key)
        {
            if seen.insert(key.clone()) {
                // The native reference draws this cell only. The actual
                // frame index also receives unrelated map growth updates.
                let crop: Vec<_> = entries
                    .iter()
                    .filter(|e| (e.rx, e.ry) == (123, 33))
                    .cloned()
                    .collect();
                probe.compare(&scene, &crop, row);
            }
        }
        if key == "density_0" || key == "empty" {
            let step = if key == "density_0" { 0 } else { 1 };
            let native = &packet["lifecycle"]["steps"][step]["state"];
            assert_eq!(
                live.overlay_id.map_or(-1, i64::from),
                native["overlay_id"].as_i64().unwrap()
            );
            assert_eq!(
                i64::from(live.overlay_data),
                native["data"].as_i64().unwrap()
            );
        }
        let miner_state = scene
            .sim()
            .substrate
            .entities
            .get(miner)
            .unwrap_or_else(|| panic!("miner missing at tick {tick}, ore {key}"))
            .miner
            .as_ref()
            .unwrap();
        if key == "density_0" {
            zero_cargo = Some(miner_state.cargo().len());
        }
        if key == "empty" {
            // Original Reduce480A80 returns zero on this final clear; Harvest
            // does not add another bale. Search can immediately hop to new ore.
            assert_eq!(
                miner_state.cargo().len(),
                zero_cargo.expect("occupied zero was visible before exhaustion")
            );
            let harvest: Value = serde_json::from_str(crate::test_fixture::text(
                "tools/spatial_oracle/harvest_field.json",
            ))
            .unwrap();
            let hop = harvest["harvest"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["input"]["name"] == "s1_depleted_hop")
                .unwrap();
            assert_eq!(
                miner_state.harvesting(),
                hop["state"]["harvesting"].as_u64().unwrap() != 0
            );
            assert!(
                matches!(scene.sim().substrate.entities.get(miner).unwrap().navigation.nav_com(),
                Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) if (rx,ry)!=(123,33)),
                "exhaustion searches onward in the populated stock ore field"
            );
            assert_eq!(
                entries.iter().filter(|e| (e.rx, e.ry) == (123, 33)).count(),
                1,
                "retained presentation slot must be suppressed by live empty Cell"
            );
            break;
        }
        let commands = if tick == 0 {
            std::slice::from_ref(&order)
        } else {
            &[]
        };
        let output = scene
            .runtime
            .advance_frame(
                commands,
                crate::headless_scenario::SIM_TICK_MS,
                crate::sim::world::TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        // Same presentation index owner and application order as sim_tick.
        entries.remove_cells(&output.overlay_removals);
        entries.upsert_occupied(output.overlay_updates);
    }
    let last = scene.sim().substrate.entities.get(miner).unwrap();
    assert!(
        seen.contains("density_4")
            && seen.contains("density_1")
            && seen.contains("density_0")
            && seen.contains("empty"),
        "native-backed states {seen:?}; tick {} nativeframe {} current {:?} queued {:?} pos ({},{}) nav {:?} miner {:?} cargo {}",
        scene.sim().session.tick,
        scene.sim().session.binary_frame,
        last.mission.current(),
        last.mission.queued(),
        last.position.rx(),
        last.position.ry(),
        last.navigation.nav_com(),
        last.miner_state(),
        last.miner.as_ref().unwrap().cargo().len()
    );
}

#[test]
#[ignore = "requires physical retail art and GPU; original flat ore caller/shape color and depth controls"]
fn retail_flat_ore_rows_match_original_color_and_depth() {
    let root = PathBuf::from(std::env::var_os("RA2_DIR").expect("physical retail root"));
    let mut scene = crate::headless_scenario::load(&root, "GoldSt.mmx", 0x12345678).unwrap();
    let packet = packet();
    let probe = Probe::new(&scene, &packet);
    let registry = &scene.runtime.resources.overlay_registry;
    let tib01 = registry.id_for_name("TIB01").unwrap();
    for control in packet["controls"].as_array().unwrap() {
        let xy: [u16; 2] = std::array::from_fn(|i| control["coords"][i].as_i64().unwrap() as u16);
        assert_eq!(
            registry.flat_tiberium_display_overlay_id(
                &scene.runtime.resources.rules.tiberium_types,
                tib01,
                xy[0],
                xy[1]
            ),
            Some(control["display_overlay_id"].as_u64().unwrap() as u8),
            "original signed selector {control}"
        );
    }
    for row in packet["rows"].as_array().unwrap() {
        let input = &row["input"];
        let at = (
            input["coords"][0].as_u64().unwrap() as u16,
            input["coords"][1].as_u64().unwrap() as u16,
        );
        let runtime = &mut scene.runtime;
        let id = runtime
            .resources
            .overlay_registry
            .id_for_name(input["type"].as_str().unwrap())
            .unwrap();
        let cell = runtime
            .simulation
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(at.0, at.1)
            .unwrap();
        assert_eq!(
            i64::from(cell.level),
            input["level"].as_i64().unwrap(),
            "independent native component level vs selected physical map"
        );
        assert_eq!(
            cell.slope_type, 0,
            "this witness is the flat resource branch"
        );
        let grid = runtime.simulation.overlay_grid.as_mut().unwrap();
        if row["draws"].as_array().unwrap().is_empty() {
            grid.clear_overlay(at.0, at.1);
        } else {
            grid.place_overlay(at.0, at.1, id, input["state"].as_u64().unwrap() as u8);
        }
        grid.recalculate_runtime_cell(
            runtime.simulation.resolved_terrain.as_mut().unwrap(),
            &runtime.resources.overlay_registry,
            at,
        );
        let entry = OverlayEntry {
            rx: at.0,
            ry: at.1,
            overlay_id: id,
            frame: input["state"].as_u64().unwrap() as u8,
        };
        probe.compare(&scene, &[entry], row);
    }
}
