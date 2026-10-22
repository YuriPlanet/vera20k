//! Component comparison, not an original whole-paradrop/Display proof.
//! Real E1 construction and the public Anim constructor reproduce the admitted
//! attachment suffix at two declared fixed heights. Only Anim AI advances.
//! Actual SHP emission supplies the real body, then the shared retained Anim
//! traversal produces the piece replayed unchanged against original
//! color/Z/A planes. The ordinary whole producer is covered separately by the
//! stock AMRADR lifecycle test. Original inputs/results are retained separately
//! in tools/spatial_oracle/parachute_canopy.json.

use super::draw_plan_lowering::{
    NativeDisplayOrder, ObjectTexture, PlannedObjectInstance, lower_object_instances,
};
use super::merge_passes::{ObjectDrawPhase, draw_native_object_pass};
use crate::app::presentation::detail::DetailState;
use crate::app::presentation::instances::{
    DisplayEffectInputs, ShpInputs, build_display_effect_instances, build_shp_instances_from_inputs,
};
use crate::app::presentation::interpolation::RenderInterpolation;
use crate::assets::shp_file::ShpFile;
use crate::map::houses::HouseColorMap;
use crate::map::lighting::{CellLightGrid, LightingConfig, build_cell_light_grid_from_heights};
use crate::render::batch::{BatchRenderer, DepthAxis, InstanceBufferPool};
use crate::render::native_z::{ZGradient, pack_z_gradient, stored_z};
use crate::render::palette_light::PaletteLight;
use crate::render::shroud_buffer::ShroudBuffer;
use crate::render::sprite_atlas::{
    ShpPaletteContext, ShpSpriteKey, SpriteAtlas, build_sprite_atlas, collect_anim_remap_base_keys,
};
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded, seed_depth_grid};
use crate::render::unit_atlas::BarrelImagePitches;
use crate::rules::effect_asset_catalog::tolerant_anim_class_roots;
use crate::rules::house_colors::HouseColorIndex;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::{AnimClassSpawnDescriptor, DriveCoord};
use crate::sim::house_state::HouseState;
use crate::sim::movement::ground_pose::position_world_xy;
use crate::sim::movement::parachute_descent::begin_parachute_descent;
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::world::{FrameEffects, Simulation};
use serde_json::{Value, json};
use std::cell::RefCell;

#[path = "parachute_interpolation_tests.rs"]
mod interpolation_tests;

const SIZE: [u32; 2] = [160, 160];
const CLIP: [u32; 4] = [0, 0, 160, 160];
const NATIVE_CAMERA: [f32; 2] = [-384., 380.];
const PIXELS: usize = 160 * 160;
const OWNER: &str = "CanopyComponentOwner";

fn vera_camera() -> [f32; 2] {
    // Every VERA world layer carries the same projection-origin translation
    // (util::lepton::WORLD_ROW_BIAS_PX). Normalize that coordinate convention
    // through its existing owner, without fitting any observed sprite point.
    let origin = crate::util::lepton::absolute_leptons_to_screen(0, 0, 0);
    [NATIVE_CAMERA[0] + origin.0, NATIVE_CAMERA[1] + origin.1]
}

fn integer(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("native signed integer")).unwrap()
}

fn plane(planes: &Value, name: &str) -> Vec<u16> {
    let mut words = Vec::with_capacity(PIXELS);
    for pair in planes[name]
        .as_str()
        .expect("native count:word RLE")
        .split_ascii_whitespace()
    {
        let (count, word) = pair.split_once(':').expect("count:word");
        let count: usize = count.parse().expect("unsigned count");
        let word: u16 = word.parse().expect("uint16 word");
        assert!(count > 0 && count <= PIXELS - words.len());
        words.resize(words.len() + count, word);
    }
    assert_eq!(words.len(), PIXELS);
    words
}

struct Scene {
    runtime: SimRuntime,
    body: u64,
    canopy: u64,
    visits: u32,
    height: i32,
}

impl Scene {
    fn new(rules: RuleSet, height: i32) -> Self {
        let mut sim = Simulation::with_seed(0x5F59_4004);
        sim.session_mut_for_test().set_binary_frame_for_test(1000);
        let owner = sim.intern(OWNER);
        let country = sim.intern("Americans");
        sim.register_house(HouseState::new(owner, 0, Some(country), true, 0, 10));
        let body = sim
            .spawn_object("E1", OWNER, 10, 20, 0, &rules)
            .expect("ordinary E1 constructor/Unlimbo admits this component scene");
        // The native corpus starts after admission with this declared Location, rather
        // than measuring Infantry Unlimbo's subcell choice. Match that input
        // boundary through the Location owner; the complete ordinary producer
        // and its subcell admission remain covered by the lifecycle test.
        sim.entities_mut_for_test()
            .get_mut(body)
            .unwrap()
            .position
            .put_location(DriveCoord {
                x: 2688,
                y: 5248,
                z: height,
            });
        assert!(begin_parachute_descent(
            sim.entities_mut_for_test(),
            body,
            height
        ));
        let entity = sim.entities().get(body).unwrap();
        assert_eq!(position_world_xy(&entity.position), [2688, 5248]);
        assert_eq!(entity.position.exact_z_leptons(), Some(height));
        let coord = AnimWorldCoord {
            x: 2688,
            y: 5248,
            z: height + 75,
        };
        let (rx, ry, sub_x, sub_y, level) = coord.to_cell_sub_z();
        let type_id = sim.intern("PARACH");
        // attach_parachute_anim is core-private. This is its public constructor
        // boundary, not a replacement whole Paradrop or aircraft release call:
        // real SetOwner converts this absolute input to relative (0,0,75).
        let canopy = sim
            .spawn_anim_at_world(
                &rules,
                AnimClassSpawnDescriptor {
                    draw_flags: 0x600,
                    owner: Some(body),
                    ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, level)
                },
                coord,
            )
            .expect("physical PARACH constructor and attachment");
        assert!(sim.anims_mut_for_test().set_anim_house_remap(canopy, owner));
        assert_eq!(sim.anim_absolute_coord(canopy), Some(coord));
        assert_eq!(sim.anim(canopy).unwrap().runtime().current_frame, 0);
        assert!(sim.display_layers().layer_of(body).is_some());
        assert!(sim.display_layers().layer_of(canopy).is_some());
        let mut resources = SimResources::empty();
        resources.rules = rules;
        Self {
            runtime: SimRuntime {
                simulation: sim,
                resources,
            },
            body,
            canopy,
            visits: 0,
            height,
        }
    }

    fn advance_to(&mut self, row: &Value) {
        let requested = u32::try_from(integer(&row["ai_visits"])).unwrap();
        while self.visits < requested {
            self.visits += 1;
            self.runtime
                .simulation
                .session_mut_for_test()
                .set_binary_frame_for_test(1000 + self.visits);
            self.runtime.simulation.visit_anim(
                self.canopy,
                &self.runtime.resources.rules,
                None,
                FrameEffects::default(),
            );
        }
        assert_eq!(self.visits, requested);
        assert_eq!(
            self.runtime
                .simulation
                .anim(self.canopy)
                .unwrap()
                .runtime()
                .current_frame,
            integer(&row["requested_frame"]),
            "frame reached by real Anim AI"
        );
        assert_eq!(
            self.runtime
                .simulation
                .entities()
                .get(self.body)
                .unwrap()
                .position
                .exact_z_leptons(),
            Some(self.height),
            "declared fixed owner; no full fall is executed"
        );
    }

    fn emit(
        &self,
        atlas: &SpriteAtlas,
        light: &CellLightGrid,
        colors: &HouseColorMap,
        frame: u16,
    ) -> PlannedObjectInstance {
        let sim = &self.runtime.simulation;
        let order = NativeDisplayOrder::from_display(sim.display_layers());
        let interpolation = RenderInterpolation::new(false);
        let pitches = RefCell::new(BarrelImagePitches::default());
        let mut parents = Vec::new();
        build_shp_instances_from_inputs(
            &ShpInputs {
                runtime: &self.runtime,
                atlas: Some(atlas),
                overlay_atlas: None,
                unit_atlas: None,
                barrel_image_pitches: &pitches,
                interpolation: &interpolation,
                camera: vera_camera(),
                render_size: SIZE,
                zoom: 1.,
                axis: DepthAxis::NONE,
                local_owner: Some(OWNER),
                ignore_visibility: true,
                lighting: light,
                house_colors: colors,
                building_zshape: false,
            },
            &mut parents,
            &order,
        );
        assert!(
            parents
                .iter()
                .any(|p| p.parent.id == self.body && !p.pieces.is_empty()),
            "actual E1 body, not a prepared empty parent"
        );
        let mut detail = DetailState::new();
        for _ in 0..60 {
            detail.record_logic_visit();
        }
        detail.throttle_tail(0);
        let detail = RefCell::new(detail);
        let hidden = Default::default();
        let input = DisplayEffectInputs {
            interpolation: Some(&interpolation),
            sim,
            rules: &self.runtime.resources.rules,
            camera: vera_camera(),
            render_size: SIZE,
            tactical: CLIP,
            zoom: 1.,
            axis: DepthAxis::NONE,
            atlas: Some(atlas),
            detail: &detail,
            detail_level: 2,
            lighting: light,
            house_colors: colors,
            hidden_super_anims: &hidden,
            show_hidden: false,
            local_owner: Some(sim.interner().get(OWNER).unwrap()),
            observe_particles: false,
        };
        build_display_effect_instances(&input, &mut parents, &order, Default::default());
        let key = ShpSpriteKey {
            palette_context: ShpPaletteContext::SelectedScheme,
            type_id: "PARACH".into(),
            facing: 0,
            frame,
            house_color: HouseColorIndex(0),
        };
        let entry = atlas
            .get(&key)
            .expect("actual selected-scheme PARACH frame");
        let mut selected = Vec::new();
        for parent in parents {
            for piece in parent.pieces {
                if piece.target == ObjectTexture::ShpPage(entry.page as usize)
                    && piece.instance.uv_origin == entry.uv_origin
                    && piece.instance.uv_size == entry.uv_size
                {
                    // Component-only replay: preserve the emitted parent and all
                    // piece fields, omit body/shadow pixels not in this oracle.
                    selected.push(PlannedObjectInstance::object(parent.parent, vec![piece]));
                }
            }
        }
        assert_eq!(selected.len(), 1, "one real canopy piece");
        assert_eq!(
            selected[0].parent.id, self.canopy,
            "the canopy retains its own Anim Display encounter"
        );
        selected.pop().unwrap()
    }
}

#[test]
#[ignore = "requires retail assets and GPU; actual canopy emitter against full native component planes"]
fn retail_parachute_actual_emitter_matches_original_component_planes() {
    let native = crate::test_fixture::json("tools/spatial_oracle/parachute_canopy.json");
    assert_eq!(native["schema"], 1);
    let cases = native["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 2);
    let retail =
        crate::rules::retail_ini_fixture::retail_battle_rules().expect("retail rules required");
    let (_, mut assets) =
        crate::rules::retail_ini_fixture::retail_assets().expect("retail assets required");
    let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let mut rules = retail.rules;
    rules.bind_anim_class_assets(
        &tolerant_anim_class_roots(&rules),
        &assets,
        "tem",
        "TEMPERATE",
    );
    assert_eq!(rules.general.parachute_shp.as_deref(), Some("PARACH"));
    let config = rules.art().anim_runtime_config("PARACH").unwrap();
    assert_eq!(
        (config.loop_start, config.loop_end, config.loop_count),
        (20, 39, 30)
    );
    assert_eq!(config.z_adjust, -10);
    let physical = ShpFile::from_bytes(assets.get_ref("PARACH.SHP").unwrap()).unwrap();
    assert_eq!(
        config.raw_shp_frame_count,
        Some(physical.frames.len() as i32)
    );
    let colors = HouseColorMap::from([(OWNER.to_owned(), HouseColorIndex(0))]);
    // Default Scenario inputs and flat level-zero cell are the native component
    // priors. Real relighting derives intensity; never assign the measured 950.
    let light = build_cell_light_grid_from_heights([((10, 20), 0)], &LightingConfig::default());
    let gpu = Gpu::new();
    let mut differences = Vec::new();
    let mut transports = Vec::new();
    let mut compared = 0;
    for case in cases {
        assert_eq!(
            case["checked_original_text_and_selected_vtables_unchanged"],
            true
        );
        let height = integer(&case["supplied_owner_height"]);
        assert!([104, 232].contains(&height));
        let mut scene = Scene::new(rules.clone(), height);
        let remaps = collect_anim_remap_base_keys(&scene.runtime.simulation, &colors);
        let mut formats: Vec<_> = [
            wgpu::TextureFormat::Bgra8UnormSrgb,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ]
        .into_iter()
        .map(|format| {
            let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
            let atlas = build_sprite_atlas(
                &gpu.device,
                &gpu.queue,
                &batch,
                scene.runtime.simulation.entities(),
                &assets,
                &theater.unit_palette,
                "tem",
                "TEMPERATE",
                Some(&rules),
                Some(rules.art()),
                &colors,
                &[],
                &remaps,
                &Default::default(),
                &Default::default(),
                None,
                None,
                Some(scene.runtime.simulation.interner()),
            )
            .expect("physical E1 and all live PARACH frames");
            (format, batch, atlas)
        })
        .collect();
        let draws = case["draws"].as_array().unwrap();
        assert_eq!(draws.len(), 4);
        for row in draws {
            scene.advance_to(row);
            let sim = &scene.runtime.simulation;
            let before = (sim.state_hash(), sim.rng_state());
            let name = row["name"].as_str().unwrap();
            assert_eq!(row["size"], json!(SIZE));
            assert_eq!(row["camera"], json!([-384, 380]));
            assert_eq!(
                row["state_before"]["owner"]["actual_get_coords"]["value"],
                json!([2688, 5248, height])
            );
            assert_eq!(
                row["state_before"]["anim"]["actual_get_coords"]["value"],
                json!([2688, 5248, height + 75])
            );
            assert!(row["rng_entries"].as_array().unwrap().is_empty());
            assert_eq!(row["a_unchanged"], true);
            assert_eq!(row["z_unchanged"], true);
            let calls = row["calls"].as_array().unwrap();
            assert_eq!(calls.len(), 1);
            let call = &calls[0];
            let frame = u16::try_from(integer(&call["frame"])).unwrap();
            let raw = &physical.frames[usize::from(frame)];
            assert_eq!(
                call["source_frame"]["canvas"],
                json!([physical.width, physical.height])
            );
            assert_eq!(
                call["source_frame"]["crop"],
                json!([raw.frame_x, raw.frame_y, raw.frame_width, raw.frame_height])
            );
            assert_eq!(call["clip"], json!(CLIP));
            assert_eq!(call["has_zshape"], false);
            let old_color = plane(&call["before_planes"], "color_words_rle");
            let old_z = plane(&call["before_planes"], "z_words_rle");
            let old_a = plane(&call["before_planes"], "a_words_rle");
            assert_eq!(old_color, vec![0xBDF7; PIXELS]);
            assert_eq!(old_a, vec![127; PIXELS]);
            let split = name.ends_with("left_half_zero");
            assert_eq!(
                old_z,
                (0..PIXELS)
                    .map(|i| if split && i % 160 < 80 { 0 } else { 65535 })
                    .collect::<Vec<_>>()
            );
            let expected = plane(&call["after_planes"], "color_words_rle");
            let expected_z = plane(&call["after_planes"], "z_words_rle");
            let expected_a = plane(&call["after_planes"], "a_words_rle");
            for (format, batch, atlas) in &mut formats {
                let mut view = camera(SIZE);
                view.camera_pos = vera_camera();
                batch.write_camera(&gpu.queue, view);
                let fog = ShroudBuffer::fixture(
                    &gpu.device,
                    &gpu.queue,
                    SIZE,
                    vera_camera(),
                    &vec![127; PIXELS],
                );
                batch.bind_shroud(&gpu.device, Some(&fog));
                let parent = scene.emit(atlas, &light, &colors, frame);
                let layer_index = parent.parent.layer.0 as usize;
                let piece = &parent.pieces[0];
                let sprite = &piece.instance;
                let point = [
                    integer(&call["point"][0]) as f32,
                    integer(&call["point"][1]) as f32,
                ];
                let wanted_position = [
                    point[0] + f32::from(raw.frame_x) - f32::from(physical.width / 2),
                    point[1] + f32::from(raw.frame_y) - f32::from(physical.height / 2),
                ];
                let actual_position = [
                    sprite.position[0] - vera_camera()[0],
                    sprite.position[1] - vera_camera()[1],
                ];
                if actual_position != wanted_position
                    || piece.render_z != RenderZPolicy::ReadOnly
                    || sprite.z_adjust != integer(&call["z_adjust"]) as f32
                    || sprite.z_gradient != pack_z_gradient(ZGradient::Vertical, false)
                    || sprite.palette_light
                        != PaletteLight::color_scheme([1000; 3], integer(&call["intensity"]))
                {
                    transports.push(format!("{name}/{format:?}: pos {actual_position:?}/{wanted_position:?}, policy {:?}, z {} / {}, gradient {}, light {:?}",
                        piece.render_z, sprite.z_adjust, call["z_adjust"], sprite.z_gradient, sprite.palette_light));
                }
                let layers = lower_object_instances(vec![parent]);
                let layer = &layers[layer_index];
                assert_eq!(layer.instances.len(), 1);
                let mut pool = InstanceBufferPool::new();
                pool.upload_on_device(&gpu.device, &gpu.queue, "canopy", &layer.instances);
                let color = gpu.target(SIZE, *format);
                gpu.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &color,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &encoded(0xBDF7, *format).repeat(PIXELS),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(SIZE[0] * 4),
                        rows_per_image: Some(SIZE[1]),
                    },
                    color.size(),
                );
                let cv = color.create_view(&Default::default());
                let depth = gpu.target(SIZE, wgpu::TextureFormat::Depth32Float);
                let dv = depth.create_view(&Default::default());
                let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, *format, batch);
                terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                clear(&mut encoder, &cv, &dv, u16::MAX, wgpu::LoadOp::Load);
                seed_depth_grid(&gpu, &mut encoder, &dv, &old_z, SIZE[0]);
                draw_native_object_pass(
                    &mut encoder,
                    ObjectDrawPhase::Display,
                    &cv,
                    &dv,
                    &mut terrain,
                    CLIP,
                    batch,
                    pool.get("canopy"),
                    layer,
                    None,
                    None,
                    None,
                    Some(atlas),
                    None,
                    batch.default_zshape_bind_group(),
                    None,
                    None,
                );
                let reads = [
                    gpu.read(&mut encoder, &color),
                    gpu.read(&mut encoder, &depth),
                ];
                let output = gpu.finish(encoder, &reads, SIZE);
                let (mut color_errors, mut z_errors, mut first) = (0, 0, None);
                for (at, &word) in expected.iter().enumerate() {
                    let actual: [u8; 4] = output[0][at * 4..at * 4 + 4].try_into().unwrap();
                    if actual != encoded(word, *format) {
                        color_errors += 1;
                        first.get_or_insert((at % 160, at / 160, actual, encoded(word, *format)));
                    }
                    z_errors += usize::from(
                        stored_z(f32::from_le_bytes(
                            output[1][at * 4..at * 4 + 4].try_into().unwrap(),
                        )) != expected_z[at],
                    );
                }
                let a_errors = gpu
                    .read_float_texels(fog.gpu_source().0, [0, 0], SIZE)
                    .iter()
                    .zip(&expected_a)
                    .filter(|(a, wanted)| (a[0] * 255.).round() as u16 != **wanted)
                    .count();
                eprintln!(
                    "PARACH {name}/{format:?}: color {color_errors}/{PIXELS}, Z {z_errors}, A {a_errors}, first {first:?}"
                );
                if color_errors + z_errors + a_errors != 0 {
                    differences.push(format!(
                        "{name}/{format:?}: color={color_errors} Z={z_errors} A={a_errors}"
                    ));
                }
                compared += 1;
                assert_eq!(
                    (sim.state_hash(), sim.rng_state()),
                    before,
                    "read-only emission and replay"
                );
            }
        }
    }
    assert_eq!(compared, 16, "both heights/frames/underlays/formats");
    assert!(
        differences.is_empty() && transports.is_empty(),
        "full-plane differences: {differences:#?}\ntransport differences: {transports:#?}"
    );
}
