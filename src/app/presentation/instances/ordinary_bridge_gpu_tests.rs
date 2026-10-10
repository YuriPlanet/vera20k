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
use super::{CellOverlayInputs, build_cell_overlay_instances};
use crate::app::presentation::lighting::MatchLighting;
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::headless_scenario::HeadlessScenario;
use crate::map::lighting::{CellLightGrid, parse_lighting};
use crate::map::overlay::OverlayEntry;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::render::batch::{BatchRenderer, InstanceBufferPool, SpriteInstance};
use crate::render::overlay_atlas::{OverlayAtlas, OverlaySpriteKey, build_overlay_atlas_on_device};
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded};
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
        let mut names = BTreeMap::new();
        for entry in &entries {
            names.insert(
                entry.overlay_id,
                crate::render::overlay_assets::resolve_overlay_name_for_render(
                    registry,
                    entry.overlay_id,
                )
                .unwrap(),
            );
        }
        crate::app::frontend::skirmish::preregister_runtime_overlay_names(
            registry,
            &rules.crate_rules,
            &mut names,
        );
        let gpu = Gpu::new();
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, FORMAT);
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
        )
        .expect("production preloads the physical low-bridge variants");
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
        camera: [f32; 2],
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
                tiberium_types: Some(&scene.runtime.resources.rules.tiberium_types),
                terrain: Some(&self.terrain),
                heights: &heights,
                lighting: self.lighting.grid(),
                camera,
                viewport: SIZE.map(|value| value as f32),
                origin_y: 0.0,
                world_height: f32::from(self.terrain.height()) * 30.0,
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
        self.batch.write_camera(&self.gpu.queue, view);
        let color = self.gpu.target(SIZE, FORMAT);
        let depth = self.gpu.target(SIZE, wgpu::TextureFormat::Depth32Float);
        let cv = color.create_view(&Default::default());
        let dv = depth.create_view(&Default::default());
        let mut pool = InstanceBufferPool::new();
        pool.upload_on_device(&self.gpu.device, &self.gpu.queue, "overlay", &instances);
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
                &policies,
            );
        }
        let reads = [
            self.gpu.read(&mut encoder, &color),
            self.gpu.read(&mut encoder, &depth),
        ];
        let output = self.gpu.finish(encoder, &reads, SIZE);
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
                for pixel in rgba.chunks_exact_mut(4) {
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
