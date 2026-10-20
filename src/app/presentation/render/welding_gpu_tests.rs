//! Whole original System/Particle bodies share one native destination in these
//! controls. The positive route uses actual retained GROUND2 -> AIR3 lowering;
//! reversing those two replays is an explicit component-order negative control.

use super::super::draw_plan_lowering::{
    NativeDisplayOrder, ObjectBodyDraw, PlannedObjectInstance, lower_object_instances,
};
use super::*;
use crate::app::presentation::combat_lights::CombatLight;
use crate::app::presentation::detail::DetailState;
use crate::render::combat_light::CombatLightRenderer;
use crate::render::surface_line::{SurfaceDraw, SurfaceLineViewport};
use crate::render::tactical_compat::{
    SparkPointPlanes, SparkPointPreparation, prepare_spark_point, welding_tests as fixture,
};
use crate::render::tactical_draw_plan::SpriteEncoding;
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, encoded, seed_depth_grid};
use crate::rules::ruleset::DetailRules;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::display_layers::{DisplayLayer, DisplayLayers};

#[test]
#[ignore = "requires retail assets and GPU; ordinary Engineer IFV children through the canonical builder, Detail traversal and shared composition"]
fn ordinary_repair_children_reach_the_actual_effect_builder_and_gpu_without_mutating_simulation() {
    use crate::app::presentation::instances::{
        DisplayEffectInputs, build_display_effect_instances,
    };
    use crate::render::batch::DepthAxis;
    use crate::render::shroud_buffer::ShroudBuffer;
    use crate::render::sprite_atlas::SpriteAtlas;
    use crate::render::terrain_draw_gpu_tests::clear;
    use crate::rules::art_data::AnimLayer;
    use crate::sim::anim_class::AnimWorldCoord;
    use crate::sim::components::AnimClassSpawnDescriptor;
    use std::cell::RefCell;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut rules = retail.rules;
    crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    // A real, bound stock Ground Anim supplies a class boundary after the
    // System. Its image is intentionally absent from this effect-only atlas:
    // its native Detail query precedes the later frame/atlas lookup.
    let candidates: Vec<_> = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&rules)
        .into_iter()
        .filter(|name| {
            rules.art().anim_runtime_config(name).is_some_and(|config| {
                config.art_body_read
                    && config.layer == AnimLayer::Ground
                    && config.y_sort_adjust == 0
                    && !config.psi_warning
            })
        })
        .collect();
    let anim_type = candidates
        .into_iter()
        .find(|name| {
            rules.bind_anim_class_assets(std::slice::from_ref(name), &assets, "tem", "TEMPERATE");
            rules
                .art()
                .anim_runtime_config(name)
                .is_some_and(|config| config.raw_shp_frame_count.is_some())
        })
        .expect("a physically bound stock Ground animation");
    let fixture =
        crate::app::input::context_order::retail_ifv_repair_tests::ordinary_welding_children(
            &rules,
        );
    let mut sim = fixture.sim;
    let system_id = fixture.system;
    assert!(
        sim.anims().next().is_none(),
        "the ordinary fixture has no prior Anim query"
    );
    assert!(
        sim.particle_systems()
            .iter()
            .all(|(&id, system)| id == system_id || system.particles().is_empty()),
        "the selected fixture has no unrelated Particle queries"
    );
    let system = sim.particle_systems().get(system_id).unwrap();
    let at = system.coords();
    let child_ids: Vec<_> = sim
        .display_layers()
        .members(DisplayLayer::AIR)
        .iter()
        .copied()
        .filter(|id| {
            system
                .particles()
                .iter()
                .any(|particle| particle.stable_id() == *id)
        })
        .collect();
    assert_eq!(child_ids.len(), system.particles().len());
    assert!(!child_ids.is_empty());
    assert_eq!(
        sim.display_layers().layer_of(system_id),
        Some(DisplayLayer::GROUND)
    );
    let type_id = sim.interner.intern(&anim_type);
    let anim = sim
        .spawn_anim_at_world(
            &rules,
            AnimClassSpawnDescriptor::new(
                type_id,
                16,
                16,
                crate::util::fixed_math::SIM_ZERO,
                crate::util::fixed_math::SIM_ZERO,
                0,
            ),
            AnimWorldCoord {
                x: at.x + 256,
                y: at.y + 256,
                z: at.z,
            },
        )
        .unwrap();
    let ground = sim.display_layers().members(DisplayLayer::GROUND);
    assert!(
        ground.iter().position(|&id| id == system_id).unwrap()
            < ground.iter().position(|&id| id == anim).unwrap(),
        "retained System -> Anim -> AIR queries"
    );
    let before_hash = sim.state_hash();
    let before_rng = sim.rng_state();
    let size = [160, 120];
    let center = crate::util::lepton::absolute_leptons_to_screen(at.x, at.y, at.z);
    let view_camera = [center.0 - 80., center.1 - 60.];
    let order = NativeDisplayOrder::from_display(sim.display_layers());
    let lighting = crate::map::lighting::CellLightGrid::new();
    let house_colors = Default::default();
    let hidden = Default::default();
    let atlas = SpriteAtlas::from_test_entries(Default::default());
    let build = |detail: &RefCell<DetailState>, atlas: Option<&SpriteAtlas>| {
        let mut parents = Vec::new();
        build_display_effect_instances(
            &DisplayEffectInputs {
                sim: &sim,
                rules: &rules,
                camera: view_camera,
                render_size: size,
                tactical: [0, 0, size[0], size[1]],
                zoom: 1.,
                axis: DepthAxis::NONE,
                atlas,
                detail,
                detail_level: 2,
                lighting: &lighting,
                house_colors: &house_colors,
                hidden_super_anims: &hidden,
                show_hidden: false,
                local_owner: sim.session.current_house(),
                observe_particles: false,
            },
            &mut parents,
            &order,
        );
        assert_eq!(
            sim.state_hash(),
            before_hash,
            "canonical effect builder is read-only"
        );
        assert_eq!(
            sim.rng_state(),
            before_rng,
            "Scenario/Main/MapGen are unchanged"
        );
        parents
    };
    let normal_detail = || {
        let mut detail = DetailState::new();
        for _ in 0..60 {
            detail.record_logic_visit();
        }
        detail.throttle_tail(0);
        RefCell::new(detail)
    };
    let normal = build(&normal_detail(), Some(&atlas));
    let expected: Vec<_> = std::iter::once(system_id)
        .chain(child_ids.iter().copied())
        .collect();
    assert_eq!(
        normal
            .iter()
            .map(|parent| parent.parent.id)
            .collect::<Vec<_>>(),
        expected,
        "actual retained System and child slots feed lowering"
    );
    // Whole ParticleDraw's three-query overflow control supplies the expected
    // shared55AF60 outcomes. An Anim after System must take the middle query;
    // grouping classes before sorting already-built output changes the light.
    let control = fixture::native()["spark_draw_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "unsigned_threshold_wrap")
        .unwrap();
    let overflow_detail = || {
        let mut detail = DetailState::new();
        detail.configure_normal(&DetailRules {
            min_frame_rate_normal: fixture::unsigned(&control["input"]["minimum"]) as i32,
            buffer_zone_width: fixture::unsigned(&control["input"]["buffer"]) as i32,
            ..Default::default()
        });
        detail.throttle_tail(0);
        RefCell::new(detail)
    };
    let detail = overflow_detail();
    let ordered = build(&detail, Some(&atlas));
    let has =
        |parents: &[PlannedObjectInstance], id| parents.iter().any(|parent| parent.parent.id == id);
    assert_eq!(
        has(&ordered, system_id),
        control["draws"][0]["performance_passed"].as_bool().unwrap()
    );
    assert_eq!(
        has(&ordered, child_ids[0]),
        control["draws"][2]["performance_passed"].as_bool().unwrap()
    );
    let calls = child_ids.len() + 2;
    assert_eq!(
        serde_json::to_value(detail.borrow().observation()).unwrap()["reduced"],
        control["draws"][(calls - 1) % 2]["reduced_after"]
    );
    let without_anim_atlas = build(&overflow_detail(), None);
    assert_eq!(
        has(&without_anim_atlas, child_ids[0]),
        control["draws"][1]["performance_passed"].as_bool().unwrap(),
        "Spark's query and draw do not require an SHP atlas"
    );

    let gpu = Gpu::new();
    let background =
        fixture::unsigned(&fixture::native()["system_draw_cases"][0]["input"]["background"]) as u16;
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        batch.write_camera(&gpu.queue, camera(size));
        let mut lights = CombatLightRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        let a = ShroudBuffer::fixture(
            &gpu.device,
            &gpu.queue,
            size,
            view_camera,
            &vec![127; (size[0] * size[1]) as usize],
        );
        let initial_a = gpu.read_float_texels(a.gpu_source().0, [0, 0], size);
        let mut frames = Vec::new();
        for selected in 0..3 {
            let mut parents = build(&normal_detail(), Some(&atlas));
            // Component-removal negatives use the real built commands, never
            // mutate the world or supply a second raster/numeric expectation.
            parents.retain(|parent| match (&parent.body_draw, selected) {
                (Some(ObjectBodyDraw::CombatLight(_)), 1) => false,
                (Some(ObjectBodyDraw::Surface(_)), 2) => false,
                _ => true,
            });
            let mut layers = lower_object_instances(parents);
            let mut records = Vec::new();
            for layer in &mut layers {
                layer.light_record_base = records.len();
                records.extend_from_slice(&layer.light_records);
            }
            lights.prepare_with_device(
                &gpu.device,
                &gpu.queue,
                &records,
                size.map(|value| value as f32),
                view_camera,
                1.,
            );
            let cv = lights.composition_view();
            let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
            let dv = depth.create_view(&Default::default());
            terrain.prepare(
                &gpu.device,
                lights.composition_texture(),
                &dv,
                batch.camera_uniform(),
            );
            terrain.begin_surface_lines();
            for layer in &mut layers {
                layer.line_batches = layer
                    .surface_draws
                    .iter()
                    .map(|draws| {
                        terrain.prepare_surface_lines(
                            &gpu.device,
                            &gpu.queue,
                            std::iter::empty(),
                            draws.iter().copied(),
                            &[],
                            &[],
                            &[],
                            SurfaceLineViewport {
                                camera: view_camera.map(|value| value.floor() as i32),
                                clip: [0, 0, size[0] as i32, size[1] as i32],
                                z_origin_y: 0,
                                zoom: 1.,
                            },
                            |_| false,
                            |[x, y]| {
                                u16::from(
                                    a.sample_world(
                                        x as f32 + view_camera[0].floor(),
                                        y as f32 + view_camera[1].floor(),
                                        view_camera[0],
                                        view_camera[1],
                                    )
                                    .unwrap(),
                                )
                            },
                        )
                    })
                    .collect();
            }
            let initial = encoded(background, format).repeat((size[0] * size[1]) as usize);
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: lights.composition_texture(),
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &initial,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size[0] * 4),
                    rows_per_image: Some(size[1]),
                },
                lights.composition_texture().size(),
            );
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            clear(
                &mut encoder,
                &cv,
                &dv,
                crate::render::native_z::EMPTY_Z,
                wgpu::LoadOp::Load,
            );
            for layer in &layers {
                draw_native_object_pass(
                    &mut encoder,
                    &cv,
                    &dv,
                    &mut terrain,
                    [0, 0, size[0], size[1]],
                    &batch,
                    None,
                    layer,
                    None,
                    None,
                    None,
                    None,
                    None,
                    batch.default_zshape_bind_group(),
                    None,
                    Some(&lights),
                );
            }
            let reads = [
                gpu.read(&mut encoder, lights.composition_texture()),
                gpu.read(&mut encoder, &depth),
            ];
            let output = gpu.finish(encoder, &reads, size);
            assert_eq!(
                output[1],
                1f32.to_le_bytes().repeat((size[0] * size[1]) as usize),
                "real effect replay never writes Z"
            );
            frames.push(output[0].clone());
        }
        assert_ne!(
            frames[0], frames[1],
            "the actual System light reaches composition {format:?}"
        );
        assert_ne!(
            frames[0], frames[2],
            "the actual children reach the point GPU consumer {format:?}"
        );
        assert_eq!(
            gpu.read_float_texels(a.gpu_source().0, [0, 0], size),
            initial_a,
            "canonical A texture is unchanged"
        );
    }
    assert_eq!(
        sim.state_hash(),
        before_hash,
        "lower/upload/GPU replay is read-only"
    );
    assert_eq!(
        sim.rng_state(),
        before_rng,
        "full Scenario/Main/MapGen states are unchanged"
    );
}

#[test]
#[ignore = "requires GPU; original System/Particle overlap through retained lowering, live shared Z and light composition"]
fn retained_welding_system_then_particles_matches_original_shared_destination() {
    let rows = fixture::native()["mixed_draw_cases"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        2,
        "positive and reversed original component controls"
    );
    assert_ne!(
        fixture::expected_words(&rows[0]),
        fixture::expected_words(&rows[1]),
        "native overlap must expose body order"
    );
    let gpu = Gpu::new();
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut lights = CombatLightRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        for row in rows {
            let input = &row["input"];
            let system = &input["system"];
            let particles = &input["particles"];
            let size = fixture::array::<2>(&system["size"]).map(|value| value as u32);
            assert_eq!(system["size"], particles["size"]);
            assert_eq!(system["clip"], particles["clip"]);
            batch.write_camera(&gpu.queue, camera(size));
            let mut display = DisplayLayers::default();
            assert!(display.submit(1, Some(DisplayLayer::GROUND), &|_| 0));
            let points = particles["points"].as_array().unwrap();
            for index in 0..points.len() {
                assert!(display.submit(index as u64 + 2, Some(DisplayLayer::AIR), &|_| 0));
            }
            let order = NativeDisplayOrder::from_display(&display);
            let mut parents = Vec::new();
            let native_light = &row["light"];
            assert_eq!(native_light["admitted"], true);
            let at = fixture::array::<3>(&system["position"]);
            let mut detail = DetailState::new();
            if system["reduced"].as_bool().unwrap() {
                detail.minimum_frame_rate();
            }
            detail.configure_normal(&DetailRules {
                min_frame_rate_normal: fixture::unsigned(&system["minimum"]) as i32,
                buffer_zone_width: fixture::unsigned(&system["buffer"]) as i32,
                ..Default::default()
            });
            for _ in 0..fixture::unsigned(&system["fps"]) {
                detail.record_logic_visit();
            }
            detail.throttle_tail(0);
            // Whole System62E280's captured base size is the input to the one
            // app Spotlight owner. The base-size arithmetic is independently
            // compared by combat_lights' complete System-draw controls.
            let light = CombatLight {
                coord: ProjectileCoord::new(at[0], at[1], at[2]),
                stage: u8::try_from(fixture::int(&system["stage"])).unwrap(),
                base_size: fixture::int(&native_light["base_size"]),
                flags: 0,
            }
            .draw_record(&mut detail)
            .unwrap();
            assert_eq!(
                serde_json::json!(light.surface_index),
                native_light["surface_index"]
            );
            let mut parent = PlannedObjectInstance::object(
                order.object_draw(1, SpriteEncoding::Plain).unwrap(),
                Vec::new(),
            );
            parent.body_draw = Some(ObjectBodyDraw::CombatLight(light));
            parents.push(parent);
            let colors = fixture::colors(particles);
            for (index, (point, draw)) in points
                .iter()
                .zip(row["draws"].as_array().unwrap())
                .enumerate()
            {
                if let SparkPointPreparation::Ready { point, .. } = prepare_spark_point(
                    fixture::command(point, draw, index),
                    &colors,
                    fixture::viewport(particles),
                    SparkPointPlanes::Deferred,
                )
                .unwrap()
                {
                    let mut parent = PlannedObjectInstance::object(
                        order
                            .object_draw(index as u64 + 2, SpriteEncoding::Plain)
                            .unwrap(),
                        Vec::new(),
                    );
                    parent.body_draw = Some(ObjectBodyDraw::Surface(SurfaceDraw::Point(point)));
                    parents.push(parent);
                }
            }
            // Storage has no authority over Display or body execution order.
            parents.reverse();
            let mut layers = lower_object_instances(parents);
            assert_eq!(layers[2].light_records.len(), 1);
            assert_eq!(
                layers[3].surface_draws.len(),
                1,
                "adjacent points share one surface run"
            );
            let mut records = Vec::new();
            for layer in &mut layers {
                layer.light_record_base = records.len();
                records.extend_from_slice(&layer.light_records);
            }
            let mut light_camera = fixture::array::<2>(&system["camera"]);
            light_camera[1] += crate::util::lepton::absolute_leptons_to_screen(0, 0, 0).1 as i32;
            lights.prepare_with_device(
                &gpu.device,
                &gpu.queue,
                &records,
                size.map(|value| value as f32),
                light_camera.map(|value| value as f32),
                1.,
            );
            let cv = lights.composition_view();
            let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
            let dv = depth.create_view(&Default::default());
            terrain.prepare(
                &gpu.device,
                lights.composition_texture(),
                &dv,
                batch.camera_uniform(),
            );
            let a = fixture::plane(particles, "a");
            let z = fixture::plane(particles, "z");
            terrain.begin_surface_lines();
            for layer in &mut layers {
                layer.line_batches = layer
                    .surface_draws
                    .iter()
                    .map(|draws| {
                        terrain.prepare_surface_lines(
                            &gpu.device,
                            &gpu.queue,
                            std::iter::empty(),
                            draws.iter().copied(),
                            &[],
                            &[],
                            &[],
                            SurfaceLineViewport {
                                camera: fixture::array(&particles["camera"]),
                                clip: fixture::array(&particles["clip"]),
                                z_origin_y: fixture::int(&particles["z_origin_term"]),
                                zoom: 1.,
                            },
                            |_| false,
                            |[x, y]| a[(y as u32 * size[0] + x as u32) as usize],
                        )
                    })
                    .collect();
            }
            let initial = encoded(fixture::int(&system["background"]) as u16, format)
                .repeat((size[0] * size[1]) as usize);
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: lights.composition_texture(),
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &initial,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size[0] * 4),
                    rows_per_image: Some(size[1]),
                },
                lights.composition_texture().size(),
            );
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            seed_depth_grid(&gpu, &mut encoder, &dv, &z, size[0]);
            let replay = if input["order"] == serde_json::json!(["system", "particle"]) {
                [2, 3]
            } else {
                assert_eq!(input["order"], serde_json::json!(["particle", "system"]));
                [3, 2]
            };
            for index in replay {
                draw_native_object_pass(
                    &mut encoder,
                    &cv,
                    &dv,
                    &mut terrain,
                    fixture::array::<4>(&system["clip"]).map(|value| value as u32),
                    &batch,
                    None,
                    &layers[index],
                    None,
                    None,
                    None,
                    None,
                    None,
                    batch.default_zshape_bind_group(),
                    None,
                    Some(&lights),
                );
            }
            let reads = [
                gpu.read(&mut encoder, lights.composition_texture()),
                gpu.read(&mut encoder, &depth),
            ];
            let output = gpu.finish(encoder, &reads, size);
            for (index, (pixel, expected)) in output[0]
                .as_chunks::<4>()
                .0
                .iter()
                .zip(fixture::expected_words(row))
                .enumerate()
            {
                let rgb = if format == wgpu::TextureFormat::Bgra8UnormSrgb {
                    [pixel[2], pixel[1], pixel[0]]
                } else {
                    [pixel[0], pixel[1], pixel[2]]
                };
                assert_eq!(
                    crate::render::native_surface_format::RGB565.pack_rgb8(rgb),
                    expected,
                    "{} {format:?} pixel{index}",
                    input["name"]
                );
                assert_eq!(pixel[3], 255);
            }
            let expected_z: Vec<_> = z
                .iter()
                .flat_map(|&word| crate::render::native_z::stored_depth(word).to_le_bytes())
                .collect();
            assert_eq!(
                output[1], expected_z,
                "{} live Z is immutable",
                input["name"]
            );
        }
    }
}
