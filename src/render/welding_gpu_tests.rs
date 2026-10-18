//! Original ParticleDraw controls through the common surface upload/snapshot
//! and live signed-Z shader, including full-u16 A and chunk boundaries.

use super::*;
use crate::render::batch::BatchRenderer;
use crate::render::tactical_compat::welding_tests as fixture;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, encoded, seed_depth_grid};

#[test]
#[ignore = "requires GPU; original Welding points, full-u16 A, signed live Z, ordered overlap and chunk boundaries"]
fn production_welding_points_match_original_pixels_and_preserve_depth() {
    let gpu = Gpu::new();
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        for row in fixture::native()["spark_draw_cases"].as_array().unwrap() {
            let input = &row["input"];
            let size = fixture::array::<2>(&input["size"]).map(|value| value as u32);
            batch.write_camera(&gpu.queue, camera(size));
            let color = gpu.target(size, format);
            let cv = color.create_view(&Default::default());
            let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
            let dv = depth.create_view(&Default::default());
            terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
            let a = fixture::plane(input, "a");
            let z = fixture::plane(input, "z");
            let draws: Vec<_> = fixture::prepared_points(input, &row["draws"])
                .into_iter()
                .map(SurfaceDraw::Point)
                .collect();
            let initial = encoded(fixture::int(&input["background"]) as u16, format)
                .repeat((size[0] * size[1]) as usize);
            let expected: Vec<_> = fixture::expected_words(row)
                .into_iter()
                .flat_map(|word| encoded(word, format))
                .collect();
            for limit in [None, Some(1), Some(97)] {
                terrain.surface_lines.test_operation_limit = limit;
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
                        bytes_per_row: Some(size[0] * 4),
                        rows_per_image: Some(size[1]),
                    },
                    color.size(),
                );
                terrain.begin_surface_lines();
                let prepared = terrain.prepare_surface_lines(
                    &gpu.device,
                    &gpu.queue,
                    std::iter::empty(),
                    draws.iter().copied(),
                    &[],
                    &[],
                    &[],
                    SurfaceLineViewport {
                        camera: fixture::array(&input["camera"]),
                        clip: fixture::array(&input["clip"]),
                        z_origin_y: fixture::int(&input["z_origin_term"]),
                        zoom: 1.,
                    },
                    |_| false,
                    |[x, y]| a[(y as u32 * size[0] + x as u32) as usize],
                );
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                seed_depth_grid(&gpu, &mut encoder, &dv, &z, size[0]);
                terrain.draw_surface_lines(&mut encoder, &cv, prepared);
                let reads = [
                    gpu.read(&mut encoder, &color),
                    gpu.read(&mut encoder, &depth),
                ];
                let output = gpu.finish(encoder, &reads, size);
                assert_eq!(
                    output[0], expected,
                    "{} {format:?} limit{limit:?}",
                    input["name"]
                );
                let old_z: Vec<_> = z
                    .iter()
                    .flat_map(|&word| crate::render::native_z::stored_depth(word).to_le_bytes())
                    .collect();
                assert_eq!(output[1], old_z, "{} immutable Z {format:?}", input["name"]);
            }
        }
    }
}
