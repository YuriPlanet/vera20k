//! Original Cell rasters through the production ordinary overlay
//! atlas, live identity/frame owner and submission. Lifecycle checks use real
//! retail maps and the bound runtime's ordinary harvest, growth and fire paths.
use super::{CellOverlayInputs, build_cell_overlay_instances};
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::headless_scenario::HeadlessScenario;
use crate::map::lighting::CellLightGrid;
use crate::map::overlay::OverlayEntry;
use crate::render::batch::{BatchRenderer, InstanceBufferPool, SpriteInstance};
use crate::render::bridge_atlas::{BridgeAtlas, build_bridge_atlas};
use crate::render::overlay_atlas::{OverlayAtlas, build_overlay_atlas_on_device};
use crate::render::shroud_buffer::ShroudBuffer;
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{
    Gpu, assert_native_color_and_depth, camera, clear, encoded, extent,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

struct ProbeBatch {
    format: wgpu::TextureFormat,
    batch: BatchRenderer,
    atlas: OverlayAtlas,
    shadow: Option<(BridgeAtlas, TerrainDrawRenderer)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CellRasterPiece {
    Body,
    Shadow,
    BodyThenShadow,
}

struct Probe {
    gpu: Gpu,
    batches: Vec<ProbeBatch>,
    names: BTreeMap<u8, String>,
    size: [u32; 2],
    lighting: crate::app::presentation::lighting::MatchLighting,
    live_a: Option<(ShroudBuffer, crate::sim::intern::InternedId, u8)>,
}

impl Probe {
    fn new(
        scene: &HeadlessScenario,
        packet: &Value,
        source_cell: Option<(u16, u16)>,
        viewer: Option<crate::sim::intern::InternedId>,
    ) -> Self {
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
        let material_sources = packet["inputs"]["material_sources"].as_array();
        if let Some(materials) = material_sources {
            assert!(!materials.is_empty());
            for material in materials {
                assert!(
                    packet["inputs"]["sources"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|source| {
                            source["name"]
                                .as_str()
                                .unwrap()
                                .eq_ignore_ascii_case(material.as_str().unwrap())
                        }),
                    "native material source {material}"
                );
            }
        }
        for source in packet["inputs"]["sources"].as_array().unwrap() {
            let name = source["name"].as_str().unwrap();
            let selected = material_sources.map_or_else(
                || {
                    (name.to_ascii_lowercase().starts_with("tib")
                        && name
                            .to_ascii_lowercase()
                            .ends_with(&format!(".{}", theater.extension)))
                        || name.eq_ignore_ascii_case("urban.pal")
                        || name.eq_ignore_ascii_case("temperat.pal")
                },
                |materials| {
                    materials
                        .iter()
                        .any(|material| material.as_str().unwrap().eq_ignore_ascii_case(name))
                },
            );
            if selected {
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
        // A future paid wall has no map-pack slot. The production atlas
        // preloads all Wall frames from the retail registry independently.
        let entries: Vec<_> = source_cell
            .map(|at| {
                let entries: Vec<_> = scene
                    .map
                    .overlays
                    .iter()
                    .filter(|e| (e.rx, e.ry) == at)
                    .cloned()
                    .collect();
                assert_eq!(entries.len(), 1, "authored source slot at {at:?}");
                entries
            })
            .unwrap_or_default();
        let names = crate::render::overlay_assets::render_overlay_names(registry);
        let with_shadows = packet["rows"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row.get("shadow").is_some()));
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
            let shadow = with_shadows.then(|| {
                let atlas = build_bridge_atlas(
                    &gpu.device,
                    &gpu.queue,
                    &batch,
                    &names,
                    &assets,
                    &theater.iso_palette,
                    &theater.unit_palette,
                    theater.extension,
                    &scene.map.header.theater,
                    registry,
                    &rules_ini,
                    rules.art(),
                    &[],
                )
                .expect("production Cell shadow atlas");
                let renderer = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
                (atlas, renderer)
            });
            ProbeBatch {
                format,
                batch,
                atlas,
                shadow,
            }
        })
        .collect();
        let size = [
            packet["surface"]["width"].as_u64().unwrap() as u32,
            packet["surface"]["height"].as_u64().unwrap() as u32,
        ];
        let live_a = viewer.map(|owner| {
            let shp =
                crate::assets::shp_file::ShpFile::from_bytes(assets.get_ref("SHROUD.SHP").unwrap())
                    .unwrap();
            let (frames, width, height) =
                crate::render::shroud_buffer::extract_shp_brightness(&shp);
            (
                ShroudBuffer::new(
                    &gpu.device,
                    &gpu.queue,
                    size,
                    [scene.sim().fog.width, scene.sim().fog.height],
                    frames,
                    [width, height],
                    crate::render::shroud_buffer::SHROUD_EDGE_LUT,
                ),
                owner,
                packet["surface"]["a_value"].as_u64().unwrap() as u8,
            )
        });
        let mut lighting = crate::app::presentation::lighting::MatchLighting::default();
        lighting.install(
            CellLightGrid::new(),
            crate::map::lighting::parse_lighting(&scene.map.ini),
            2,
            Some((scene.sim().resolved_terrain.as_ref().unwrap(), scene.sim())),
        );
        Self {
            gpu,
            batches,
            names,
            size,
            lighting,
            live_a,
        }
    }

    fn compare(&mut self, scene: &HeadlessScenario, entries: &[OverlayEntry], row: &Value) {
        self.compare_piece(scene, entries, row, CellRasterPiece::Body);
        if let Some(shadow) = row.get("shadow") {
            self.compare_piece(scene, entries, shadow, CellRasterPiece::Shadow);
            self.compare_piece(
                scene,
                entries,
                &row["body_then_shadow"],
                CellRasterPiece::BodyThenShadow,
            );
        }
    }

    fn compare_piece(
        &mut self,
        scene: &HeadlessScenario,
        entries: &[OverlayEntry],
        row: &Value,
        piece: CellRasterPiece,
    ) {
        let hash_before = scene.sim().state_hash();
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
        if let Some((source, owner, expected)) = &mut self.live_a {
            source.rebuild_if_needed(
                &self.gpu.device,
                &self.gpu.queue,
                &scene.sim().fog,
                *owner,
                camera.camera_pos[0],
                camera.camera_pos[1],
                self.size[0],
                self.size[1],
                1.0,
            );
            let actual_a = self
                .gpu
                .read_float_texels(source.gpu_source().0, [0, 0], self.size);
            assert!(
                actual_a
                    .iter()
                    .all(|pixel| (pixel[0] * 255.0).round() as u8 == *expected)
            );
        }
        for ProbeBatch {
            format,
            batch,
            atlas,
            shadow,
        } in &mut self.batches
        {
            batch.bind_shroud(
                &self.gpu.device,
                self.live_a.as_ref().map(|(source, _, _)| source),
            );
            let mut instances: Vec<SpriteInstance> = Vec::new();
            let mut policies: Vec<RenderZPolicy> = Vec::new();
            if piece != CellRasterPiece::Shadow {
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
                        lighting: self.lighting.grid(),
                        scenario_lighting: &scene.sim().session.lighting,
                        camera: camera.camera_pos,
                        viewport: self.size.map(|v| v as f32),
                        origin_y: camera.world_origin_y,
                        world_height: camera.world_height,
                    },
                    &mut instances,
                    &mut policies,
                );
            }
            let mut shadow_instances = Vec::new();
            if piece != CellRasterPiece::Body {
                let (atlas, _) = shadow.as_ref().expect("native shadow comparison resources");
                crate::app::presentation::instances::bridges::build_bridge_shadow_instances_inner(
                    terrain,
                    atlas,
                    &self.names,
                    entries,
                    scene.sim().overlay_grid.as_ref(),
                    Some(&scene.runtime.resources.overlay_registry),
                    None,
                    camera.world_origin_y,
                    camera.world_height,
                    camera.camera_pos[0],
                    camera.camera_pos[1],
                    self.size[0] as f32,
                    self.size[1] as f32,
                    &mut shadow_instances,
                );
            }
            if input["overlay_id"].as_i64() == Some(-1) {
                assert!(
                    instances.is_empty(),
                    "live erased Cell suppresses its retained entry"
                );
                assert!(
                    shadow_instances.is_empty(),
                    "erased Cell suppresses its retained shadow"
                );
            }
            let mut pool = InstanceBufferPool::new();
            pool.upload_on_device(
                &self.gpu.device,
                &self.gpu.queue,
                "cell-overlay-native",
                &instances,
            );
            if piece != CellRasterPiece::Body {
                pool.upload_on_device(
                    &self.gpu.device,
                    &self.gpu.queue,
                    "cell-shadow-native",
                    &shadow_instances,
                );
            }
            batch.write_camera(&self.gpu.queue, camera);
            let color = self.gpu.target(self.size, *format);
            let depth = self
                .gpu
                .target(self.size, wgpu::TextureFormat::Depth32Float);
            let cv = color.create_view(&Default::default());
            let dv = depth.create_view(&Default::default());
            if piece != CellRasterPiece::Body {
                shadow
                    .as_mut()
                    .unwrap()
                    .1
                    .prepare(&self.gpu.device, &color, &dv, camera);
            }
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
                    "cell-overlay-native",
                    &policies,
                );
            }
            if piece != CellRasterPiece::Body {
                let (atlas, renderer) = shadow.as_mut().unwrap();
                renderer.note_external_passes(&mut encoder, 1);
                let clip = std::array::from_fn(|i| input["clip"][i].as_u64().unwrap() as u32);
                crate::app::presentation::render::draw_bridge_shadows(
                    &mut encoder,
                    &cv,
                    &dv,
                    renderer,
                    clip,
                    batch,
                    pool.get("cell-shadow-native"),
                    &shadow_instances,
                    Some(&atlas.texture),
                );
            }
            let reads = [
                self.gpu.read(&mut encoder, &color),
                self.gpu.read(&mut encoder, &depth),
            ];
            let output = self.gpu.finish(encoder, &reads, self.size);
            assert_native_color_and_depth(&output, row, *format, self.size);
        }
        assert_eq!(
            scene.sim().state_hash(),
            hash_before,
            "ordinary overlay presentation is read-only"
        );
    }
}

fn packet() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/ore_render.json")
}

/// A normal stock AP command supplies the wall states; original Cell47F6A0
/// rasters independently supply their expected material, color and depth.
/// The CPU packet's independent seed/impact history is not a World timing oracle.
#[test]
#[ignore = "requires physical retail XMP03T4 and GPU; ordinary fence fire publication and native wall rasters with live A"]
fn retail_authored_fence_fire_reaches_index_a_and_native_cell_raster() {
    assert_retail_wall_profile_reaches_native_raster(
        "tools/map_observation.authored-fence.example.json",
        "tools/spatial_oracle/wall_render.json",
        "CAFNCB",
        true,
    );
}

/// Normal paid GAWALL admission and AP damage supply the live wall states.
/// Original Cell47F6A0 supplies raster expectations; its independent damage
/// history is not an oracle for this World's shot frames or recursive rolls.
#[test]
#[ignore = "requires physical retail XMP03T4 and GPU; paid concrete wall publication and native wall rasters with live A"]
fn retail_paid_concrete_wall_fire_reaches_index_a_and_native_cell_raster() {
    assert_retail_wall_profile_reaches_native_raster(
        "tools/map_observation.concrete-wall.example.json",
        "tools/spatial_oracle/concrete_wall_render.json",
        "GAWALL",
        false,
    );
}

fn assert_retail_wall_profile_reaches_native_raster(
    profile_path: &'static str,
    native_path: &'static str,
    wall_name: &str,
    initially_authored: bool,
) {
    use crate::app::presentation::overlay_index::OverlayRenderIndex;
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::world::{FrameEffects, TickLane};

    let Some((root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let profile = crate::test_fixture::json(profile_path);
    let native = crate::test_fixture::json(native_path);
    let launch: crate::skirmish_launch::SkirmishLaunchSession =
        serde_json::from_value(profile["launch"].clone()).unwrap();
    let map_name = launch.selected_map_file.as_deref().unwrap().to_owned();
    let selected =
        crate::map::source::load_map_by_name_or_path_with_assets(&root, &map_name, &assets)
            .unwrap();
    let source = native["inputs"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| {
            source["name"]
                .as_str()
                .unwrap()
                .eq_ignore_ascii_case(&map_name)
        })
        .unwrap();
    assert_eq!(
        serde_json::to_value(selected.source).unwrap()["source_sha256"],
        source["sha256"]
    );
    let mut scene = crate::headless_scenario::load_session(
        &root,
        &map_name,
        profile["seed"].as_u64().unwrap() as u32,
        launch,
    )
    .unwrap();
    scene.runtime.simulation.batch_path_searches();
    scene.runtime.simulation.batch_target_scans();
    let owner = scene.sim().session.current_house.unwrap();
    let orders: Vec<_> = profile["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|record| {
            assert_eq!(record["owner"], "VERA-OBSERVER");
            CommandEnvelope::new(
                owner,
                record["issue_after_step"].as_u64().unwrap(),
                serde_json::from_value::<Command>(record["payload"].clone()).unwrap(),
            )
        })
        .collect();
    let (attacker_id, source) = orders
        .iter()
        .find_map(|order| match order.payload {
            Command::ForceAttackCell {
                attacker_id,
                target_rx,
                target_ry,
            } => Some((attacker_id, (target_rx, target_ry))),
            _ => None,
        })
        .expect("ordinary Cell attack profile");
    if initially_authored {
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].execute_tick, 0);
    }
    let tank = scene.sim().entities().get(attacker_id).unwrap();
    assert_eq!(scene.sim().resolve(tank.type_ref()), "MTNK");
    assert_eq!(tank.owner(), owner);
    let wall = scene
        .runtime
        .resources
        .overlay_registry
        .id_for_name(wall_name)
        .unwrap();
    let initial = scene
        .sim()
        .overlay_grid
        .as_ref()
        .unwrap()
        .cell(source.0, source.1);
    assert_eq!(initial.overlay_id, initially_authored.then_some(wall));
    if !initially_authored {
        assert_eq!(initial.overlay_data, 0);
    }
    let rows = native["rows"].as_array().unwrap();
    assert!(!rows.is_empty());
    let coords = |row: &Value| {
        (
            row["input"]["coords"][0].as_u64().unwrap() as u16,
            row["input"]["coords"][1].as_u64().unwrap() as u16,
        )
    };
    let mut entries = OverlayRenderIndex::default();
    entries.replace_from_source(scene.map.overlays.clone());
    assert!(
        scene
            .runtime
            .simulation
            .prepare_fog_view_for("VERA-OBSERVER")
    );
    let hash = scene.sim().state_hash();
    let mut probe = Probe::new(
        &scene,
        native,
        initially_authored.then_some(source),
        Some(owner),
    );
    assert_eq!(
        scene.sim().state_hash(),
        hash,
        "atlas/A/light preparation is read-only"
    );
    let observed_cells: std::collections::BTreeSet<_> = rows.iter().map(coords).collect();
    if !initially_authored {
        for &at in &observed_cells {
            let initial = scene.sim().overlay_grid.as_ref().unwrap().cell(at.0, at.1);
            assert_eq!((initial.overlay_id, initial.overlay_data), (None, 0));
            assert!(!entries.iter().any(|entry| (entry.rx, entry.ry) == at));
        }
    }
    let mut placed = std::collections::BTreeSet::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut dispatched = 0;
    for step in 0..=profile["ticks"].as_u64().unwrap() {
        for &at in &observed_cells {
            let live = scene.sim().overlay_grid.as_ref().unwrap().cell(at.0, at.1);
            if live.overlay_id == Some(wall) {
                let published: Vec<_> = entries
                    .iter()
                    .filter(|entry| (entry.rx, entry.ry) == at)
                    .collect();
                assert_eq!(
                    published.len(),
                    1,
                    "published live wall at {at:?}, step {step}"
                );
                assert_eq!(
                    (published[0].overlay_id, published[0].frame),
                    (wall, live.overlay_data)
                );
                placed.insert(at);
            }
        }
        for (index, row) in rows.iter().enumerate() {
            if seen.contains(&index) {
                continue;
            }
            let at = coords(row);
            let live = scene.sim().overlay_grid.as_ref().unwrap().cell(at.0, at.1);
            // Initial absence is not removal. Credit an erased raster only
            // after ordinary placement has published this coordinate's slot.
            if !placed.contains(&at) {
                continue;
            }
            if live.overlay_id.map_or(-1, i64::from) != row["input"]["overlay_id"].as_i64().unwrap()
                || u64::from(live.overlay_data) != row["input"]["state"].as_u64().unwrap()
            {
                continue;
            }
            let cell = scene
                .sim()
                .resolved_terrain
                .as_ref()
                .unwrap()
                .cell(at.0, at.1)
                .unwrap();
            assert_eq!(
                i64::from(cell.level),
                row["input"]["level"].as_i64().unwrap()
            );
            assert_eq!(cell.slope_type, 0);
            let crop: Vec<_> = entries
                .iter()
                .filter(|entry| (entry.rx, entry.ry) == at)
                .cloned()
                .collect();
            assert_eq!(crop.len(), 1, "unique retained source slot at {at:?}");
            if let Some(id) = live.overlay_id {
                assert_eq!((crop[0].overlay_id, crop[0].frame), (id, live.overlay_data));
            }
            assert!(
                scene
                    .runtime
                    .simulation
                    .prepare_fog_view_for("VERA-OBSERVER")
            );
            probe.compare(&scene, &crop, row);
            seen.insert(index);
        }
        if seen.len() == rows.len() {
            break;
        }
        if step == profile["ticks"].as_u64().unwrap() {
            break;
        }
        let due: Vec<_> = orders
            .iter()
            .filter(|order| order.execute_tick == step)
            .cloned()
            .collect();
        dispatched += due.len();
        let output = scene
            .runtime
            .advance_frame(&due, 22, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        assert!(output.tick.frame_committed);
        entries.remove_cells(&output.overlay_removals);
        entries.upsert_occupied(output.overlay_updates);
    }
    assert_eq!(
        dispatched,
        orders.len(),
        "all ordinary profile commands executed"
    );
    assert_eq!(
        seen.len(),
        rows.len(),
        "ordinary {wall_name} fire must reach each independent native raster witness; saw {seen:?}"
    );
}

/// Cell483710/487190's ordinary source gain reaches the same retained index,
/// atlas, A source and Cell47F6A0 submission as the two original rasters. Native
/// scheduler/RNG and other object AI remain the core consumer's separate scope.
#[test]
#[ignore = "requires physical retail XMP03T4 and GPU; ordinary growth publication and native source rasters with live A"]
fn retail_natural_ore_growth_reaches_index_a_and_native_cell_raster() {
    use crate::app::presentation::overlay_index::OverlayRenderIndex;
    use crate::sim::world::{FrameEffects, TickLane};

    let Some((root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let profile = crate::test_fixture::json("tools/map_observation.ore-spread.example.json");
    assert!(profile["commands"].as_array().unwrap().is_empty());
    let launch: crate::skirmish_launch::SkirmishLaunchSession =
        serde_json::from_value(profile["launch"].clone()).unwrap();
    let map_name = launch.selected_map_file.as_deref().unwrap().to_owned();
    let native = crate::test_fixture::json("tools/spatial_oracle/ore_render_natural.json");
    let selected =
        crate::map::source::load_map_by_name_or_path_with_assets(&root, &map_name, &assets)
            .unwrap();
    let map_input = native["inputs"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| {
            source["name"]
                .as_str()
                .unwrap()
                .eq_ignore_ascii_case(&map_name)
        })
        .unwrap();
    assert_eq!(
        serde_json::to_value(selected.source).unwrap()["source_sha256"],
        map_input["sha256"],
        "physical map, including loose-file precedence"
    );
    let mut scene = crate::headless_scenario::load_session(
        &root,
        &map_name,
        profile["seed"].as_u64().unwrap() as u32,
        launch,
    )
    .unwrap();
    let before = &native["rows"][0];
    let after = &native["rows"][1];
    let source = (
        before["input"]["coords"][0].as_u64().unwrap() as u16,
        before["input"]["coords"][1].as_u64().unwrap() as u16,
    );
    let cell = scene
        .sim()
        .resolved_terrain
        .as_ref()
        .unwrap()
        .cell(source.0, source.1)
        .unwrap();
    assert_eq!(
        i64::from(cell.level),
        before["input"]["level"].as_i64().unwrap()
    );
    assert_eq!(cell.slope_type, 0);
    let mut entries = OverlayRenderIndex::default();
    entries.replace_from_source(scene.map.overlays.clone());
    let witnesses = [(71, 24), (72, 24), (73, 24), (71, 25), (71, 26)];
    assert!(
        witnesses
            .iter()
            .all(|at| !entries.iter().any(|e| (e.rx, e.ry) == *at))
    );
    assert!(
        scene
            .runtime
            .simulation
            .prepare_fog_view_for("VERA-OBSERVER")
    );
    let owner = scene.sim().session.current_house.unwrap();
    let hash = scene.sim().state_hash();
    let mut probe = Probe::new(&scene, native, Some(source), Some(owner));
    assert_eq!(
        scene.sim().state_hash(),
        hash,
        "atlas/A/light preparation is read-only"
    );
    let source_entries = |index: &OverlayRenderIndex| {
        index
            .iter()
            .filter(|e| (e.rx, e.ry) == source)
            .cloned()
            .collect::<Vec<_>>()
    };
    let initial = source_entries(&entries);
    assert_eq!(initial.len(), 1);
    assert_eq!(
        u64::from(initial[0].frame),
        before["input"]["state"].as_u64().unwrap()
    );
    probe.compare(&scene, &initial, before);

    let output = scene
        .runtime
        .advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())
        .unwrap();
    assert!(output.tick.frame_committed);
    entries.remove_cells(&output.overlay_removals);
    let retained_slot = entries.iter().position(|e| (e.rx, e.ry) == source).unwrap();
    entries.upsert_occupied(output.overlay_updates);
    assert_eq!(
        entries.iter().position(|e| (e.rx, e.ry) == source),
        Some(retained_slot)
    );
    let grown = source_entries(&entries);
    assert_eq!(grown.len(), 1, "a live update retains one source slot");
    assert_eq!(
        u64::from(grown[0].frame),
        after["input"]["state"].as_u64().unwrap()
    );
    for at in witnesses {
        let retained: Vec<_> = entries.iter().filter(|e| (e.rx, e.ry) == at).collect();
        assert_eq!(retained.len(), 1, "new ordinary spread cell {at:?}");
        let live = scene.sim().overlay_grid.as_ref().unwrap().cell(at.0, at.1);
        assert_eq!(Some(retained[0].overlay_id), live.overlay_id);
        assert_eq!(retained[0].frame, live.overlay_data);
    }
    assert!(
        scene
            .runtime
            .simulation
            .prepare_fog_view_for("VERA-OBSERVER")
    );
    probe.compare(&scene, &grown, after);
}

#[test]
#[ignore = "requires retail GoldSt and GPU; native flat ore through actual harvest, frame publication and ordinary body submission"]
fn retail_ore_harvest_publication_matches_original_cell_raster() {
    use crate::app::presentation::overlay_index::OverlayRenderIndex;
    use crate::sim::command::{Command, CommandEnvelope};
    let root = PathBuf::from(std::env::var_os("RA2_DIR").expect("physical retail root"));
    let mut scene = crate::headless_scenario::load(&root, "GoldSt.mmx", 0x12345678).unwrap();
    let packet = packet();
    let mut probe = Probe::new(&scene, &packet, Some((123, 33)), None);
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
    let mut probe = Probe::new(&scene, &packet, Some((123, 33)), None);
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
