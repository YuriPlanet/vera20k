//! Actual shared production renderer, native ellipse/cubic-line pixels and
//! immutable Z. Prepared actor/clock/plane priors are not a native whole frame.
use super::*;
use crate::render::batch::BatchRenderer;
use crate::render::radial_indicator::tests::{draws, integer, native, point};
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, encoded, seed_depth_grid};
use serde_json::Value;

fn patched_plane(row: &Value, name: &str, default: u16) -> Vec<u16> {
    let mut plane = vec![default; 160 * 160];
    for patch in row["planes"][name].as_array().unwrap() {
        plane[integer(&patch[0]) as usize] = integer(&patch[1]) as u16;
    }
    plane
}

#[test]
#[ignore = "requires GPU; native radial ellipses, primary-gated AA pairs, ordering and chunk boundaries"]
fn production_radial_pixels_match_original_in_both_formats_and_immutable_depth() {
    let gpu = Gpu::new();
    let size = [160, 160];
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        batch.write_camera(&gpu.queue, camera(size));
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        for (scene, row) in native()["scenes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (true, r))
            .chain(
                native()["ray_controls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| (false, r)),
            )
        {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let initial =
                encoded(integer(&row["planes"]["background"]) as u16, format).repeat(160 * 160);
            let depths = patched_plane(row, "z_patches", integer(&row["planes"]["old_z"]) as u16);
            let alpha = patched_plane(row, "a_patches", integer(&row["planes"]["a_value"]) as u16);
            let radials = if scene { draws(row) } else { Vec::new() };
            let lines = if scene {
                Vec::new()
            } else {
                vec![SurfaceLine {
                    from: point(&input["start"]),
                    to: point(&input["end"]),
                    z_adjust: point(&input["z"]),
                    blend: SurfaceLineBlend::Antialiased {
                        rgb: point::<3>(&input["rgb"]).map(|v| v as u8),
                        intensity: crate::util::native_x87::NativeF32Bits::from_bits(
                            u32::from_str_radix(input["strength_f32_bits"].as_str().unwrap(), 16)
                                .unwrap(),
                        ),
                    },
                }]
            };
            let color = gpu.target(size, format);
            let cv = color.create_view(&Default::default());
            let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
            let dv = depth.create_view(&Default::default());
            terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
            let mut expected = initial.clone();
            for pixel in row["pixels"].as_array().unwrap() {
                let at = (integer(&pixel[1]) as usize * 160 + integer(&pixel[0]) as usize) * 4;
                expected[at..at + 4].copy_from_slice(&encoded(integer(&pixel[2]) as u16, format));
            }
            let mut limits = vec![None];
            if name.starts_with("pair_") {
                limits.push(Some(1));
            }
            if name.contains("overlap") {
                limits.push(Some(97));
            }
            for limit in limits {
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
                        bytes_per_row: Some(160 * 4),
                        rows_per_image: Some(160),
                    },
                    color.size(),
                );
                terrain.begin_surface_lines();
                let prepared = terrain.prepare_surface_lines(
                    &gpu.device,
                    &gpu.queue,
                    std::iter::empty(),
                    &lines,
                    &[],
                    &radials,
                    SurfaceLineViewport {
                        camera: [0; 2],
                        clip: point(&input["clip"]),
                        z_origin_y: 0,
                        zoom: 1.,
                    },
                    || true,
                    |[x, y]| alpha[(y * 160 + x) as usize],
                );
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                seed_depth_grid(&gpu, &mut encoder, &dv, &depths, 160);
                terrain.draw_surface_lines(&mut encoder, &cv, prepared);
                let reads = [
                    gpu.read(&mut encoder, &color),
                    gpu.read(&mut encoder, &depth),
                ];
                let output = gpu.finish(encoder, &reads, size);
                let differences: Vec<_> = output[0]
                    .chunks_exact(4)
                    .zip(expected.chunks_exact(4))
                    .enumerate()
                    .filter(|(_, (actual, expected))| actual != expected)
                    .map(|(i, (actual, expected))| {
                        (i % 160, i / 160, actual.to_vec(), expected.to_vec())
                    })
                    .collect();
                assert_eq!(
                    differences.len(),
                    0,
                    "{format:?} {name} chunk{limit:?} first{:?}",
                    &differences[..differences.len().min(8)]
                );
                let expected_z: Vec<u8> = depths
                    .iter()
                    .flat_map(|&word| crate::render::native_z::stored_depth(word).to_le_bytes())
                    .collect();
                assert!(
                    output[1] == expected_z,
                    "immutable Z {format:?} {name} chunk{limit:?}"
                );
            }
        }
    }
}
