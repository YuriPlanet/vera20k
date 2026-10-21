//! Physical SMKPUFF construction/AI -> Display emitter -> retained GPU replay.
//!
//! Original output planes come from anim_translucency.json. The scene supplies
//! a camera, neutral light and constant color/Z/A destinations; it does not
//! replay a Flak shot or a complete battle. No tested SpriteInstance is supplied
//! or modified: all effect pieces come from build_display_effect_instances.

use super::draw_plan_lowering::{NativeDisplayOrder, ObjectTexture, lower_object_instances};
use super::merge_passes::{ObjectDrawPhase, draw_native_object_pass};
use crate::app::presentation::detail::DetailState;
use crate::app::presentation::instances::{DisplayEffectInputs, build_display_effect_instances};
use crate::assets::{pal_file::Palette, shp_file::ShpFile};
use crate::map::lighting::CellLightGrid;
use crate::render::batch::{BatchRenderer, DepthAxis, InstanceBufferPool};
use crate::render::native_z::{ZGradient, pack_z_gradient, stored_z};
use crate::render::palette_light::PaletteLight;
use crate::render::shroud_buffer::ShroudBuffer;
use crate::render::sprite_atlas::{ShpPaletteContext, ShpSpriteKey, build_sprite_atlas};
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded};
use crate::rules::house_colors::HouseColorIndex;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::world::{FrameEffects, Simulation};
use serde_json::{Value, json};
use std::cell::RefCell;

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn color_words(row: &Value, count: usize) -> Vec<u16> {
    let mut words = Vec::with_capacity(count);
    for run in row["color_words_rle"].as_array().unwrap() {
        let length = usize::try_from(run[0].as_u64().unwrap()).unwrap();
        let word = u16::try_from(run[1].as_u64().unwrap()).unwrap();
        assert!(length > 0 && words.len() + length <= count);
        words.resize(words.len() + length, word);
    }
    assert_eq!(words.len(), count, "complete original row-major plane");
    words
}

#[test]
#[ignore = "requires retail RA2/YR assets and GPU; real SMKPUFF emitter against twelve original full color/Z/A planes"]
fn retail_smkpuff_effect_builder_matches_original_translucency() {
    let native = crate::test_fixture::json("tools/spatial_oracle/anim_translucency.json");
    assert_eq!(native["schema"], 1);
    let inputs = &native["inputs"];
    let size: [u32; 2] = std::array::from_fn(|i| inputs["size"][i].as_u64().unwrap() as u32);
    let clip: [u32; 4] = std::array::from_fn(|i| inputs["clip"][i].as_u64().unwrap() as u32);
    let point: [f32; 2] = std::array::from_fn(|i| int(&inputs["screen_point"][i]) as f32);
    let coord = AnimWorldCoord {
        x: int(&inputs["world_coord"][0]),
        y: int(&inputs["world_coord"][1]),
        z: int(&inputs["world_coord"][2]),
    };
    assert_eq!(size, [160, 160]);
    assert_eq!(clip, [0, 0, size[0], size[1]]);
    let pixels = (size[0] * size[1]) as usize;
    let cases = native["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 3);

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let mut rules = retail.rules;
    let roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&rules);
    rules.bind_anim_class_assets(&roots, &assets, "tem", "TEMPERATE");
    let config = rules.art().anim_runtime_config("SMKPUFF").unwrap();
    let physical = ShpFile::from_bytes(assets.get_ref("SMKPUFF.SHP").unwrap()).unwrap();
    let palette = Palette::from_bytes(assets.get_ref("UNITTEM.PAL").unwrap()).unwrap();
    // The production atlas must resolve global ANIM.PAL, not its unit fallback.
    Palette::from_bytes(assets.get_ref("ANIM.PAL").unwrap()).unwrap();
    for case in cases {
        assert_eq!(case["selection"]["selected"], "SMKPUFF");
        let measured = &case["type_reader"];
        assert_eq!(json!(config.art_body_read), measured["art_body_read"]);
        assert_eq!(json!(config.start), measured["start"]);
        assert_eq!(json!(config.loop_start), measured["loop_start"]);
        assert_eq!(json!(config.loop_end), measured["loop_end"]);
        assert_eq!(json!(config.end), measured["end"]);
        assert_eq!(json!(config.rate_logic_frames), measured["rate"]);
        assert_eq!(
            json!(config.raw_shp_frame_count),
            measured["raw_shp_frame_count"]
        );
        assert_eq!(
            json!(physical.frames.len()),
            measured["raw_shp_frame_count"]
        );
        assert_eq!(json!(config.shadow), measured["shadow"]);
        assert_eq!(json!(config.flat), measured["flat"]);
        assert_eq!(json!(config.translucent), measured["translucent"]);
        assert_eq!(json!(config.translucency), measured["translucency"]);
    }

    let (rx, ry, sub_x, sub_y, height) = coord.to_cell_sub_z();
    let projected = crate::util::lepton::lepton_to_screen_exact_z(rx, ry, sub_x, sub_y, coord.z);
    let view_camera = [projected.0 - point[0], projected.1 - point[1]];
    let lighting = CellLightGrid::new();
    let house_colors = Default::default();
    let hidden = Default::default();
    let gpu = Gpu::new();
    let mut differences = Vec::new();
    let mut transport_differences = Vec::new();
    let mut compared = 0;
    let mut opaque_controls = 0;

    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut camera_view = camera(size);
        camera_view.camera_pos = view_camera;
        batch.write_camera(&gpu.queue, camera_view);
        let empty = Simulation::new();
        let atlas = build_sprite_atlas(
            &gpu.device,
            &gpu.queue,
            &batch,
            empty.entities(),
            &assets,
            &palette,
            "tem",
            "TEMPERATE",
            Some(&rules),
            Some(rules.art()),
            &house_colors,
            &[],
            &Default::default(),
            &Default::default(),
            None,
            None,
            Some(empty.interner()),
        )
        .expect("physical retail effect atlas");
        for case in cases {
            let name = case["input"]["name"].as_str().unwrap();
            let background = u16::try_from(case["input"]["background"].as_u64().unwrap()).unwrap();
            let old_z = u16::try_from(case["input"]["initial_z"].as_u64().unwrap()).unwrap();
            let a_value = u8::try_from(case["input"]["a_value"].as_u64().unwrap()).unwrap();
            let fog = ShroudBuffer::fixture(
                &gpu.device,
                &gpu.queue,
                size,
                view_camera,
                &vec![a_value; pixels],
            );
            batch.bind_shroud(&gpu.device, Some(&fog));
            let mut sim = Simulation::with_seed(inputs["scenario_seed"].as_u64().unwrap());
            let mut clock = inputs["frame"].as_u64().unwrap() as u32;
            sim.session_mut_for_test().set_binary_frame_for_test(clock);
            let type_id = sim.intern("SMKPUFF");
            let descriptor = AnimClassSpawnDescriptor {
                draw_flags: case["input"]["constructor_flags"].as_u64().unwrap() as u32,
                delay: inputs["delay"].as_u64().unwrap() as u16,
                loop_count: int(&inputs["loop_count"]),
                ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, height)
            };
            let id = sim.spawn_anim_at_world(&rules, descriptor, coord).unwrap();
            let display_layer = sim
                .display_layers()
                .layer_of(id)
                .expect("constructor registers Display");
            assert_eq!(sim.anim(id).unwrap().runtime().current_frame, 0);
            let mut visits = 0;
            let draws = case["draws"].as_array().unwrap();
            assert_eq!(
                draws
                    .iter()
                    .map(|draw| int(&draw["stage"]))
                    .collect::<Vec<_>>(),
                [0, 5, 9, 13]
            );
            for draw in draws {
                let stage = int(&draw["stage"]);
                let target_visits = draw["ai_visits"].as_u64().unwrap();
                while visits < target_visits {
                    clock += 1;
                    sim.session_mut_for_test().set_binary_frame_for_test(clock);
                    sim.visit_anim(id, &rules, None, FrameEffects::default());
                    visits += 1;
                }
                assert_eq!(
                    u64::from(clock),
                    draw["clock"].as_u64().unwrap(),
                    "{name} stage{stage}"
                );
                let anim = sim.anim(id).expect("sampled stage remains live");
                assert_eq!(
                    anim.runtime().current_frame,
                    stage,
                    "actual AI reaches native stage"
                );
                assert_eq!(
                    anim.draw_flags(),
                    draw["anim_draw_flags"].as_u64().unwrap() as u32
                );
                assert_eq!(sim.anim_absolute_coord(id), Some(coord));
                assert_eq!(sim.display_layers().layer_of(id), Some(display_layer));
                let call = &draw["call"];
                assert_eq!(call["frame"], stage);
                assert_eq!(call["point"], inputs["screen_point"]);
                assert_eq!(call["clip"], inputs["clip"]);
                assert_eq!(call["has_zshape"], false);
                assert_eq!(call["color"], 0);
                assert_eq!(call["gradient"], 2);
                let raw_frame = &physical.frames[stage as usize];
                assert_eq!(
                    json!([physical.width, physical.height]),
                    call["source_frame"]["canvas"]
                );
                assert_eq!(
                    json!([
                        raw_frame.frame_x,
                        raw_frame.frame_y,
                        raw_frame.frame_width,
                        raw_frame.frame_height
                    ]),
                    call["source_frame"]["crop"]
                );
                let key = ShpSpriteKey {
                    palette_context: ShpPaletteContext::GlobalAnim,
                    type_id: "SMKPUFF".into(),
                    facing: 0,
                    frame: stage as u16,
                    house_color: HouseColorIndex(0),
                };
                let entry = atlas
                    .get(&key)
                    .expect("physical SMKPUFF frame with global Anim palette");
                assert_ne!(entry.source_palette, 0, "indexed physical source palette");
                let before_hash = sim.state_hash();
                let before_rng = sim.rng_state();
                let mut detail = DetailState::new();
                for _ in 0..60 {
                    detail.record_logic_visit();
                }
                detail.throttle_tail(0);
                let detail = RefCell::new(detail);
                let order = NativeDisplayOrder::from_display(sim.display_layers());
                let mut parents = Vec::new();
                build_display_effect_instances(
                    &DisplayEffectInputs {
                        interpolation: None,
                        sim: &sim,
                        rules: &rules,
                        camera: view_camera,
                        render_size: size,
                        tactical: clip,
                        zoom: 1.,
                        axis: DepthAxis::NONE,
                        atlas: Some(&atlas),
                        detail: &detail,
                        detail_level: 2,
                        lighting: &lighting,
                        house_colors: &house_colors,
                        hidden_super_anims: &hidden,
                        show_hidden: false,
                        local_owner: None,
                        observe_particles: false,
                    },
                    &mut parents,
                    &order,
                );
                assert_eq!(
                    parents.len(),
                    1,
                    "{name} stage{stage}: one actual Display Anim"
                );
                assert_eq!(parents[0].parent.id, id);
                let layer_index = parents[0].parent.layer.0 as usize;
                assert_eq!(parents[0].pieces.len(), 1, "stock Shadow=no");
                let piece = &parents[0].pieces[0];
                assert_eq!(piece.target, ObjectTexture::ShpPage(entry.page as usize));
                assert_eq!(piece.render_z, RenderZPolicy::ReadOnly);
                let sprite = &piece.instance;
                assert_eq!(sprite.source_palette, entry.source_palette);
                assert_eq!(sprite.uv_origin, entry.uv_origin);
                assert_eq!(sprite.uv_size, entry.uv_size);
                assert_eq!(
                    sprite.size,
                    [
                        f32::from(raw_frame.frame_width),
                        f32::from(raw_frame.frame_height)
                    ]
                );
                assert_eq!(
                    [
                        sprite.position[0] - view_camera[0],
                        sprite.position[1] - view_camera[1],
                    ],
                    [
                        point[0] + f32::from(raw_frame.frame_x) - f32::from(physical.width / 2),
                        point[1] + f32::from(raw_frame.frame_y) - f32::from(physical.height / 2),
                    ],
                    "{name} stage{stage}: original point plus physical crop"
                );
                assert_eq!(sprite.z_adjust, int(&call["z_adjust"]) as f32);
                assert_eq!(
                    sprite.z_gradient,
                    pack_z_gradient(ZGradient::Vertical, false)
                );
                assert_eq!(sprite.tint, [1.; 3]);
                assert_eq!(
                    sprite.palette_light,
                    PaletteLight::plain(53, int(&call["intensity"]))
                );
                let selector = call["flags"].as_u64().unwrap() as u32 & 0xe;
                // Do not stop the baseline replay at the known transport gap:
                // all GPU rows, including opaque controls, must be measured.
                if sprite.draw_state.native_selector_bits() != selector || sprite.alpha != 1. {
                    transport_differences.push(format!("{name} stage{stage} {format:?}: selector{} alpha{} expected selector{selector} alpha1", sprite.draw_state.native_selector_bits(), sprite.alpha));
                }
                let layers = lower_object_instances(parents);
                let layer = &layers[layer_index];
                assert_eq!(layer.instances.len(), 1);
                assert_eq!(layer.owners, [id]);
                assert_eq!(layer.runs.len(), 1);
                let expected_packed = (selector & 6 != 0).then_some(id);
                if layer.runs[0].packed_parent != expected_packed {
                    transport_differences.push(format!(
                        "{name} stage{stage} {format:?}: packed {:?}, expected {expected_packed:?}",
                        layer.runs[0].packed_parent
                    ));
                }
                let mut pool = InstanceBufferPool::new();
                pool.upload_on_device(&gpu.device, &gpu.queue, "smkpuff", &layer.instances);
                let color = gpu.target(size, format);
                // Explicit native underlay, not an expected blend calculation.
                gpu.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &color,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &encoded(background, format).repeat(pixels),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(size[0] * 4),
                        rows_per_image: Some(size[1]),
                    },
                    color.size(),
                );
                let cv = color.create_view(&Default::default());
                let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
                let dv = depth.create_view(&Default::default());
                let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
                terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                clear(&mut encoder, &cv, &dv, old_z, wgpu::LoadOp::Load);
                draw_native_object_pass(
                    &mut encoder,
                    ObjectDrawPhase::Display,
                    &cv,
                    &dv,
                    &mut terrain,
                    clip,
                    &batch,
                    pool.get("smkpuff"),
                    layer,
                    None,
                    None,
                    None,
                    Some(&atlas),
                    None,
                    batch.default_zshape_bind_group(),
                    None,
                    None,
                );
                let reads = [
                    gpu.read(&mut encoder, &color),
                    gpu.read(&mut encoder, &depth),
                ];
                let output = gpu.finish(encoder, &reads, size);
                let expected = color_words(draw, pixels);
                assert_eq!(draw["z_unchanged"], true);
                assert_eq!(draw["a_unchanged"], true);
                assert_eq!(draw["z_store_count"], 0);
                assert_eq!(draw["a_store_count"], 0);
                assert!(draw["rng_entries"].as_array().unwrap().is_empty());
                let mut color_errors = 0;
                let mut z_errors = 0;
                let mut first_error = None;
                for (at, &word) in expected.iter().enumerate() {
                    let actual: [u8; 4] = output[0][at * 4..at * 4 + 4].try_into().unwrap();
                    let wanted = encoded(word, format);
                    if actual != wanted {
                        color_errors += 1;
                        first_error.get_or_insert((
                            at % size[0] as usize,
                            at / size[0] as usize,
                            actual,
                            wanted,
                        ));
                    }
                    z_errors += usize::from(
                        stored_z(f32::from_le_bytes(
                            output[1][at * 4..at * 4 + 4].try_into().unwrap(),
                        )) != old_z,
                    );
                }
                let a_errors = gpu
                    .read_float_texels(fog.gpu_source().0, [0, 0], size)
                    .iter()
                    .filter(|actual| (actual[0] * 255.).round() as u8 != a_value)
                    .count();
                eprintln!(
                    "SMKPUFF {name} stage{stage} {format:?}: color mismatches {color_errors}/{pixels}, Z {z_errors}, A {a_errors}, first {first_error:?}"
                );
                if color_errors != 0 || z_errors != 0 || a_errors != 0 {
                    differences.push(format!("{name} stage{stage} {format:?}: color={color_errors} Z={z_errors} A={a_errors}"));
                } else if stage == 0 {
                    opaque_controls += 1;
                }
                assert_eq!(
                    sim.state_hash(),
                    before_hash,
                    "emission/lowering/GPU replay is read-only"
                );
                assert_eq!(
                    sim.rng_state(),
                    before_rng,
                    "presentation leaves all simulation RNG streams unchanged"
                );
                compared += 1;
            }
        }
    }
    assert_eq!(
        compared, 24,
        "three surfaces, four actual AI stages, two formats"
    );
    assert_eq!(
        opaque_controls, 6,
        "all unchanged opaque controls must match: {differences:?}"
    );
    assert!(
        differences.is_empty() && transport_differences.is_empty(),
        "native plane differences: {differences:#?}\nselector transport differences: {transport_differences:#?}"
    );
}
