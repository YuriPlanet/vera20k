//! Real shared packed-surface GPU stage against original straight-beam pixels.
use super::*;
use crate::render::batch::BatchRenderer;
use crate::render::radiation_beam::tests as native;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded, seed_depth_grid};
use crate::util::sha256::sha256_hex;

#[test]
#[ignore = "requires GPU; original RadBeam RGB565/A/Z, preclipping and bounded batches"]
fn production_radbeam_pixels_match_original_and_preserve_depth() {
    let gpu = Gpu::new();
    let size = [160, 120];
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
        for row in native::drawing_rows() {
            let name = &row["input"]["name"];
            let surface = &row["surface"];
            let background = surface["background"].as_u64().unwrap() as u16;
            let mut expected_words = vec![background; (size[0] * size[1]) as usize];
            for pixel in row["after"]["pixels"].as_array().unwrap() {
                let x = pixel[0].as_u64().unwrap() as usize;
                let y = pixel[1].as_u64().unwrap() as usize;
                expected_words[y * size[0] as usize + x] = pixel[2].as_u64().unwrap() as u16;
            }
            let native_bytes = expected_words
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>();
            assert_eq!(
                sha256_hex(&native_bytes),
                row["after"]["pixel_sha256"].as_str().unwrap(),
                "{name} lossless whole-plane expansion"
            );
            let z = (0..size[1])
                .flat_map(|y| {
                    (0..size[0]).map(move |x| {
                        surface["z"].as_array().map_or_else(
                            || surface["z"].as_u64().unwrap() as u16,
                            |z| {
                                z[((x / 11 + y / 7) % z.len() as u32) as usize]
                                    .as_u64()
                                    .unwrap() as u16
                            },
                        )
                    })
                })
                .collect::<Vec<_>>();
            let alpha = (0..size[1])
                .flat_map(|y| {
                    (0..size[0]).map(move |x| match surface["alpha"].as_str().unwrap() {
                        "mixed" => [0_u16, 1, 63, 127, 255][((x / 9 + y / 7) % 5) as usize],
                        "black" => 0,
                        "clear" => 127,
                        other => panic!("unknown declared native A pattern: {other}"),
                    })
                })
                .collect::<Vec<_>>();
            for (plane, key) in [(&z, "z"), (&alpha, "alpha")] {
                let digest = sha256_hex(
                    &plane
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect::<Vec<_>>(),
                );
                assert_eq!(digest, row["after"][format!("{key}_before_sha256")]);
                assert_eq!(digest, row["after"][format!("{key}_after_sha256")]);
            }
            let draws = native::lines(row)
                .into_iter()
                .map(SurfaceDraw::Line)
                .collect::<Vec<_>>();
            for limit in [None, Some(97)] {
                terrain.surface_lines.test_operation_limit = limit;
                let bytes = encoded(background, format).repeat(expected_words.len());
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
                terrain.begin_surface_lines();
                let prepared = terrain.prepare_surface_lines(
                    &gpu.device,
                    &gpu.queue,
                    std::iter::empty(),
                    std::iter::empty(),
                    &[],
                    &draws,
                    &[],
                    native::viewport(row),
                    |_| true,
                    |[x, y]| alpha[(y as u32 * size[0] + x as u32) as usize],
                );
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                if surface["z"].is_array() {
                    seed_depth_grid(&gpu, &mut encoder, &dv, &z, size[0]);
                } else {
                    clear(&mut encoder, &cv, &dv, z[0], wgpu::LoadOp::Load);
                }
                terrain.draw_surface_lines(&mut encoder, &cv, prepared);
                let reads = [
                    gpu.read(&mut encoder, &color),
                    gpu.read(&mut encoder, &depth),
                ];
                let output = gpu.finish(encoder, &reads, size);
                let expected = expected_words
                    .iter()
                    .flat_map(|&v| encoded(v, format))
                    .collect::<Vec<_>>();
                let mismatches = output[0]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(expected.as_chunks::<4>().0)
                    .enumerate()
                    .filter_map(|(i, (a, b))| (a != b).then_some((i, a, b)))
                    .collect::<Vec<_>>();
                assert!(
                    mismatches.is_empty(),
                    "{name} {format:?} limit{limit:?}: {} different pixels, first{:?}",
                    mismatches.len(),
                    &mismatches[..mismatches.len().min(8)]
                );
                let expected_z = z
                    .iter()
                    .flat_map(|&v| crate::render::native_z::stored_depth(v).to_le_bytes())
                    .collect::<Vec<_>>();
                assert_eq!(output[1], expected_z, "{name} {format:?} immutable Z");
            }
        }
    }
}
