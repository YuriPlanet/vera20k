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
            terrain.prepare_surface_lines(
                &gpu.device,
                &gpu.queue,
                std::iter::empty(),
                &segments,
                viewport,
                || true,
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
            terrain.draw_surface_lines(&mut encoder, &cv);
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
                terrain.prepare_surface_lines(
                    &gpu.device,
                    &gpu.queue,
                    std::iter::empty(),
                    &segments,
                    ordered_viewport(row),
                    || true,
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
                terrain.draw_surface_lines(&mut encoder, &cv);
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
#[ignore = "requires quiet GPU; reports bounded debug preparation + completed submission timing, not FPS"]
fn line_trail_native_fixture_submission_timings() {
    use std::time::Instant;
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
            let segments = overlapping_segments(row);
            let mut samples = Vec::new();
            for repetition in 0..22 {
                let begin = Instant::now();
                terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
                terrain.prepare_surface_lines(
                    &gpu.device,
                    &gpu.queue,
                    std::iter::empty(),
                    &segments,
                    ordered_viewport(row),
                    || true,
                    |_| row["input"]["alpha"].as_u64().unwrap() as u16,
                );
                let prepared = begin.elapsed().as_secs_f64() * 1000.;
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                clear(
                    &mut encoder,
                    &cv,
                    &dv,
                    65535,
                    wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                );
                terrain.draw_surface_lines(&mut encoder, &cv);
                gpu.queue.submit([encoder.finish()]);
                gpu.device
                    .poll(wgpu::PollType::wait_indefinitely())
                    .unwrap();
                if repetition >= 2 {
                    samples.push((prepared, begin.elapsed().as_secs_f64() * 1000.));
                }
            }
            let median = |field: usize| {
                let mut values = samples
                    .iter()
                    .map(|v| if field == 0 { v.0 } else { v.1 })
                    .collect::<Vec<_>>();
                values.sort_by(f64::total_cmp);
                values[values.len() / 2]
            };
            eprintln!(
                "LINE_TRAIL_TIMING {}",
                serde_json::json!({"format":format!("{format:?}"),"trails":row["count"],"alpha":row["input"]["alpha"],"size":size,"samples":samples.len(),"prepare_ms_median":median(0),"complete_ms_median":median(1),"complete_ms_min":samples.iter().map(|v|v.1).fold(f64::INFINITY,f64::min),"complete_ms_max":samples.iter().map(|v|v.1).fold(0.,f64::max)})
            );
        }
    }
}
