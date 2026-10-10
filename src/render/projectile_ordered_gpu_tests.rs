//! Reduction checks against ordered original-leaf execution. Fixture uploads
//! are explicit destination/palette boundaries, not retail-reader evidence.
use super::*;
use crate::render::batch::SpriteInstance;
use crate::render::terrain_draw_gpu_tests::seed_depth;

fn upload_color(gpu: &Gpu, color: &wgpu::Texture, bytes: &[u8]) {
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: color,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(color.width() * 4),
            rows_per_image: Some(color.height()),
        },
        color.size(),
    );
}

fn force_scratch_rows(gpu: &Gpu, terrain: &mut TerrainDrawRenderer, width: u32, rows: u32) {
    // Exercise the production capacity-band path without allocating an 8K
    // target. This replaces only derived scratch with a smaller valid binding.
    let scratch = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Bounded storage band fixture"),
        size: u64::from(width * rows) * 8,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Bounded storage band fixture"),
        layout: &terrain.read_only_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: scratch.as_entire_binding(),
        }],
    });
    let targets = terrain.targets.as_mut().unwrap();
    targets.read_only = group;
    targets.read_only_rows = rows;
}

#[test]
#[ignore = "requires GPU; ordered native Bullet leaf sequences, reused indices and bounded storage bands"]
fn ordered_reduction_matches_original_sequence_prefixes() {
    let gpu = Gpu::new();
    let native = native();
    let cases = native["sequence_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 13);
    assert_eq!(
        cases
            .iter()
            .map(|case| case["steps"].as_array().unwrap().len())
            .sum::<usize>(),
        157
    );
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        // Reusing the owner across different attachment heights also exercises
        // target/bind-group replacement, then one-row scratch reuse at height3.
        for height in [1, 3] {
            let size = [4, height];
            batch.write_camera(&gpu.queue, camera(size));
            let color = gpu.target(size, format);
            let cv = color.create_view(&Default::default());
            let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
            let dv = depth.create_view(&Default::default());
            terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
            if height == 3 {
                force_scratch_rows(&gpu, &mut terrain, 4, 1);
            }
            for case in cases {
                let steps = case["steps"].as_array().unwrap();
                let old_words = case["initial_colors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_u64().unwrap() as u16)
                    .collect::<Vec<_>>();
                let old_z = case["initial_depths"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_u64().unwrap() as u16)
                    .collect::<Vec<_>>();
                let atlases = steps
                    .iter()
                    .map(|step| {
                        let indices = step["decoded_indices"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|x| x.as_u64().unwrap() as u8)
                            .collect::<Vec<_>>();
                        let bytes = indices
                            .iter()
                            .flat_map(|&index| {
                                let color = step["palette_colors"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .find(|entry| entry["index"].as_u64() == Some(u64::from(index)))
                                    .unwrap()["color"]
                                    .as_u64()
                                    .unwrap() as u16;
                                encoded(color, wgpu::TextureFormat::Rgba8UnormSrgb)
                            })
                            .collect::<Vec<_>>();
                        batch.create_texture_on_device(
                            &gpu.device,
                            &gpu.queue,
                            &bytes,
                            indices.len() as u32,
                            1,
                            Some(&indices),
                        )
                    })
                    .collect::<Vec<_>>();
                // Geometry-identical commands deliberately reuse an instance;
                // unique instances are assigned in reverse native sequence.
                let key = |step: &serde_json::Value, y: u32| {
                    (
                        step["destination_offset"].as_u64().unwrap() as u32,
                        step["decoded_indices"].as_array().unwrap().len() as u32,
                        step["candidate"].as_i64().unwrap() as i32,
                        y,
                    )
                };
                let mut keys = Vec::new();
                let mut instances: Vec<SpriteInstance> = Vec::new();
                for step in steps.iter().rev() {
                    for y in 0..height {
                        let k = key(step, y);
                        if !keys.contains(&k) {
                            keys.push(k);
                            instances.push(sprite(
                                [k.0 as f32, y as f32],
                                [k.1 as f32, 1.],
                                (k.2 - (32768 - y as i32)) as f32,
                            ));
                        }
                    }
                }
                let commands = steps
                    .iter()
                    .enumerate()
                    .flat_map(|(page, step)| {
                        let keys = &keys;
                        (0..height).map(move |y| {
                            read_only(
                                keys.iter().position(|k| *k == key(step, y)).unwrap() as u32,
                                if step["leaf"].as_str().unwrap().ends_with("shadow") {
                                    TerrainPiece::Shadow
                                } else {
                                    TerrainPiece::Body
                                },
                                page,
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                let buffer = gpu
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Reversed and reused native source instances"),
                        contents: bytemuck::cast_slice(&instances),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
                for prefix in 1..=steps.len() {
                    // All prefixes at height1; one full span for forced bands.
                    if height != 1 && prefix != steps.len() {
                        continue;
                    }
                    let bytes = (0..height)
                        .flat_map(|_| old_words.iter().flat_map(|&word| encoded(word, format)))
                        .collect::<Vec<_>>();
                    upload_color(&gpu, &color, &bytes);
                    let mut encoder = gpu.device.create_command_encoder(&Default::default());
                    seed_depth(&gpu, &mut encoder, &dv, &old_z);
                    let stats = terrain.draw_span(
                        &mut encoder,
                        &cv,
                        &dv,
                        &batch,
                        |slot| atlases.get(slot),
                        &buffer,
                        &instances,
                        commands[..prefix * height as usize].iter().copied(),
                        [0, 0, 4, height],
                    );
                    assert_eq!(stats.passes, 4 * height as usize);
                    assert_eq!(stats.waves, 1);
                    assert_eq!(stats.tile_dependencies, 0);
                    let reads = [
                        gpu.read(&mut encoder, &color),
                        gpu.read(&mut encoder, &depth),
                    ];
                    let output = gpu.finish(encoder, &reads, size);
                    for y in 0..height as usize {
                        for x in 0..4 {
                            let offset = (y * 4 + x) * 4;
                            let expected = steps[prefix - 1]["colors"][x].as_u64().unwrap() as u16;
                            assert_eq!(
                                output[0][offset..offset + 4],
                                encoded(expected, format),
                                "case={} prefix={prefix} format={format:?} y={y} x={x}",
                                case["name"]
                            );
                            assert_eq!(
                                output[1][offset..offset + 4],
                                crate::render::native_z::stored_depth(old_z[x]).to_le_bytes(),
                                "original read-only depth"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "requires GPU; empty/no-op spans and command-ordinal chunk boundary"]
fn ordered_reduction_preserves_noop_bytes_and_chunk_continuation() {
    let gpu = Gpu::new();
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let size = [1, 1];
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        batch.write_camera(&gpu.queue, camera(size));
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        let atlases = [0, 1].map(|index| {
            batch.create_texture_on_device(
                &gpu.device,
                &gpu.queue,
                &encoded(0xf800, wgpu::TextureFormat::Rgba8UnormSrgb),
                1,
                1,
                Some(&[index]),
            )
        });
        let instances = [
            sprite([0.; 2], [1.; 2], 4096. - 32768.),
            sprite([0.; 2], [1.; 2], 5000. - 32768.),
        ];
        let buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX,
            });
        for commands in [
            vec![],
            vec![
                read_only(0, TerrainPiece::Body, 0),
                read_only(0, TerrainPiece::Shadow, 0),
                read_only(1, TerrainPiece::Body, 1),
                read_only(1, TerrainPiece::Shadow, 1),
            ],
        ] {
            let bytes = [19, 37, 73, 255]; // Deliberately not representable in RGB565.
            upload_color(&gpu, &color, &bytes);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            clear(&mut encoder, &cv, &dv, 5000, wgpu::LoadOp::Load);
            terrain.draw_span(
                &mut encoder,
                &cv,
                &dv,
                &batch,
                |slot| atlases.get(slot),
                &buffer,
                &instances,
                commands,
                [0, 0, 1, 1],
            );
            let reads = [gpu.read(&mut encoder, &color)];
            assert_eq!(gpu.finish(encoder, &reads, size)[0], bytes);
        }
        let mut commands =
            vec![read_only(0, TerrainPiece::Body, 0); super::super::READ_ONLY_COMMAND_LIMIT - 1];
        commands.push(read_only(0, TerrainPiece::Body, 1));
        commands.push(read_only(0, TerrainPiece::Shadow, 1));
        // Last body is exactly ordinal65535, followed by a new chunk shadow.
        // The independent exhaustive original-leaf corpus supplies the red
        // destination's shadow transition across this implementation chunk.
        let words =
            crate::test_fixture::bytes("tools/projectile_oracle/bridge_render_pixels.rgb565.bin");
        let expected = u16::from_le_bytes(words[0xf800 * 2..0xf800 * 2 + 2].try_into().unwrap());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(
            &mut encoder,
            &cv,
            &dv,
            5000,
            wgpu::LoadOp::Clear(wgpu::Color::WHITE),
        );
        let stats = terrain.draw_span(
            &mut encoder,
            &cv,
            &dv,
            &batch,
            |slot| atlases.get(slot),
            &buffer,
            &instances,
            commands,
            [0, 0, 1, 1],
        );
        assert_eq!((stats.pieces, stats.waves, stats.passes), (65536, 2, 8));
        let reads = [
            gpu.read(&mut encoder, &color),
            gpu.read(&mut encoder, &depth),
        ];
        let output = gpu.finish(encoder, &reads, size);
        assert_eq!(output[0], encoded(expected, format));
        assert_eq!(
            output[1],
            crate::render::native_z::stored_depth(5000).to_le_bytes()
        );
    }
}

#[test]
fn original_exhaustive_shadow_transition_is_zero_after_six_applications() {
    let words =
        crate::test_fixture::bytes("tools/projectile_oracle/bridge_render_pixels.rgb565.bin");
    for start in 0..=u16::MAX {
        let mut word = start;
        for _ in 0..6 {
            let offset = usize::from(word) * 2;
            word = u16::from_le_bytes(words[offset..offset + 2].try_into().unwrap());
        }
        assert_eq!(word, 0, "original transition starting at {start}");
    }
}
