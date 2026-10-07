//! Original Bullet-selected plain/RLE blitter outputs through the production
//! destination-edit owner. The fixture supplies decoded source indices and a
//! prepared Convert palette; it does not establish asset/Display integration.

use super::{DestinationEditCommand, TerrainDrawRenderer, TerrainPiece};
use crate::render::batch::BatchRenderer;
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded, extent, sprite};
use wgpu::util::DeviceExt;

fn native() -> serde_json::Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/bridge_render_pixels.json",
    ))
    .unwrap()
}

fn read_only(index: u32, piece: TerrainPiece, atlas_slot: usize) -> DestinationEditCommand {
    DestinationEditCommand {
        index,
        piece,
        render_z: RenderZPolicy::ReadOnly,
        atlas_slot,
    }
}

#[test]
#[ignore = "requires GPU; original Bullet signed depth, zero stencil and repeated destination edits"]
fn production_bullet_body_shadow_matches_original_blitters() {
    let gpu = Gpu::new();
    let native = native();
    let cases = native["rows"].as_array().unwrap();
    assert_eq!(cases.len(), 400);
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let size = [16, 5];
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        batch.write_camera(&gpu.queue, camera(size));
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        // Prepared native Convert maps source index1 to RGB565 55AA. The
        // transparent page deliberately has nonzero RGBA: the source index,
        // not atlas alpha/color, owns the native zero-run stencil.
        let atlases = [0, 1].map(|index| {
            batch.create_texture_on_device(
                &gpu.device,
                &gpu.queue,
                &[80, 180, 80, 255],
                1,
                1,
                Some(&[index]),
            )
        });
        for old in [0u16, 1000, 1001, 32768, 65535] {
            let rows = cases
                .iter()
                .filter(|row| row["old_z"].as_u64() == Some(u64::from(old)))
                .collect::<Vec<_>>();
            assert_eq!(rows.len(), (size[0] * size[1]) as usize);
            let instances = rows
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    let y = (index / size[0] as usize) as i32;
                    let candidate = row["candidate"].as_i64().unwrap() as i32;
                    sprite(
                        [(index % size[0] as usize) as f32, y as f32],
                        [1.; 2],
                        (candidate - (32768 - y)) as f32,
                    )
                })
                .collect::<Vec<_>>();
            let commands = rows
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    read_only(
                        index as u32,
                        if row["leaf"].as_str().unwrap().ends_with("shadow") {
                            TerrainPiece::Shadow
                        } else {
                            TerrainPiece::Body
                        },
                        usize::from(!row["transparent"].as_bool().unwrap()),
                    )
                })
                .collect::<Vec<_>>();
            let buffer = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Original Bullet leaf inputs"),
                    contents: bytemuck::cast_slice(&instances),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            for repeat in [false, true] {
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                if !repeat {
                    clear(
                        &mut encoder,
                        &cv,
                        &dv,
                        old,
                        wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                    );
                }
                terrain.draw_span(
                    &mut encoder,
                    &cv,
                    &dv,
                    &batch,
                    |slot| atlases.get(slot),
                    &buffer,
                    &instances,
                    commands.iter().copied(),
                    [0, 0, size[0], size[1]],
                );
                let reads = [
                    gpu.read(&mut encoder, &color),
                    gpu.read(&mut encoder, &depth),
                ];
                let output = gpu.finish(encoder, &reads, size);
                for (index, row) in rows.iter().enumerate() {
                    let field = if repeat {
                        "repeat_color"
                    } else {
                        "first_color"
                    };
                    let word = row[field].as_u64().unwrap() as u16;
                    assert_eq!(
                        output[0][index * 4..index * 4 + 4],
                        encoded(word, format),
                        "{format:?}, repeat={repeat}, native={row}"
                    );
                    let field = if repeat { "repeat_z" } else { "first_z" };
                    let z = row[field].as_u64().unwrap() as u16;
                    assert_eq!(
                        output[1][index * 4..index * 4 + 4],
                        crate::render::native_z::stored_depth(z).to_le_bytes(),
                        "depth must remain bit-identical: {format:?}, repeat={repeat}, native={row}"
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "requires GPU; all original RGB565 Bullet shadow words with unchanged depth"]
fn production_bullet_shadow_matches_every_original_destination_word() {
    let gpu = Gpu::new();
    let native = native();
    let words =
        crate::test_fixture::bytes("tools/projectile_oracle/bridge_render_pixels.rgb565.bin");
    assert_eq!(words.len(), 65536 * 2);
    for row in native["packed"].as_array().unwrap() {
        assert_eq!(
            crate::util::sha256::sha256_hex(words),
            row["pixel_sha256"].as_str().unwrap()
        );
        assert_eq!(row["all_z_unchanged"], true);
    }
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let size = [256; 2];
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        batch.write_camera(&gpu.queue, camera(size));
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        let bytes = (0..=u16::MAX)
            .flat_map(|word| encoded(word, format))
            .collect::<Vec<_>>();
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
                bytes_per_row: Some(1024),
                rows_per_image: Some(256),
            },
            extent(size),
        );
        let atlas =
            batch.create_texture_on_device(&gpu.device, &gpu.queue, &[255; 4], 1, 1, Some(&[1]));
        // One instance per row preserves the native leaf's constant4096
        // candidate across its flat65536-word destination fixture.
        let instances = (0..256)
            .map(|row| sprite([0., row as f32], [256., 1.], (4096 - 32768 + row) as f32))
            .collect::<Vec<_>>();
        let buffer = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Original Bullet RGB565 destination inputs"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(&mut encoder, &cv, &dv, 5000, wgpu::LoadOp::Load);
        terrain.draw_span(
            &mut encoder,
            &cv,
            &dv,
            &batch,
            |_| Some(&atlas),
            &buffer,
            &instances,
            (0..256).map(|index| read_only(index, TerrainPiece::Shadow, 0)),
            [0, 0, 256, 256],
        );
        let reads = [
            gpu.read(&mut encoder, &color),
            gpu.read(&mut encoder, &depth),
        ];
        let output = gpu.finish(encoder, &reads, size);
        for (index, word) in words.chunks_exact(2).enumerate() {
            assert_eq!(
                output[0][index * 4..index * 4 + 4],
                encoded(u16::from_le_bytes(word.try_into().unwrap()), format),
                "{format:?} word{index:04X}"
            );
            assert_eq!(
                output[1][index * 4..index * 4 + 4],
                crate::render::native_z::stored_depth(5000).to_le_bytes(),
                "{format:?} word{index:04X} depth"
            );
        }
    }
}

#[test]
#[ignore = "requires GPU; original plain/RLE decoded zero-run stencil"]
fn production_bullet_decoded_zero_runs_preserve_color_and_depth() {
    let gpu = Gpu::new();
    let native = native();
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let size = [4, 1];
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera(size));
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
    let instance = sprite([0.; 2], [4., 1.], (4096 - 32768) as f32);
    let buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Original Bullet decoded run"),
            contents: bytemuck::bytes_of(&instance),
            usage: wgpu::BufferUsages::VERTEX,
        });
    // Same initial destination input as bridge_render_pixels.py. RLE parsing
    // itself remains the atlas owner's responsibility; these are decoded bytes.
    let initial = [0xffff, 0x39e7, 0x07e0, 0xf800]
        .into_iter()
        .flat_map(|word| encoded(word, format))
        .collect::<Vec<_>>();
    for row in native["stencil"].as_array().unwrap() {
        let indices = row["decoded_indices"]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| index.as_u64().unwrap() as u8)
            .collect::<Vec<_>>();
        let atlas = batch.create_texture_on_device(
            &gpu.device,
            &gpu.queue,
            &[80, 180, 80, 255].repeat(4),
            4,
            1,
            Some(&indices),
        );
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &color,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &initial,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(16),
                rows_per_image: Some(1),
            },
            extent(size),
        );
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(&mut encoder, &cv, &dv, 5000, wgpu::LoadOp::Load);
        let piece = if row["leaf"].as_str().unwrap().ends_with("shadow") {
            TerrainPiece::Shadow
        } else {
            TerrainPiece::Body
        };
        terrain.draw_span(
            &mut encoder,
            &cv,
            &dv,
            &batch,
            |_| Some(&atlas),
            &buffer,
            &[instance],
            [read_only(0, piece, 0)],
            [0, 0, 4, 1],
        );
        let reads = [
            gpu.read(&mut encoder, &color),
            gpu.read(&mut encoder, &depth),
        ];
        let output = gpu.finish(encoder, &reads, size);
        for index in 0..4 {
            assert_eq!(
                output[0][index * 4..index * 4 + 4],
                encoded(row["colors"][index].as_u64().unwrap() as u16, format),
                "{row}, pixel{index}"
            );
            assert_eq!(output[1][index * 4..index * 4 + 4], crate::render::native_z::stored_depth(row["depths"][index].as_u64().unwrap() as u16).to_le_bytes(), "{row}, depth{index}");
        }
    }
}

#[test]
#[ignore = "bounded CPU/completed-submission timing; close other renderers before uncontended measurements"]
fn production_bullet_small_piece_workload_timing() {
    use std::time::{Duration, Instant};
    let gpu = Gpu::new();
    let size = [1024, 768];
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera(size));
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
    let atlases = [[80, 180, 80, 255], [16, 64, 240, 255]].map(|rgba| {
        batch.create_texture_on_device(&gpu.device, &gpu.queue, &rgba, 1, 1, Some(&[1]))
    });
    let mut results = Vec::new();
    for pairs in [32usize, 128, 512] {
        for layout in ["separated", "dense"] {
            let instances = (0..pairs)
                .flat_map(|index| {
                    let [x, y] = if layout == "separated" {
                        [
                            4. + (index % 32) as f32 * 32.,
                            4. + (index / 32) as f32 * 32.,
                        ]
                    } else {
                        [516., 388.]
                    };
                    // Within-pair overlap is intentional; separated pairs fit
                    // independent32px dependency tiles on the declared target.
                    [
                        sprite([x, y], [4.; 2], -1.),
                        sprite([x + 1., y + 3.], [4.; 2], -1.),
                    ]
                })
                .collect::<Vec<_>>();
            let buffer = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Bounded four-pixel Bullet workload"),
                    contents: bytemuck::cast_slice(&instances),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            for atlas_count in [1, 2] {
                eprintln!(
                    "Bullet timing begins: pairs={pairs}, layout={layout}, atlases={atlas_count}"
                );
                // One warm-up and three single-frame samples. No asynchronous
                // frame backlog or per-frame asset/upload work is introduced.
                // Cross-pass GPU timestamps returned reversed intervals on M4
                // Metal; use an explicitly CPU+submission+GPU+wait wall interval
                // instead of claiming an invalid GPU-only duration.
                for sample in 0..4 {
                    let begin = Instant::now();
                    let mut encoder = gpu.device.create_command_encoder(&Default::default());
                    clear(
                        &mut encoder,
                        &cv,
                        &dv,
                        65535,
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
                        (0..instances.len()).map(|index| {
                            read_only(
                                index as u32,
                                if index % 2 == 0 {
                                    TerrainPiece::Body
                                } else {
                                    TerrainPiece::Shadow
                                },
                                (index / 2) % atlas_count,
                            )
                        }),
                        [0, 0, size[0], size[1]],
                    );
                    let cpu_ms = begin.elapsed().as_secs_f64() * 1000.;
                    let submission = gpu.queue.submit([encoder.finish()]);
                    gpu.device
                        .poll(wgpu::PollType::Wait {
                            submission_index: Some(submission),
                            timeout: Some(Duration::from_secs(60)),
                        })
                        .unwrap();
                    let completed_ms = begin.elapsed().as_secs_f64() * 1000.;
                    assert_eq!(stats.pieces, pairs * 2);
                    assert_eq!(stats.waves, 1);
                    assert_eq!(stats.passes, 4);
                    assert_eq!(stats.tile_dependencies, 0);
                    if sample > 0 {
                        eprintln!(
                            "Bullet timing sample: pairs={pairs}, layout={layout}, atlases={atlas_count}, sample={sample}, pieces={}, waves={}, passes={}, tile_dependencies={}, cpu_prepare_encode_ms={cpu_ms}, completed_submission_wall_ms={completed_ms}",
                            stats.pieces, stats.waves, stats.passes, stats.tile_dependencies
                        );
                        results.push(serde_json::json!({
                            "shell_pairs": pairs, "layout": layout, "atlas_pages": atlas_count,
                            "sample": sample, "frames": 1, "pieces": stats.pieces,
                            "waves": stats.waves, "passes": stats.passes,
                            "tile_dependencies": stats.tile_dependencies,
                            "cpu_prepare_encode_ms_per_frame": cpu_ms,
                            "completed_submission_wall_ms_per_frame": completed_ms,
                        }));
                    }
                }
            }
        }
    }
    let report = serde_json::json!({
        "target": size, "color_format": "Bgra8UnormSrgb",
        "source": "synthetic4x4 opaque body/shadow pairs through production read-only destination-edit owner; no full-scene performance claim",
        "interval": "CPU: encoder creation, frame clears, scheduler reset/preparation and encoding. Wall: same start through completed submission, including CPU+finish+submission+GPU+poll wait. Both exclude setup/upload. One warmup then three measured frames per workload. Debug test executable, not release app FPS.",
        "gpu_only_timing": "unavailable: encoder and cross-pass marker timestamps returned non-increasing/reversed intervals on Apple M4 Metal; no GPU-only duration reported",
        "samples": results,
    });
    eprintln!("{}", serde_json::to_string_pretty(&report).unwrap());
    if let Some(path) = std::env::var_os("VERA20K_PROJECTILE_PERF_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}

#[path = "projectile_ordered_gpu_tests.rs"]
mod ordered;
