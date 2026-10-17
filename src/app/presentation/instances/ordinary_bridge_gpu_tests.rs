//! Physical ground bridges through the ordinary overlay owner and GPU.
//! Run the ignored tests with RA2_DIR and optional VERA20K_ANYTOWN_MAP or
//! VERA20K_SHRAPNEL_MAP. VERA20K_ANYTOWN_RENDER_OUTPUT and
//! VERA20K_SHRAPNEL_RENDER_OUTPUT retain PNGs and source/readback hashes.
//!
//! Native state authority: tools/spatial_oracle/anytown_damage/navigation.json.gz
//! and tools/spatial_oracle/shrapnel_damage/. Physical SHP bytes establish the
//! source frames/stencils. Rows are drawn individually on a clear target: this
//! witnesses publication-to-pixels, not original whole-frame composition,
//! terrain painting, scene timing, shroud composition or an independent RGB oracle.
//! The opt-in starting-MTNK test additionally consumes low_concrete_bridge_render.json
//! for independent body/shadow/composite RGB565/Z controls and checks live A.
use super::{CellOverlayInputs, build_cell_overlay_instances};
use crate::app::presentation::lighting::MatchLighting;
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::headless_scenario::HeadlessScenario;
use crate::map::lighting::{CellLightGrid, parse_lighting};
use crate::map::overlay::OverlayEntry;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::render::batch::{BatchRenderer, CameraUniform, InstanceBufferPool, SpriteInstance};
use crate::render::bridge_atlas::{BridgeAtlas, build_bridge_atlas};
use crate::render::overlay_atlas::{OverlayAtlas, OverlaySpriteKey, build_overlay_atlas_on_device};
use crate::render::shroud_buffer::ShroudBuffer;
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{
    Gpu, assert_native_color_and_depth, camera, clear, encoded,
};
use crate::rules::ini_parser::IniFile;
use crate::rules::terrain_rules::LandType;
use crate::util::sha256::sha256_hex;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

const SIZE: [u32; 2] = [256, 160];
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

#[derive(Clone, Copy)]
enum Fixture {
    AnytownConcrete,
    ShrapnelWood,
}

impl Fixture {
    fn center(self) -> (u16, u16) {
        match self {
            Self::AnytownConcrete => (87, 54),
            Self::ShrapnelWood => (115, 59),
        }
    }

    fn rows(self) -> std::ops::RangeInclusive<u16> {
        match self {
            Self::AnytownConcrete => 54..=54,
            Self::ShrapnelWood => 58..=60,
        }
    }

    fn theater(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Self::AnytownConcrete => ("TEMPERATE", "tem", "temperat.mix", "isotem.pal"),
            Self::ShrapnelWood => ("SNOW", "sno", "isosnow.mix", "isosno.pal"),
        }
    }

    fn level(self) -> u8 {
        match self {
            Self::AnytownConcrete => 4,
            Self::ShrapnelWood => 2,
        }
    }

    fn overlay(self, phase: &str, row: u16) -> u8 {
        // Original controller/repair stores, including native MapGen outputs.
        match (self, phase) {
            (Self::AnytownConcrete, "loaded") => 216,
            (Self::AnytownConcrete, "damaged") => 220,
            (Self::AnytownConcrete, "collapsed") => 232,
            (Self::AnytownConcrete, "repaired") => 215,
            (Self::ShrapnelWood, "loaded" | "collapsed") => [90, 101, 91][usize::from(row - 58)],
            (Self::ShrapnelWood, "healthy") => [84, 84, 85][usize::from(row - 58)],
            (Self::ShrapnelWood, "damaged") => [87, 89, 88][usize::from(row - 58)],
            (Self::ShrapnelWood, "repaired") => [85, 83, 84][usize::from(row - 58)],
            _ => panic!("unknown physical bridge stage {phase}"),
        }
    }

    fn stages(self) -> &'static [&'static str] {
        match self {
            Self::AnytownConcrete => &["loaded", "damaged", "collapsed", "repaired"],
            Self::ShrapnelWood => &["loaded", "healthy", "damaged", "collapsed", "repaired"],
        }
    }

    fn output_env(self) -> &'static str {
        match self {
            Self::AnytownConcrete => "VERA20K_ANYTOWN_RENDER_OUTPUT",
            Self::ShrapnelWood => "VERA20K_SHRAPNEL_RENDER_OUTPUT",
        }
    }

    fn key(self, phase: &str, row: u16) -> String {
        match self {
            Self::AnytownConcrete => phase.to_owned(),
            Self::ShrapnelWood => format!("{phase}-row{row}"),
        }
    }
}

/// Bounds from the physical SNOW/TEMPERATE frame 1. Overlay ordinals come
/// from original 00668CE3 type-list construction, retained in shrapnel_repair's
/// native_inputs.overlays; the authored INI key numbers are not these ordinals.
/// Both fully collapsed center identities have six empty frames, while the
/// wooden 90/91 boundary rows retain their end stubs.
fn physical_frame_bounds(id: u8, name: &str) -> (u16, u16, u16, u16) {
    let (expected_name, bounds) = match id {
        83 => ("LOBRDG10", (31, 19, 119, 70)),
        84 => ("LOBRDG11", (32, 19, 119, 70)),
        85 => ("LOBRDG12", (32, 19, 118, 70)),
        87 => ("LOBRDG14", (32, 19, 119, 70)),
        88 => ("LOBRDG15", (31, 19, 119, 70)),
        89 => ("LOBRDG16", (32, 19, 119, 70)),
        90 => ("LOBRDG17", (26, 19, 124, 70)),
        91 => ("LOBRDG18", (30, 23, 114, 66)),
        101 => ("LOBRDG28", (0, 0, 0, 0)),
        215 => ("LOBRDB11", (30, 19, 120, 70)),
        216 => ("LOBRDB12", (30, 19, 120, 70)),
        220 => ("LOBRDB16", (30, 19, 120, 70)),
        232 => ("LOBRDB28", (0, 0, 0, 0)),
        _ => panic!("unrecorded physical bridge art {id}"),
    };
    assert_eq!(name, expected_name, "native overlay ordinal {id}");
    bounds
}

struct Probe {
    fixture: Fixture,
    gpu: Gpu,
    batch: BatchRenderer,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    live_a: Option<(ShroudBuffer, crate::sim::intern::InternedId)>,
    shadow: Option<(BridgeAtlas, TerrainDrawRenderer)>,
    atlas: OverlayAtlas,
    assets: AssetManager,
    palette: Palette,
    rules_ini: IniFile,
    terrain: ResolvedTerrainGrid,
    entries: Vec<OverlayEntry>,
    names: BTreeMap<u8, String>,
    lighting: MatchLighting,
    camera: [f32; 2],
    physical_inputs: Value,
    readbacks: BTreeMap<String, Vec<u8>>,
    receipts: Vec<Value>,
}

impl Probe {
    fn new(scene: &HeadlessScenario, fixture: Fixture) -> Self {
        Self::configured(scene, fixture, SIZE, FORMAT, false)
    }

    fn configured(
        scene: &HeadlessScenario,
        fixture: Fixture,
        size: [u32; 2],
        format: wgpu::TextureFormat,
        native: bool,
    ) -> Self {
        let root = PathBuf::from(std::env::var_os("RA2_DIR").expect("physical retail root"));
        let mut assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL).unwrap();
        let mode = IniFile::from_bytes(assets.get_ref("MPBattleMD.ini").unwrap()).unwrap();
        let (_, rules_ini, _, _) = crate::app::loading::init_helpers::load_rules_with_merged_ini(
            &assets,
            Some(&mode),
            Some(&scene.map.ini),
        )
        .unwrap();
        let theater = crate::map::theater::load_theater(&mut assets, &scene.map.header.theater)
            .expect("activate the physical map's theater before name lookup");
        let center = fixture.center();
        assert_eq!(scene.map.header.theater, fixture.theater().0);
        let entries: Vec<_> = scene
            .map
            .overlays
            .iter()
            .filter(|entry| {
                fixture.rows().contains(&entry.ry)
                    && (center.0 - 1..=center.0 + 1).contains(&entry.rx)
            })
            .cloned()
            .collect();
        assert_eq!(entries.len(), 3 * fixture.rows().count(), "authored rows");
        for entry in &entries {
            assert_eq!(
                (entry.overlay_id, entry.frame),
                (
                    fixture.overlay("loaded", entry.ry),
                    (entry.rx + 1 - center.0) as u8
                )
            );
        }
        let rules = &scene.runtime.resources.rules;
        let registry = &scene.runtime.resources.overlay_registry;
        let names = crate::render::overlay_assets::render_overlay_names(registry);
        let gpu = Gpu::new();
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
        .expect("production preloads the physical low-bridge variants");
        let live_a = native.then(|| {
            let shp = ShpFile::from_bytes(assets.get_ref("SHROUD.SHP").unwrap()).unwrap();
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
                scene.sim().session.current_house.unwrap(),
            )
        });
        let shadow = native.then(|| {
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
            )
            .unwrap();
            let renderer = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
            (atlas, renderer)
        });
        let terrain = scene.sim().resolved_terrain.as_ref().unwrap().clone();
        let mut lighting = MatchLighting::default();
        lighting.install(
            CellLightGrid::new(),
            parse_lighting(&scene.map.ini),
            2,
            Some((&terrain, scene.sim(), rules)),
        );
        let palette = assets.resolve_ref(fixture.theater().3).unwrap();
        let profile = crate::map::lighting::normal_profile_units(&parse_lighting(&scene.map.ini));
        let physical_inputs = json!({
            "theater":scene.map.header.theater,
            "palette":{"file":fixture.theater().3,"archive":palette.source_archive,
                "sha256":sha256_hex(palette.bytes)},
            "map_lighting":{"ambient":profile.ambient_percent,
                "rgb":[profile.red_percent,profile.green_percent,profile.blue_percent],
                "ground":profile.ground_units,"level":profile.level_units}
        });
        let level = scene
            .sim()
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(center.0, center.1)
            .unwrap()
            .level;
        assert_eq!(level, fixture.level());
        let point = crate::map::terrain::iso_to_screen(center.0, center.1, level);
        let camera = [
            point.0 + 30.0 - SIZE[0] as f32 / 2.0,
            point.1 + 15.0 - SIZE[1] as f32 / 2.0,
        ];
        Self {
            fixture,
            gpu,
            batch,
            size,
            format,
            live_a,
            shadow,
            atlas,
            assets,
            palette: theater.iso_palette,
            rules_ini,
            terrain,
            entries,
            names,
            lighting,
            camera,
            physical_inputs,
            readbacks: BTreeMap::new(),
            receipts: Vec::new(),
        }
    }

    fn instances(
        &self,
        scene: &HeadlessScenario,
        entries: &[OverlayEntry],
        camera_pos: [f32; 2],
    ) -> (Vec<SpriteInstance>, Vec<RenderZPolicy>) {
        let mut view = camera(self.size);
        view.camera_pos = camera_pos;
        view.world_origin_y = 0.0;
        view.world_height = f32::from(self.terrain.height()) * 30.0;
        self.instances_in_view(scene, entries, &view)
    }

    fn instances_in_view(
        &self,
        scene: &HeadlessScenario,
        entries: &[OverlayEntry],
        view: &CameraUniform,
    ) -> (Vec<SpriteInstance>, Vec<RenderZPolicy>) {
        let mut instances = Vec::new();
        let mut policies = Vec::new();
        let heights = self.terrain.build_height_map();
        build_cell_overlay_instances(
            &CellOverlayInputs {
                // Keep the initial map identities throughout; the production
                // owner must select the live grid's changed identity and data.
                entries,
                atlas: &self.atlas,
                names: &self.names,
                live_grid: scene.sim().overlay_grid.as_ref(),
                registry: Some(&scene.runtime.resources.overlay_registry),
                rules: None,
                interner: None,
                tiberium_types: Some(&scene.runtime.resources.rules.tiberium_types),
                terrain: Some(&self.terrain),
                heights: &heights,
                lighting: self.lighting.grid(),
                scenario_lighting: &scene.sim().session.lighting,
                camera: view.camera_pos,
                viewport: self.size.map(|value| value as f32),
                origin_y: view.world_origin_y,
                world_height: view.world_height,
            },
            &mut instances,
            &mut policies,
        );
        (instances, policies)
    }

    fn observe(&mut self, phase: &str, scene: &HeadlessScenario) {
        for row in self.fixture.rows() {
            self.observe_row(phase, row, scene);
        }
    }

    fn observe_row(&mut self, phase: &str, row: u16, scene: &HeadlessScenario) {
        let sim = scene.sim();
        let before = sim.state_hash();
        let rules = &scene.runtime.resources.rules;
        self.lighting.refresh(&self.terrain, sim, rules, 2);
        let id = self.fixture.overlay(phase, row);
        let center = self.fixture.center();
        let entries: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.ry == row)
            .cloned()
            .collect();
        let mut cells = Vec::new();
        for x in center.0 - 1..=center.0 + 1 {
            let live = sim.overlay_grid.as_ref().unwrap().cell(x, row);
            let cell = sim.resolved_terrain.as_ref().unwrap().cell(x, row).unwrap();
            assert_eq!(
                (live.overlay_id, live.overlay_data),
                (Some(id), (x + 1 - center.0) as u8),
                "{phase}"
            );
            assert_eq!(cell.bridge_facts.overlay_id, live.overlay_id, "{phase}");
            assert!(
                !cell.bridge_facts.has_structural_bridge(),
                "ground-surface family"
            );
            assert_eq!((cell.level, cell.final_tile_index, cell.final_sub_tile), {
                let initial = self.terrain.cell(x, row).unwrap();
                (
                    initial.level,
                    initial.final_tile_index,
                    initial.final_sub_tile,
                )
            });
            assert_eq!(cell.level, self.fixture.level());
            let land = if matches!(id, 101 | 232) {
                LandType::Water
            } else {
                LandType::Road
            };
            assert_eq!(cell.yr_cell_land_type, land.as_index(), "{phase} {x},{row}");
            cells.push(json!({"coord":[x,row],"level":cell.level,
                "tile":cell.final_tile_index,"subtile":cell.final_sub_tile,
                "land":cell.yr_cell_land_type,"raw_bridge_flags":cell.bridge_facts.raw_flags}));
        }
        let name = &self.names[&id];
        assert_eq!(
            rules.art().resolve_overlay_image_id(name, &self.rules_ini),
            *name
        );
        let filename = format!("{}.{}", name.to_ascii_lowercase(), self.fixture.theater().1);
        let asset = self
            .assets
            .resolve_ref(&filename)
            .expect("active theater lookup");
        assert!(
            asset
                .source_archive
                .to_ascii_lowercase()
                .contains(self.fixture.theater().2)
        );
        let source = json!({"file":filename,"archive":asset.source_archive,
            "entry_id":format!("0x{:08X}",asset.entry_id as u32),
            "sha256":sha256_hex(asset.bytes),"bytes":asset.bytes.len()});
        let shp = ShpFile::from_bytes(asset.bytes).unwrap();
        assert_eq!((shp.width, shp.height, shp.frames.len()), (180, 120, 6));
        let frame = &shp.frames[1];
        for frame_index in [0, 2] {
            assert_eq!(
                (
                    shp.frames[frame_index].frame_width,
                    shp.frames[frame_index].frame_height
                ),
                (0, 0)
            );
            assert!(
                self.atlas
                    .get(&OverlaySpriteKey {
                        name: name.clone(),
                        frame: frame_index as u8
                    })
                    .is_none(),
                "flank must not substitute populated body art"
            );
        }
        let bounds = physical_frame_bounds(id, name);
        assert_eq!(
            (
                frame.frame_x,
                frame.frame_y,
                frame.frame_width,
                frame.frame_height
            ),
            bounds
        );
        let visible = bounds.2 != 0;
        if !visible {
            assert!(
                shp.frames
                    .iter()
                    .all(|frame| frame.frame_width == 0 && frame.frame_height == 0)
            );
            assert!(
                self.atlas
                    .get(&OverlaySpriteKey {
                        name: name.clone(),
                        frame: 1
                    })
                    .is_none()
            );
        } else {
            assert!(
                self.atlas
                    .get(&OverlaySpriteKey {
                        name: name.clone(),
                        frame: 1
                    })
                    .is_some()
            );
        }

        if phase == "loaded" {
            let owner = sim.session.current_house.unwrap();
            assert!(
                !sim.fog.is_cell_revealed(owner, center.0, row),
                "the physical bridge witness must begin with an unexplored anchor"
            );
        }
        assert!(
            self.instances(scene, &entries, [1_000_000.0; 2])
                .0
                .is_empty(),
            "camera rejection uses production admission"
        );
        let (instances, policies) = self.instances(scene, &entries, self.camera);
        assert_eq!(
            instances.len(),
            usize::from(visible),
            "{phase}: only center frame1 owns art, independently of anchor exploration"
        );
        assert!(
            policies
                .iter()
                .all(|policy| *policy == RenderZPolicy::ReadWrite)
        );
        // Common 180x120 body canvas, centered on the cell. Keep each row at its
        // actual map offset in the shared camera rather than recentering it.
        let dy = i32::from(row) - i32::from(center.1);
        let left = SIZE[0] as i32 / 2 - 90 + i32::from(bounds.0) - 30 * dy;
        let top = SIZE[1] as i32 / 2 - 60 + i32::from(bounds.1) + 15 * dy;
        if let Some(instance) = instances.first() {
            assert_eq!(
                [
                    instance.position[0] - self.camera[0],
                    instance.position[1] - self.camera[1]
                ],
                [left as f32, top as f32]
            );
            assert_eq!(instance.size, [f32::from(bounds.2), f32::from(bounds.3)]);
            assert_eq!(
                instance.palette_light,
                crate::render::palette_light::PaletteLight::cell(
                    self.lighting.grid(),
                    (center.0, row),
                    false
                )
            );
        }
        let mut view = camera(SIZE);
        view.camera_pos = self.camera;
        let output = self.render(&instances, &policies, view, None);
        let mut opaque = 0usize;
        for y in 0..SIZE[1] as usize {
            for x in 0..SIZE[0] as usize {
                let local_x = x as i32 - left;
                let local_y = y as i32 - top;
                let index = if visible
                    && (0..i32::from(bounds.2)).contains(&local_x)
                    && (0..i32::from(bounds.3)).contains(&local_y)
                {
                    frame.pixels[local_y as usize * usize::from(bounds.2) + local_x as usize]
                } else {
                    0
                };
                let offset = (y * SIZE[0] as usize + x) * 4;
                let expected = if index == 0 {
                    [255; 4]
                } else {
                    opaque += 1;
                    let rgb = self.palette.colors[usize::from(index)];
                    // Existing native-backed LightConvert owner supplies the
                    // expected source color. Clear tactical A is 127
                    // (006D3F9F), matching palette_light.wgsl; A=0 selects the
                    // black row. This is GPU replay consistency, not an
                    // independent gamemd whole-scene RGB oracle.
                    encoded(
                        instances[0]
                            .palette_light
                            .rgb565([rgb.r, rgb.g, rgb.b], index, 127),
                        FORMAT,
                    )
                };
                assert_eq!(
                    &output[0][offset..offset + 4],
                    expected,
                    "{phase} pixel{x},{y}"
                );
                let z = crate::render::native_z::stored_z(f32::from_le_bytes(
                    output[1][offset..offset + 4].try_into().unwrap(),
                ));
                assert_eq!(
                    z != 65535,
                    index != 0,
                    "{phase}: source stencil owns color/depth at{x},{y}"
                );
            }
        }
        assert_eq!(
            opaque,
            frame.pixels.iter().filter(|&&index| index != 0).count()
        );
        assert_eq!(
            sim.state_hash(),
            before,
            "rendering cannot mutate simulation"
        );
        let light = self.lighting.grid().cell_light_at((center.0, row)).unwrap();
        let receipt = json!({"phase":phase,"row":row,"frame":sim.session.binary_frame,
            "overlay":id,"overlay_data":[0,1,2],"source":source,
            "source_bounds":[bounds.0,bounds.1,bounds.2,bounds.3],"cells":cells,
            "cell_light":{"rgb_key":light.rgb_key,"common_scalar":light.common_scalar},
            "source_indices_sha256":sha256_hex(&frame.pixels),
            "instances":instances.len(),"opaque_pixels":opaque,
            "color_sha256":sha256_hex(&output[0]),"depth_sha256":sha256_hex(&output[1])});
        eprintln!("ORDINARY_BRIDGE_RENDER {receipt}");
        self.receipts.push(receipt);
        self.readbacks
            .insert(self.fixture.key(phase, row), output[0].clone());
    }

    /// One upload/submission/readback route for the historical fixture and
    /// the independent native raster comparison. Shadow uses the existing
    /// destination-edit dispatcher; no shadow raster is implemented here.
    fn render(
        &mut self,
        instances: &[SpriteInstance],
        policies: &[RenderZPolicy],
        view: CameraUniform,
        shadows: Option<&[SpriteInstance]>,
    ) -> Vec<Vec<u8>> {
        self.batch.write_camera(&self.gpu.queue, view);
        let color = self.gpu.target(self.size, self.format);
        let depth = self
            .gpu
            .target(self.size, wgpu::TextureFormat::Depth32Float);
        let cv = color.create_view(&Default::default());
        let dv = depth.create_view(&Default::default());
        let mut pool = InstanceBufferPool::new();
        pool.upload_on_device(&self.gpu.device, &self.gpu.queue, "overlay", instances);
        if let Some(shadows) = shadows {
            pool.upload_on_device(&self.gpu.device, &self.gpu.queue, "bridge-shadow", shadows);
            self.shadow
                .as_mut()
                .unwrap()
                .1
                .prepare(&self.gpu.device, &color, &dv, view);
        }
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        clear(
            &mut encoder,
            &cv,
            &dv,
            65535,
            wgpu::LoadOp::Clear(wgpu::Color::WHITE),
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Physical ground bridge ordinary overlay submission"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &cv,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &dv,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            crate::app::presentation::render::draw_pooled_overlay_bodies(
                &mut pass,
                &self.batch,
                &pool,
                Some(&self.atlas),
                "overlay",
                policies,
            );
        }
        if let Some(shadows) = shadows {
            let (atlas, renderer) = self.shadow.as_mut().unwrap();
            renderer.note_external_passes(&mut encoder, 1);
            crate::app::presentation::render::draw_bridge_shadows(
                &mut encoder,
                &cv,
                &dv,
                renderer,
                [0, 0, self.size[0], self.size[1]],
                &self.batch,
                pool.get("bridge-shadow"),
                shadows,
                Some(&atlas.texture),
            );
        }
        let reads = [
            self.gpu.read(&mut encoder, &color),
            self.gpu.read(&mut encoder, &depth),
        ];
        self.gpu.finish(encoder, &reads, self.size)
    }

    fn compare_native(&mut self, scene: &HeadlessScenario, entries: &[OverlayEntry], row: &Value) {
        let before = scene.sim().state_hash();
        let input = &row["input"];
        assert_eq!(input["background"], 65535);
        assert_eq!(input["old_z"], 65535);
        assert_eq!(input["viewport_y"], 0);
        assert_eq!(input["clip"], json!([0, 0, self.size[0], self.size[1]]));
        let coord = (
            input["coords"][0].as_u64().unwrap() as u16,
            input["coords"][1].as_u64().unwrap() as u16,
        );
        assert_eq!(entries.len(), 1);
        assert_eq!((entries[0].rx, entries[0].ry), coord);
        let live = scene
            .sim()
            .overlay_grid
            .as_ref()
            .unwrap()
            .cell(coord.0, coord.1);
        assert_eq!(live.overlay_id.map(i64::from), input["overlay_id"].as_i64());
        assert_eq!(
            u64::from(live.overlay_data),
            input["state"].as_u64().unwrap()
        );
        let terrain = scene.sim().resolved_terrain.as_ref().unwrap();
        let cell = terrain.cell(coord.0, coord.1).unwrap();
        let native = &row["native_cell"];
        assert_eq!(
            (cell.final_tile_index, cell.final_sub_tile, cell.level),
            (
                native["tile"].as_u64().unwrap() as i32,
                native["subtile"].as_u64().unwrap() as u8,
                native["level"].as_u64().unwrap() as u8
            )
        );
        // Original47F6A0/47F510/480110/47FB90/47FDE0 only inspect
        // bit80. Native mission bit2 and ordinary World0 share this draw arm;
        // no equality of their upstream flag producers is asserted.
        assert_eq!(cell.bridge_facts.raw_flags & 0x80, 0);
        assert_eq!(input["flags"], 2);
        self.lighting
            .refresh(terrain, scene.sim(), &scene.runtime.resources.rules, 2);
        let light = self.lighting.grid().cell_light_at(coord).unwrap();
        assert_eq!(
            i64::from(light.common_scalar),
            row["lighting_output"]["common_scalar"].as_i64().unwrap()
        );
        let name = &self.names[&live.overlay_id.unwrap()];
        let source = self.assets.resolve_ref(&format!("{name}.tem")).unwrap();
        let packet = low_concrete_packet();
        let material = packet["inputs"]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|source| {
                source["name"]
                    .as_str()
                    .unwrap()
                    .eq_ignore_ascii_case(&format!("{name}.tem"))
            })
            .unwrap();
        assert_eq!(
            sha256_hex(source.bytes),
            material["sha256"].as_str().unwrap()
        );
        let shp = ShpFile::from_bytes(source.bytes).unwrap();
        let frame = row["shadow"]["draws"][0]["frame"].as_u64().unwrap() as usize;
        assert_eq!(
            (
                shp.frames[frame].frame_width,
                shp.frames[frame].frame_height
            ),
            (0, 0)
        );
        assert_eq!(row["shadow"]["native_rect"][2], 0);
        assert_eq!(row["shadow"]["native_rect"][3], 0);
        let point = crate::map::terrain::iso_to_screen(coord.0, coord.1, 0);
        let mut view = camera(self.size);
        view.camera_pos = [
            point.0 - input["cell_point"][0].as_i64().unwrap() as f32,
            point.1 - input["cell_point"][1].as_i64().unwrap() as f32,
        ];
        let (source, owner) = self.live_a.as_mut().unwrap();
        source.rebuild_if_needed(
            &self.gpu.device,
            &self.gpu.queue,
            &scene.sim().fog,
            *owner,
            view.camera_pos[0],
            view.camera_pos[1],
            self.size[0],
            self.size[1],
            1.0,
        );
        let actual_a = self
            .gpu
            .read_float_texels(source.gpu_source().0, [0, 0], self.size);
        let expected_a = packet["surface"]["a_value"].as_u64().unwrap() as u8;
        assert!(
            actual_a
                .iter()
                .all(|pixel| (pixel[0] * 255.0).round() as u8 == expected_a)
        );
        self.batch.bind_shroud(&self.gpu.device, Some(source));
        let (instances, policies) = self.instances_in_view(scene, entries, &view);
        let mut shadows = Vec::new();
        crate::app::presentation::instances::bridges::build_bridge_shadow_instances_inner(
            terrain,
            &self.shadow.as_ref().unwrap().0,
            &self.names,
            entries,
            scene.sim().overlay_grid.as_ref(),
            Some(&scene.runtime.resources.overlay_registry),
            view.world_origin_y,
            view.world_height,
            view.camera_pos[0],
            view.camera_pos[1],
            self.size[0] as f32,
            self.size[1] as f32,
            &mut shadows,
        );
        assert!(
            shadows.is_empty(),
            "actual selected low-bridge shadow halves are empty"
        );
        for (expected, body, shadow) in [
            (row, true, false),
            (&row["shadow"], false, true),
            (&row["body_then_shadow"], true, true),
        ] {
            let output = self.render(
                if body { &instances } else { &[] },
                if body { &policies } else { &[] },
                view,
                shadow.then_some(shadows.as_slice()),
            );
            assert_native_color_and_depth(&output, expected, self.format, self.size);
        }
        assert_eq!(
            scene.sim().state_hash(),
            before,
            "native comparison presentation is read-only"
        );
    }

    fn finish(self) {
        for row in self.fixture.rows() {
            for phases in self.fixture.stages().windows(2) {
                assert_ne!(
                    self.readbacks[&self.fixture.key(phases[0], row)],
                    self.readbacks[&self.fixture.key(phases[1], row)],
                    "{phases:?} row{row}"
                );
            }
        }
        if let Some(path) = std::env::var_os(self.fixture.output_env()) {
            let path = PathBuf::from(path);
            std::fs::create_dir_all(&path).unwrap();
            for (phase, bgra) in self.readbacks {
                let mut rgba = bgra;
                for pixel in rgba.as_chunks_mut::<4>().0 {
                    pixel.swap(0, 2);
                }
                image::save_buffer(
                    path.join(format!("{phase}.png")),
                    &rgba,
                    SIZE[0],
                    SIZE[1],
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
            serde_json::to_writer_pretty(std::fs::File::create(path.join("receipt.json")).unwrap(),&json!({
                "size":SIZE,"camera":self.camera,"center":self.fixture.center(),
                "physical_inputs":self.physical_inputs,"stages":self.receipts,
                "scope":"Physical authored rows, real damage and ordinary Engineer repair, retained physical atlas, shared ordinary-overlay builder, upload/submission and every color/depth pixel. Rows drawn separately against clear targets; initial anchor exploration is false and does not gate art submission. Camera-rejection controls. Ground level, TMP identity and live Road/Water publication checked; underlying terrain and shroud are not drawn. No original composited frame, native timing or full-scene rendering parity claim."
            })).unwrap();
        }
    }
}

#[test]
#[ignore = "requires physical Anytown, retail assets and an offscreen GPU"]
fn retail_concrete_bridge_publication_reaches_gpu_pixels() {
    let mut probe = None;
    crate::sim::world::bridge_test_evidence::visit_anytown_concrete_stages(|phase, scene| {
        probe
            .get_or_insert_with(|| Probe::new(scene, Fixture::AnytownConcrete))
            .observe(phase, scene);
    });
    probe.expect("all four physical stages executed").finish();
}

#[test]
#[ignore = "requires physical Shrapnel, retail assets and an offscreen GPU"]
fn retail_wood_bridge_publication_reaches_gpu_pixels() {
    let mut probe = None;
    crate::sim::world::bridge_test_evidence::visit_shrapnel_wood_stages(|phase, scene| {
        probe
            .get_or_insert_with(|| Probe::new(scene, Fixture::ShrapnelWood))
            .observe(phase, scene);
    });
    probe.expect("all five physical stages executed").finish();
}

fn low_concrete_packet() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/low_concrete_bridge_render.json")
}

/// Normal starting MTNK Move/AP damage publishes the three low-bridge stages.
/// Native isolated mission epochs/RNG are not copied into this World run.
#[test]
#[ignore = "requires physical retail XMP03T4 and GPU; normal low concrete bridge stages through native Cell RGB/Z and live A"]
fn retail_starting_mtnk_low_concrete_bridge_matches_native_raster() {
    use crate::app::presentation::overlay_index::OverlayRenderIndex;
    let packet = low_concrete_packet();
    let size = [
        packet["surface"]["width"].as_u64().unwrap() as u32,
        packet["surface"]["height"].as_u64().unwrap() as u32,
    ];
    let mut index = OverlayRenderIndex::default();
    let mut probes: Option<Vec<Probe>> = None;
    let mut seen = std::collections::BTreeSet::new();
    crate::sim::world::bridge_test_evidence::visit_starting_mtnk_concrete_attack(
        |phase, scene, output| {
            if let Some(output) = output {
                index.remove_cells(&output.overlay_removals);
                index.upsert_occupied(output.overlay_updates.clone());
            }
            if probes.is_none() {
                index.replace_from_source(scene.map.overlays.clone());
                probes = Some(
                    [
                        wgpu::TextureFormat::Bgra8UnormSrgb,
                        wgpu::TextureFormat::Rgba8UnormSrgb,
                    ]
                    .into_iter()
                    .map(|format| {
                        Probe::configured(scene, Fixture::AnytownConcrete, size, format, true)
                    })
                    .collect(),
                );
            }
            let Some(phase) = phase else {
                return;
            };
            assert!(
                scene
                    .runtime
                    .simulation
                    .prepare_fog_view_for("VERA-OBSERVER")
            );
            let prefix = match phase {
                "loaded" => "loaded_",
                "damaged" => "damaged_",
                "collapsed" => "collapsed_",
                "settled" => return,
                _ => panic!("unknown ordinary bridge phase"),
            };
            for row in packet["rows"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["name"].as_str().unwrap().starts_with(prefix))
            {
                let coord = (
                    row["input"]["coords"][0].as_u64().unwrap() as u16,
                    row["input"]["coords"][1].as_u64().unwrap() as u16,
                );
                // Initial source-map anchors stay indexed. Bridge transactions
                // mirror Terrain/Grid directly and need not emit generic upserts.
                let entries: Vec<_> = index
                    .iter()
                    .filter(|entry| (entry.rx, entry.ry) == coord)
                    .cloned()
                    .collect();
                for probe in probes.as_mut().unwrap() {
                    probe.compare_native(scene, &entries, row);
                }
                assert!(seen.insert(row["name"].as_str().unwrap().to_owned()));
            }
        },
    );
    assert_eq!(
        seen.len(),
        5,
        "two positive bodies and three native empty-body witnesses"
    );
}
