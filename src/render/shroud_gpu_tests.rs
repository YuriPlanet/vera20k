//! Actual world blitters consume the same A source as native surface admission.
//! Expected colors/Z come from original executed leaves, not Rust palette math.
use super::{
    batch::{BatchRenderer, DepthAxis, SpriteInstance},
    native_z,
    palette_light::PaletteLight,
    shroud_buffer::ShroudBuffer,
    tactical_draw_plan::RenderZPolicy,
    terrain_draw::{DestinationEditCommand, TerrainDrawRenderer, TerrainPiece},
    terrain_draw_gpu_tests::{Gpu, camera, clear, encoded},
};
use serde_json::Value;
use wgpu::util::DeviceExt;

fn fixture() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/blitter_a.json",
    ))
    .unwrap()
}
fn buffer(gpu: &Gpu, instances: &[SpriteInstance]) -> wgpu::Buffer {
    gpu.device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Native A composition inputs"),
            contents: bytemuck::cast_slice(instances),
            usage: wgpu::BufferUsages::VERTEX,
        })
}
fn light(row: &Value) -> PaletteLight {
    let brightness = row["brightness"].as_i64().unwrap() as i32;
    match row["profile"].as_str().unwrap() {
        "plain1" => PaletteLight::plain(1, brightness),
        "plain53" => PaletteLight::plain(53, brightness),
        "light27" => PaletteLight::new([1000; 3], 27, brightness, false),
        "scheme53" => PaletteLight::color_scheme([1000; 3], brightness),
        name => panic!("unrecognized native profile {name}"),
    }
}
fn load_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    color: &'a wgpu::TextureView,
    depth: &'a wgpu::TextureView,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Raw surface then native A blit"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: color,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    })
}

#[test]
#[ignore = "requires GPU; native A body/shadow colors, raw stores, source holes and depth rejects"]
fn production_a_blitters_preserve_raw_surface_pixels_through_holes_and_depth_rejects() {
    let native = fixture();
    let cases = native["cases"].as_array().unwrap();
    let gpu = Gpu::new();
    let size = [3, cases.len() as u32];
    let rgb: [u8; 3] = std::array::from_fn(|i| native["rgb"][i].as_u64().unwrap() as u8);
    let candidate = native["candidate"].as_i64().unwrap() as i32;
    let raw = native["raw"].as_u64().unwrap() as u16;
    let a = cases
        .iter()
        .flat_map(|row| [row["a"].as_u64().unwrap() as u8; 3])
        .collect::<Vec<_>>();
    let indices = cases
        .iter()
        .flat_map(|row| {
            let index = row["index"].as_u64().unwrap() as u8;
            [index, 0, index]
        })
        .collect::<Vec<_>>();
    let rgba = indices
        .iter()
        .flat_map(|&index| [rgb[0], rgb[1], rgb[2], if index == 0 { 0 } else { 255 }])
        .collect::<Vec<_>>();
    let bodies = cases
        .iter()
        .enumerate()
        .map(|(y, row)| SpriteInstance {
            position: [0.0, y as f32],
            size: [3.0, 1.0],
            uv_origin: [0.0, y as f32 / size[1] as f32],
            uv_size: [1.0, 1.0 / size[1] as f32],
            tint: [1.0; 3],
            alpha: 1.0,
            z_adjust: (candidate - (32768 - y as i32)) as f32,
            z_gradient: native_z::Z_GRADIENT_ZSHAPE_FLAG,
            palette_light: light(row),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    let seeds = (0..size[1])
        .map(|y| SpriteInstance {
            position: [2.0, y as f32],
            size: [1.0; 2],
            uv_size: [1.0; 2],
            tint: [1.0; 3],
            alpha: 1.0,
            z_adjust: -(32768 - y as i32) as f32,
            z_gradient: native_z::Z_GRADIENT_ZSHAPE_FLAG,
            palette_light: PaletteLight::plain(1, 1000),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    let bodies_buffer = buffer(&gpu, &bodies);
    let seeds_buffer = buffer(&gpu, &seeds);
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        batch.write_camera(&gpu.queue, camera(size));
        let source = ShroudBuffer::fixture(&gpu.device, &gpu.queue, size, [0.0; 2], &a);
        batch.bind_shroud(&gpu.device, Some(&source));
        // Reusing an unchanged source must retain the derived binding.
        let bound = batch.camera_bind_group().clone();
        batch.bind_shroud(&gpu.device, Some(&source));
        assert_eq!(&bound, batch.camera_bind_group());
        let white =
            batch.create_texture_on_device(&gpu.device, &gpu.queue, &[255; 4], 1, 1, Some(&[1]));
        let atlas = batch.create_texture_on_device(
            &gpu.device,
            &gpu.queue,
            &rgba,
            size[0],
            size[1],
            Some(&indices),
        );
        let shape = gpu.device.create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("Native zero signed BUILDNGZ source"),
                size: super::terrain_draw_gpu_tests::extent(size),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &vec![128; (size[0] * size[1]) as usize],
        );
        let zshape = batch.create_zshape_bind_group_on_device(
            &gpu.device,
            &shape.create_view(&Default::default()),
        );
        let raw_rgb = encoded(raw, wgpu::TextureFormat::Rgba8UnormSrgb);
        let raw_buffer = buffer(
            &gpu,
            &[SpriteInstance {
                size: size.map(|v| v as f32),
                uv_size: [1.0; 2],
                tint: std::array::from_fn(|i| f32::from(raw_rgb[i]) / 255.0),
                alpha: 1.0,
                ..Default::default()
            }],
        );
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        for route in ["building", "terrain", "shadow"] {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            clear(
                &mut encoder,
                &cv,
                &dv,
                65535,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            );
            {
                let mut pass = load_pass(&mut encoder, &cv, &dv);
                batch.draw_zsprite_range(
                    &mut pass,
                    &white,
                    &zshape,
                    &seeds_buffer,
                    0,
                    size[1],
                    true,
                );
                // Actual production raw surface path: A is intentionally not a
                // brightness multiplier. The partial-A rows model rally pass0.
                batch.draw_with_buffer_passthrough(&mut pass, &white, &raw_buffer, 1);
                if route == "building" {
                    batch.draw_zsprite_range(
                        &mut pass,
                        &atlas,
                        &zshape,
                        &bodies_buffer,
                        0,
                        size[1],
                        true,
                    );
                }
            }
            if route != "building" {
                terrain.draw_span(
                    &mut encoder,
                    &cv,
                    &dv,
                    &batch,
                    |_| Some(&atlas),
                    &bodies_buffer,
                    &bodies,
                    (0..size[1]).map(|index| DestinationEditCommand {
                        index,
                        piece: if route == "shadow" {
                            TerrainPiece::Shadow
                        } else {
                            TerrainPiece::Body
                        },
                        render_z: RenderZPolicy::ReadWrite,
                        atlas_slot: 0,
                    }),
                    [0, 0, size[0], size[1]],
                );
            }
            let reads = [
                gpu.read(&mut encoder, &color),
                gpu.read(&mut encoder, &depth),
            ];
            let output = gpu.finish(encoder, &reads, size);
            for (y, row) in cases.iter().enumerate() {
                let expected = &row[if route == "shadow" { "shadow" } else { "body" }];
                for x in 0..3 {
                    let offset = (y * 3 + x) * 4;
                    assert_eq!(
                        &output[0][offset..offset + 4],
                        &encoded(expected["colors"][x].as_u64().unwrap() as u16, format),
                        "{route} {format:?} case{y} pixel{x} {row}"
                    );
                    let z = f32::from_le_bytes(output[1][offset..offset + 4].try_into().unwrap());
                    assert_eq!(
                        native_z::stored_z(z),
                        expected["depths"][x].as_u64().unwrap() as u16,
                        "{route} case{y} pixel{x} depth"
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "requires GPU; shared A origin, fractional camera, zoom, cache replacement and sandbox"]
fn production_world_a_sampling_matches_cpu_source_at_fractional_scroll_and_zoom() {
    let gpu = Gpu::new();
    let native = fixture();
    let native_rows = native["cases"].as_array().unwrap();
    let mut expected_colors = [0u16; 256];
    let mut alphas = Vec::new();
    for row in native_rows.iter().filter(|row| {
        row["profile"] == "scheme53" && row["index"] == 1 && row["brightness"] == 1000
    }) {
        let a = row["a"].as_u64().unwrap() as usize;
        expected_colors[a] = row["body"]["colors"][0].as_u64().unwrap() as u16;
        alphas.push(a as u8);
    }
    let size = [19, 11];
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    let texture = batch.create_texture_on_device(
        &gpu.device,
        &gpu.queue,
        &[200, 100, 50, 255],
        1,
        1,
        Some(&[1]),
    );
    let zero = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("Native zero Z source"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &[0],
    );
    let tmp = batch.create_zdepth_bind_group_on_device(
        &gpu.device,
        &texture.view,
        &zero.create_view(&Default::default()),
    );
    let voxel = batch.create_unit_atlas_texture_on_device(&gpu.device, &gpu.queue, 1, 1, &[1]);
    let palette = super::palette_textures::PaletteSet::new_on_device(
        &gpu.device,
        &gpu.queue,
        &crate::assets::pal_file::Palette {
            colors: [crate::assets::pal_file::Color::rgb(200, 100, 50); 256],
        },
        &crate::rules::house_colors::HouseColorRamps::from_schemes(&[]),
        &[],
    );
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    for zoom in [0.75, 1.0, 1.5, 2.0] {
        for scroll in [[13.25_f32, -7.25_f32], [13.75, -7.75]] {
            batch.update_camera_on_queue(
                &gpu.queue,
                size[0] as f32,
                size[1] as f32,
                scroll[0],
                scroll[1],
                zoom,
                DepthAxis::NONE,
            );
            let cam = batch.camera_uniform();
            let virtual_size = size.map(|v| (v as f32 / zoom).ceil() as u32);
            let a = (0..virtual_size[0] * virtual_size[1])
                .map(|i| {
                    alphas[((i % virtual_size[0] + 3 * (i / virtual_size[0])) as usize)
                        % alphas.len()]
                })
                .collect::<Vec<_>>();
            let source = ShroudBuffer::fixture(&gpu.device, &gpu.queue, virtual_size, scroll, &a);
            // Every iteration replaces the source view, even at identical size.
            batch.bind_shroud(&gpu.device, Some(&source));
            let instance = SpriteInstance {
                position: cam.camera_pos,
                size: virtual_size.map(|v| v as f32),
                uv_size: [1.0; 2],
                tint: [1.0; 3],
                alpha: 1.0,
                palette_light: PaletteLight::color_scheme([1000; 3], 1000),
                ..Default::default()
            };
            let data = buffer(&gpu, &[instance]);
            for disabled in [false, true] {
                batch.bind_shroud(&gpu.device, if disabled { None } else { Some(&source) });
                for route in ["batch", "tmp", "voxel"] {
                    let mut encoder = gpu.device.create_command_encoder(&Default::default());
                    clear(
                        &mut encoder,
                        &cv,
                        &dv,
                        65535,
                        wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    );
                    {
                        let mut pass = load_pass(&mut encoder, &cv, &dv);
                        match route {
                            "batch" => {
                                batch.draw_passthrough_range(&mut pass, &texture, &data, 0, 1)
                            }
                            "tmp" => batch.draw_with_buffer_zdepth(&mut pass, &tmp, &data, 1),
                            "voxel" => batch.draw_voxel_sprites_range(
                                &mut pass,
                                &voxel,
                                &palette.bind_group,
                                &data,
                                0,
                                1,
                            ),
                            _ => unreachable!(),
                        }
                    }
                    let reads = [gpu.read(&mut encoder, &color)];
                    let output = gpu.finish(encoder, &reads, size);
                    for y in 0..size[1] {
                        for x in 0..size[0] {
                            // Read the authoritative CPU lattice at the world
                            // point represented by this fragment center.
                            let a = if disabled {
                                127
                            } else {
                                source
                                    .sample_world(
                                        (x as f32 + 0.5) / zoom + cam.camera_pos[0],
                                        (y as f32 + 0.5) / zoom + cam.camera_pos[1],
                                        scroll[0],
                                        scroll[1],
                                    )
                                    .unwrap_or(127)
                            };
                            let offset = ((y * size[0] + x) * 4) as usize;
                            assert_eq!(
                                &output[0][offset..offset + 4],
                                &encoded(expected_colors[a as usize], format),
                                "{route} zoom{zoom} scroll{scroll:?} disabled{disabled} pixel{x},{y} A{a}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "requires GPU timestamps; run optimized for live A source cost at production batch sizes"]
fn production_a_blitter_workload_timing() {
    let gpu = Gpu::with_features(wgpu::Features::TIMESTAMP_QUERY);
    let size = [1280, 720];
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera(size));
    let texture = batch.create_texture_on_device(
        &gpu.device,
        &gpu.queue,
        &[200, 100, 50, 255],
        1,
        1,
        Some(&[1]),
    );
    let pixels = (0..size[0] * size[1])
        .map(|i| [0, 63, 127][(i as usize / 16) % 3])
        .collect::<Vec<_>>();
    let source = ShroudBuffer::fixture(&gpu.device, &gpu.queue, size, [0.0; 2], &pixels);
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    let queries = gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
        label: Some("Shared A world blitter pass timestamps"),
        ty: wgpu::QueryType::Timestamp,
        count: 2,
    });
    let resolved = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    for count in [1_000u32, 20_000] {
        let instances = (0..count)
            .map(|i| SpriteInstance {
                position: [(i % 80 * 16) as f32, ((i / 80) % 45 * 16) as f32],
                size: [16.0; 2],
                uv_size: [1.0; 2],
                tint: [1.0; 3],
                alpha: 1.0,
                palette_light: PaletteLight::color_scheme([1000; 3], 1000),
                ..Default::default()
            })
            .collect::<Vec<_>>();
        let instances = buffer(&gpu, &instances);
        for live in [false, true] {
            batch.bind_shroud(&gpu.device, if live { Some(&source) } else { None });
            let mut cpu_ms = Vec::new();
            let mut gpu_ms = Vec::new();
            let mut completed_ms = Vec::new();
            for frame in 0..22 {
                let start = std::time::Instant::now();
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                {
                    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Actual shared-A world source batch"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &cv,
                            resolve_target: None,
                            depth_slice: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &dv,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: Some(wgpu::RenderPassTimestampWrites {
                            query_set: &queries,
                            beginning_of_pass_write_index: Some(0),
                            end_of_pass_write_index: Some(1),
                        }),
                        occlusion_query_set: None,
                    });
                    batch.draw_passthrough_range(&mut pass, &texture, &instances, 0, count);
                }
                encoder.resolve_query_set(&queries, 0..2, &resolved, 0);
                encoder.copy_buffer_to_buffer(&resolved, 0, &read, 0, 16);
                let command = encoder.finish();
                let encode_ms = start.elapsed().as_secs_f64() * 1000.0;
                let submitted = std::time::Instant::now();
                let submission = gpu.queue.submit([command]);
                let (tx, rx) = std::sync::mpsc::channel();
                read.slice(..)
                    .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
                gpu.device
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(submission),
                        timeout: Some(std::time::Duration::from_secs(60)),
                    })
                    .unwrap();
                rx.recv().unwrap().unwrap();
                let bytes = read.slice(..).get_mapped_range();
                let ticks = bytemuck::cast_slice::<u8, u64>(&bytes);
                // Some Metal drivers expose timestamp queries but return an
                // unsupported sentinel. Never report such values as zero cost;
                // retain submit-to-completion wall timing independently.
                let valid = ticks[0] != 0 && ticks[1] != u64::MAX && ticks[1] > ticks[0];
                if frame >= 2 {
                    cpu_ms.push(encode_ms);
                    completed_ms.push(submitted.elapsed().as_secs_f64() * 1000.0);
                    if valid {
                        gpu_ms.push(
                            (ticks[1] - ticks[0]) as f64
                                * f64::from(gpu.queue.get_timestamp_period())
                                / 1_000_000.0,
                        );
                    }
                }
                drop(bytes);
                read.unmap();
            }
            let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
            let gpu_time = if gpu_ms.len() == 20 {
                format!("{:.4}", mean(&gpu_ms))
            } else {
                "unavailable".into()
            };
            eprintln!(
                "A_BLITTER_TIMING count={count} live_a={live} frames=20 encode_mean_ms={:.4} completed_mean_ms={:.4} gpu_mean_ms={gpu_time} valid_gpu_samples={}",
                mean(&cpu_ms),
                mean(&completed_ms),
                gpu_ms.len()
            );
        }
    }
}
