//! Original complete LineTrail→4BEAC0 pixels through the live production owner.
use super::*;
use crate::render::batch::BatchRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded};
use crate::sim::projectile::ProjectileCoord;
use serde_json::Value;

#[test]
#[ignore = "requires GPU; original LineTrail full ordered pixels and unchanged shared depth"]
fn production_line_trail_matches_original_full_line_pixels() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/line_trail.json",
    ))
    .unwrap();
    let gpu = Gpu::new();
    let size = [64, 96];
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        batch.write_camera(&gpu.queue, camera(size));
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        for row in native["pixels"].as_array().unwrap() {
            let input = &row["input"];
            let background = input["background"].as_u64().unwrap() as u16;
            let old_z = input["old_z"].as_u64().unwrap() as u16;
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
                color.size(),
            );
            let final_visit = 2 + input["idle_visits"].as_u64().unwrap();
            let origin: [i32; 3] =
                std::array::from_fn(|i| input["origin"][i].as_i64().unwrap() as i32);
            let delta: [i32; 3] =
                std::array::from_fn(|i| input["delta"][i].as_i64().unwrap() as i32);
            let segments = row["draw_calls"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|c| c["visit"].as_u64() == Some(final_visit))
                .map(|call| LineTrailSegment {
                    from: ProjectileCoord::new(
                        origin[0] + delta[0],
                        origin[1] + delta[1],
                        origin[2] + delta[2],
                    ),
                    to: ProjectileCoord::new(origin[0], origin[1], origin[2]),
                    color: std::array::from_fn(|i| call["rgb"][i].as_u64().unwrap() as u8),
                    strength: call["intensity"].as_i64().unwrap() as i32,
                })
                .collect::<Vec<_>>();
            let viewport = SurfaceLineViewport {
                // Only the global coordinate convention differs: +15 on all
                // VERA world layers is absorbed by the same camera translation.
                camera: [
                    row["camera"][0].as_i64().unwrap() as i32,
                    row["camera"][1].as_i64().unwrap() as i32 + 15,
                ],
                clip: std::array::from_fn(|i| input["clip"][i].as_i64().unwrap() as i32),
                z_origin_y: input["z_origin_y"].as_i64().unwrap() as i32,
                zoom: 1.,
            };
            terrain.begin_surface_lines();
            let prepared_lines = terrain.prepare_surface_lines(
                &gpu.device,
                &gpu.queue,
                std::iter::empty(),
                std::iter::empty(),
                &segments,
                &[],
                &[],
                viewport,
                |_| true,
                |[x, y]| {
                    if input["alpha"] == "mixed" {
                        [0, 1, 63, 127, 255][((x / 9 + y / 7) % 5) as usize]
                    } else {
                        input["alpha"].as_u64().unwrap() as u16
                    }
                },
            );
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            clear(&mut encoder, &cv, &dv, old_z, wgpu::LoadOp::Load);
            terrain.draw_surface_lines(&mut encoder, &cv, prepared_lines);
            let reads = [
                gpu.read(&mut encoder, &color),
                gpu.read(&mut encoder, &depth),
            ];
            let output = gpu.finish(encoder, &reads, size);
            let mut expected = bytes;
            for p in row["changed_pixels"].as_array().unwrap() {
                let offset = (p[1].as_u64().unwrap() as usize * size[0] as usize
                    + p[0].as_u64().unwrap() as usize)
                    * 4;
                expected[offset..offset + 4]
                    .copy_from_slice(&encoded(p[2].as_u64().unwrap() as u16, format));
            }
            assert_eq!(output[0], expected, "{format:?} {}", row["name"]);
            assert_eq!(
                output[1],
                crate::render::native_z::stored_depth(old_z)
                    .to_le_bytes()
                    .repeat((size[0] * size[1]) as usize),
                "depth unchanged {format:?} {}",
                row["name"]
            );
        }
    }
}

fn overlapping_segments(row: &Value) -> Vec<LineTrailSegment> {
    let input = &row["input"];
    let origin: [i32; 3] = std::array::from_fn(|i| input["origin"][i].as_i64().unwrap() as i32);
    let delta: [i32; 3] = std::array::from_fn(|i| input["delta"][i].as_i64().unwrap() as i32);
    row["draw_calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|call| LineTrailSegment {
            from: ProjectileCoord::new(
                origin[0] + delta[0],
                origin[1] + delta[1],
                origin[2] + delta[2],
            ),
            to: ProjectileCoord::new(origin[0], origin[1], origin[2]),
            color: std::array::from_fn(|i| call["rgb"][i].as_u64().unwrap() as u8),
            strength: call["intensity"].as_i64().unwrap() as i32,
        })
        .collect()
}

fn ordered_viewport(row: &Value) -> SurfaceLineViewport {
    SurfaceLineViewport {
        camera: [
            row["camera"][0].as_i64().unwrap() as i32,
            row["camera"][1].as_i64().unwrap() as i32 + 15,
        ],
        clip: [0, 0, 64, 96],
        z_origin_y: 0,
        zoom: 1.,
    }
}

#[test]
#[ignore = "requires GPU; original reverse-registry trail overlap and bounded operation chunks"]
fn production_line_trail_preserves_native_overlap_across_chunks() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/line_trail.json",
    ))
    .unwrap();
    let gpu = Gpu::new();
    let size = [64, 96];
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        batch.write_camera(&gpu.queue, camera(size));
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        for row in native["ordered_pixel_cases"].as_array().unwrap() {
            for forced_chunk in [None, Some(3)] {
                if forced_chunk.is_some() && (row["count"] != 8 || row["input"]["alpha"] != 127) {
                    continue;
                }
                terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
                terrain.surface_lines.test_operation_limit = forced_chunk;
                let segments = overlapping_segments(row);
                terrain.begin_surface_lines();
                let prepared_lines = terrain.prepare_surface_lines(
                    &gpu.device,
                    &gpu.queue,
                    std::iter::empty(),
                    std::iter::empty(),
                    &segments,
                    &[],
                    &[],
                    ordered_viewport(row),
                    |_| true,
                    |_| row["input"]["alpha"].as_u64().unwrap() as u16,
                );
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                clear(
                    &mut encoder,
                    &cv,
                    &dv,
                    65535,
                    wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                );
                terrain.draw_surface_lines(&mut encoder, &cv, prepared_lines);
                let reads = [
                    gpu.read(&mut encoder, &color),
                    gpu.read(&mut encoder, &depth),
                ];
                let output = gpu.finish(encoder, &reads, size);
                let mut expected = encoded(65535, format).repeat((size[0] * size[1]) as usize);
                for p in row["changed_pixels"].as_array().unwrap() {
                    let offset = (p[1].as_u64().unwrap() as usize * 64
                        + p[0].as_u64().unwrap() as usize)
                        * 4;
                    expected[offset..offset + 4]
                        .copy_from_slice(&encoded(p[2].as_u64().unwrap() as u16, format));
                }
                assert_eq!(
                    output[0], expected,
                    "{format:?} trails={} chunk={forced_chunk:?}",
                    row["count"]
                );
                assert_eq!(
                    output[1],
                    1f32.to_le_bytes().repeat((size[0] * size[1]) as usize)
                );
                if forced_chunk.is_some() {
                    assert!(
                        terrain.submissions() > 0,
                        "forced chunks exercise bounded submit as well as operation grouping"
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "requires quiet GPU; run in release; bounded preparation/completed submission timing, not FPS"]
fn line_trail_native_fixture_submission_timings() {
    use crate::render::tactical_compat::{
        SparkPointCommand, SparkPointPlanes, SparkPointPreparation, TacticalPointViewport,
        TacticalRect, prepare_spark_point, welding_tests as fixture,
    };
    use std::time::Instant;
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/line_trail.json",
    ))
    .unwrap();
    let gpu = Gpu::new();
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let measure = |name: &str,
                       size: [u32; 2],
                       draws: &[SurfaceDraw],
                       spark: Option<(&[SparkPointCommand], &[[u8; 3]], TacticalPointViewport)>,
                       point_layout: Option<&str>,
                       light_records: &[crate::render::combat_light::CombatLightDrawRecord],
                       segments: &[LineTrailSegment],
                       radials: &[crate::render::radial_indicator::RadialIndicatorDraw],
                       viewport: SurfaceLineViewport,
                       alpha: u16| {
            let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
            let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
            let mut lights = crate::render::combat_light::CombatLightRenderer::new_with_device(
                &gpu.device,
                &gpu.queue,
                format,
            );
            batch.write_camera(&gpu.queue, camera(size));
            let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
            let dv = depth.create_view(&Default::default());
            let mut prepared_points = Vec::with_capacity(spark.map_or(0, |value| value.0.len()));
            let mut surface_draws = draws.len();
            let mut samples = Vec::new();
            let mut chunks = 0;
            let mut submissions = 0;
            for repetition in 0..22 {
                let begin = Instant::now();
                // Time the production numeric owner separately from transport.
                // Commands, color lists and authored cluster positions are
                // inputs; Display lookup, Detail queries, lowering and App
                // builder/allocation cost remain outside this component test.
                let point_prepared = if let Some((commands, colors, viewport)) = spark {
                    prepared_points.clear();
                    for &command in commands {
                        match prepare_spark_point(
                            command,
                            colors,
                            viewport,
                            SparkPointPlanes::Deferred,
                        )
                        .unwrap()
                        {
                            SparkPointPreparation::Ready { point, .. } => {
                                prepared_points.push(SurfaceDraw::Point(point));
                            }
                            SparkPointPreparation::Rejected(reason) => {
                                panic!("authored Welding timing input rejected: {reason:?}");
                            }
                        }
                    }
                    begin.elapsed().as_secs_f64() * 1000.
                } else {
                    0.
                };
                let draws = if spark.is_some() {
                    prepared_points.as_slice()
                } else {
                    draws
                };
                surface_draws = draws.len();
                let prepare_begin = Instant::now();
                let mut light_camera = viewport.camera.map(|value| value as f32);
                light_camera[1] += crate::util::lepton::absolute_leptons_to_screen(0, 0, 0).1;
                lights.prepare_with_device(
                    &gpu.device,
                    &gpu.queue,
                    light_records,
                    size.map(|value| value as f32),
                    light_camera,
                    1.,
                );
                let cv = lights.composition_view();
                terrain.prepare(
                    &gpu.device,
                    lights.composition_texture(),
                    &dv,
                    batch.camera_uniform(),
                );
                terrain.begin_surface_lines();
                let prepared_lines = terrain.prepare_surface_lines(
                    &gpu.device,
                    &gpu.queue,
                    std::iter::empty(),
                    draws.iter().copied(),
                    segments,
                    &[],
                    radials,
                    viewport,
                    |_| true,
                    |_| alpha,
                );
                let prepared = prepare_begin.elapsed().as_secs_f64() * 1000.;
                chunks = prepared_lines.end - prepared_lines.start;
                let encode_begin = Instant::now();
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                clear(
                    &mut encoder,
                    &cv,
                    &dv,
                    65535,
                    wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                );
                // Native Systems occupy GROUND2; their children occupy AIR3.
                // One immutable light upload and the same bounded submission
                // accounting are used by production retained-object replay.
                for index in 0..light_records.len() {
                    if lights.draw_record(&mut encoder, [0, 0, size[0], size[1]], index) {
                        terrain.note_external_passes(&mut encoder, 1);
                    }
                }
                terrain.draw_surface_lines(&mut encoder, &cv, prepared_lines);
                submissions = terrain.submissions();
                if light_records.len() >= 256 {
                    assert!(
                        submissions > 0,
                        "light bodies share the bounded submission owner"
                    );
                }
                // This interval includes the shared owner's bounded submits;
                // completed wall time also includes final submit and GPU wait.
                let encoded = encode_begin.elapsed().as_secs_f64() * 1000.;
                gpu.queue.submit([encoder.finish()]);
                gpu.device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .unwrap();
                if repetition >= 2 {
                    samples.push((
                        point_prepared,
                        prepared,
                        encoded,
                        begin.elapsed().as_secs_f64() * 1000.,
                    ));
                }
            }
            let median = |field: usize| {
                let mut values = samples
                    .iter()
                    .map(|v| match field {
                        0 => v.0,
                        1 => v.1,
                        2 => v.2,
                        _ => v.3,
                    })
                    .collect::<Vec<_>>();
                values.sort_by(f64::total_cmp);
                values[values.len() / 2]
            };
            eprintln!(
                "SURFACE_LINE_TIMING {}",
                serde_json::json!({"name":name,"format":format!("{format:?}"),"surface_draws":surface_draws,"point_layout":point_layout,"points_per_system":surface_draws.checked_div(light_records.len()),"lights":light_records.len(),"trails":segments.len(),"radials":radials.len(),"alpha":alpha,"size":size,"samples":samples.len(),"chunks":chunks,"submissions":submissions,"point_prepare_ms_median":median(0),"prepare_ms_median":median(1),"encode_ms_median":median(2),"complete_ms_median":median(3),"complete_ms_min":samples.iter().map(|v|v.3).fold(f64::INFINITY,f64::min),"complete_ms_max":samples.iter().map(|v|v.3).fold(0.,f64::max)})
            );
        };
        for row in native["ordered_pixel_cases"].as_array().unwrap() {
            measure(
                "native_line_trail",
                [64, 96],
                &[],
                None,
                None,
                &[],
                &overlapping_segments(row),
                &[],
                ordered_viewport(row),
                row["input"]["alpha"].as_u64().unwrap() as u16,
            );
        }
        // Stock selected GAGAP, native range10 and house RGB. Full visible
        // ellipses plus overlapping four-ray fans exercise dependency chunks;
        // these are preparation/submission samples, not native frame timing.
        let size = [1280, 720];
        for frame in [0, 300] {
            for count in [1, 10] {
                let radials: Vec<_> = (0..count)
                    .map(|i| crate::render::radial_indicator::RadialIndicatorDraw {
                        center: [640 + (i % 5 - 2) * 20, 360 + (i / 5) * 20],
                        radius: 10.0,
                        truncate_radius: true,
                        pixel_radius: false,
                        rgb: [255, 0, 0],
                        fade: true,
                        rays: true,
                        frame,
                    })
                    .collect();
                measure(
                    &format!("selected_gagap_frame{frame}"),
                    size,
                    &[],
                    None,
                    None,
                    &[],
                    &[],
                    &radials,
                    SurfaceLineViewport {
                        camera: [0; 2],
                        clip: [0, 0, size[0] as i32, size[1] as i32],
                        z_origin_y: 0,
                        zoom: 1.,
                    },
                    0x100,
                );
            }
        }
        // Repeated original admitted Welding components bound the existing
        // numeric preparation, upload, color-copy and submission costs. Keep
        // the cap-valued dense baseline; Spark births can exceed that cap.
        // These authored component workloads are completed-work timing, not
        // a native frame/FPS or 20k-unit claim. Each light still copies the
        // entire width*height*4 color surface; derive traffic from its count.
        let welding = crate::render::tactical_compat::welding_tests::native();
        let system = welding["system_draw_cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| {
                row["light"]["admitted"] == true
                    && row["input"]["particle_count"] == row["input"]["particle_cap"]
                    && row["input"]["particle_count"]
                        .as_u64()
                        .is_some_and(|count| count > 0)
            })
            .expect("original full-cap admitted Welding control");
        let points = welding["spark_draw_cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| {
                row["input"]["a_default"] == 127
                    && row["draws"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|draw| !draw["write"].is_null())
            })
            .expect("original canonical-A admitted Welding point control");
        let source = crate::render::tactical_compat::welding_tests::prepared_points(
            &points["input"],
            &points["draws"],
        );
        assert!(!source.is_empty());
        let source_commands: Vec<_> = points["input"]["points"]
            .as_array()
            .unwrap()
            .iter()
            .zip(points["draws"].as_array().unwrap())
            .enumerate()
            .filter_map(|(ordinal, (point, draw))| {
                draw["write"]
                    .is_object()
                    .then(|| fixture::command(point, draw, ordinal))
            })
            .collect();
        assert!(!source_commands.is_empty());
        let colors = fixture::colors(&points["input"]);
        let point_viewport = TacticalPointViewport {
            // The performance board is deliberately larger than the original
            // pixel-control crop; native projection/clip arithmetic is reused.
            clip: TacticalRect {
                left: 0,
                top: 0,
                width: size[0] as i32,
                height: size[1] as i32,
            },
            ..fixture::viewport(&points["input"])
        };
        // Largest count among retained original live boundaries, not a proven
        // maximum or a hard cap for ordinary gameplay.
        let observed_live_count = usize::try_from(
            welding["live_case"]["boundaries"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|boundary| boundary["system"]["particle_count"].as_u64())
                .max()
                .expect("an original live Welding particle count"),
        )
        .unwrap();
        assert!(observed_live_count > 0);
        let at =
            crate::render::tactical_compat::welding_tests::array::<3>(&system["input"]["position"]);
        let record = crate::render::combat_light::CombatLightDrawRecord {
            coord: ProjectileCoord::new(at[0], at[1], at[2]),
            surface_index: crate::render::tactical_compat::welding_tests::int(
                &system["light"]["surface_index"],
            ),
            flags: 0,
        };
        let per_system = system["input"]["particle_count"].as_u64().unwrap() as usize;
        let viewport = SurfaceLineViewport {
            camera: fixture::array(&system["input"]["camera"]),
            clip: [0, 0, size[0] as i32, size[1] as i32],
            z_origin_y: 0,
            zoom: 1.,
        };
        for count in [1, 10, 100, 256] {
            let draws: Vec<_> = source
                .iter()
                .copied()
                .cycle()
                .take(count * per_system)
                .map(SurfaceDraw::Point)
                .collect();
            let light_records = vec![record; count];
            measure(
                "native_welding_overlap_components",
                size,
                &draws,
                None,
                Some("dense"),
                &light_records,
                &[],
                &[],
                viewport,
                127,
            );
            for layout in ["dense", "separated"] {
                let mut commands = Vec::with_capacity(count * observed_live_count);
                let mut clustered_lights = Vec::with_capacity(count);
                for cluster in 0..count {
                    // World-space authored positions remain outside timing.
                    // The shared native owner projects every measured point;
                    // no benchmark screen-coordinate projection is substituted.
                    let offset = if layout == "dense" {
                        glam::IVec3::ZERO
                    } else {
                        let column = (cluster % 16) as i32;
                        let row = (cluster / 16) as i32;
                        glam::IVec3::new((column + row) * 256, (row - column) * 256, 0)
                    };
                    clustered_lights.push(crate::render::combat_light::CombatLightDrawRecord {
                        coord: ProjectileCoord::new(
                            record.coord.x.wrapping_add(offset.x),
                            record.coord.y.wrapping_add(offset.y),
                            record.coord.z.wrapping_add(offset.z),
                        ),
                        ..record
                    });
                    for child in 0..observed_live_count {
                        let mut command = source_commands[child % source_commands.len()];
                        command.world = glam::IVec3::new(
                            command.world.x.wrapping_add(offset.x),
                            command.world.y.wrapping_add(offset.y),
                            command.world.z.wrapping_add(offset.z),
                        );
                        command.draw_ordinal = commands.len() as u64;
                        commands.push(command);
                    }
                }
                measure(
                    "native_welding_observed_live_components",
                    size,
                    &[],
                    Some((commands.as_slice(), colors.as_slice(), point_viewport)),
                    Some(layout),
                    &clustered_lights,
                    &[],
                    &[],
                    viewport,
                    127,
                );
            }
        }
    }
}
