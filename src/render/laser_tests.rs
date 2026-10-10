//! Original full LaserDraw -> software DSurface comparisons, including live
//! production GPU destination blending and immutable native depth.
use super::*;
use crate::render::batch::BatchRenderer;
use crate::render::laser::{LaserBlend, LaserDraw, LaserLine};
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded, seed_depth_grid};
use crate::sim::projectile::ProjectileCoord;
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_prism.json",
    ))
    .unwrap()
}
fn integer(v: &Value) -> i32 {
    v.as_i64().unwrap() as i32
}
fn array<const N: usize>(v: &Value) -> [i32; N] {
    std::array::from_fn(|i| integer(&v[i]))
}
fn coord(v: &Value) -> ProjectileCoord {
    let [x, y, z] = array(v);
    ProjectileCoord::new(x, y, z)
}
fn draws(row: &Value) -> Vec<LaserDraw> {
    let laser = &row["laser"];
    let first = LaserDraw {
        from: coord(&laser["source"]),
        to: coord(&laser["target"]),
        z_adjust: integer(&laser["z_adjust"]),
        width: integer(&laser["width"]),
        supported: laser["supported"] == 1,
        rgb: array::<3>(&laser["inner"]).map(|v| v as u8),
        duration: integer(&laser["duration"]),
        age: integer(&laser["age"]),
    };
    let mut draws = Vec::new();
    if row["input"]["second_laser"] == true {
        // Original fixture's second constructor, then reverse registry draw.
        draws.push(LaserDraw {
            from: first.to,
            to: first.from,
            z_adjust: -2,
            width: 1,
            supported: false,
            rgb: [200, 150, 100],
            age: 0,
            duration: 15,
        });
    }
    draws.push(first);
    draws
}
fn high_detail(input: &Value) -> bool {
    input["detail"].as_i64().unwrap_or(2) != 0 && input["fps"].as_u64().unwrap_or(60) >= 15
}
fn viewport(input: &Value) -> SurfaceLineViewport {
    let mut camera: [i32; 2] = array(&input["camera"]);
    camera[1] += 15; // the shared VERA world-row convention
    SurfaceLineViewport {
        camera,
        clip: if input["clip"].is_array() {
            array(&input["clip"])
        } else {
            [0, 0, 160, 120]
        },
        z_origin_y: 0,
        zoom: 1.,
    }
}

#[test]
fn laser_geometry_color_and_fade_match_original_draw_arguments() {
    for row in corpus()["laser_draw"].as_array().unwrap() {
        let mut actual = Vec::new();
        let draws = draws(row);
        for (draw, fade) in draws
            .iter()
            .filter(|d| d.duration > 0)
            .zip(row["fade"].as_array().unwrap())
        {
            assert_eq!(
                draw.intensity().bits(),
                fade["intensity_bits"].as_u64().unwrap() as u32,
                "fade {}",
                row["input"]["name"]
            );
        }
        for draw in draws {
            draw.lines(
                viewport(&row["input"]).camera,
                high_detail(&row["input"]),
                |line| actual.push(line),
            );
        }
        let expected = row["segments"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|line| {
                let blend = if line["entry"] == "0x4bdf00" {
                    let scale = f64::from(f32::from_bits(
                        line["intensity_bits"].as_u64().unwrap() as u32
                    ));
                    let rgb = array::<3>(&line["rgb"]).map(|v| (f64::from(v) * scale) as u8);
                    if rgb.iter().all(|&v| v <= 7) {
                        return None;
                    }
                    LaserBlend::Add(rgb)
                } else {
                    LaserBlend::Replace(line["color"].as_u64().unwrap() as u16)
                };
                Some(LaserLine {
                    from: array(&line["from_point"]),
                    to: array(&line["to_point"]),
                    z_adjust: [integer(&line["z_start"]), integer(&line["z_end"])],
                    blend,
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "{}", row["input"]["name"]);
    }
}

#[test]
#[ignore = "requires GPU; full original house-laser RGB565 pixels, shared Z and bounded batches"]
fn production_laser_pixels_match_original_software_surface() {
    let native = corpus();
    let gpu = Gpu::new();
    let size = [160, 120];
    let mut failures = Vec::new();
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
        for row in native["laser_draw"].as_array().unwrap() {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            for chunk in [None, Some(97)] {
                if chunk.is_some() && name != "reverse_draw_order" && name != "detail2_saturation" {
                    continue;
                }
                terrain.surface_lines.test_operation_limit = chunk;
                let bytes = encoded(
                    input["background"].as_u64().unwrap_or(0x39e7) as u16,
                    format,
                )
                .repeat((size[0] * size[1]) as usize);
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
                terrain.prepare_surface_lines(
                    &gpu.device,
                    &gpu.queue,
                    draws(row),
                    &[],
                    viewport(input),
                    || high_detail(input),
                    |[x, y]| match input["alpha"].as_str() {
                        Some("mixed") => [0, 1, 63, 127, 255][((x / 9 + y / 7) % 5) as usize],
                        Some("black") => 0,
                        _ => 127,
                    },
                );
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                let z = (0..size[1])
                    .flat_map(|y| {
                        (0..size[0]).map(move |x| {
                            if let Some(z) = input["z"].as_array() {
                                z[((x / 11 + y / 7) % z.len() as u32) as usize]
                                    .as_u64()
                                    .unwrap() as u16
                            } else {
                                input["z"].as_u64().unwrap_or(65535) as u16
                            }
                        })
                    })
                    .collect::<Vec<_>>();
                if input["z"].is_array() {
                    seed_depth_grid(&gpu, &mut encoder, &dv, &z, size[0]);
                } else {
                    clear(&mut encoder, &cv, &dv, z[0], wgpu::LoadOp::Load);
                }
                terrain.draw_surface_lines(&mut encoder, &cv);
                let reads = [
                    gpu.read(&mut encoder, &color),
                    gpu.read(&mut encoder, &depth),
                ];
                let output = gpu.finish(encoder, &reads, size);
                let mut expected = bytes;
                for pixel in row["pixels"].as_array().unwrap() {
                    let index = (integer(&pixel[1]) as usize * size[0] as usize
                        + integer(&pixel[0]) as usize)
                        * 4;
                    expected[index..index + 4]
                        .copy_from_slice(&encoded(integer(&pixel[2]) as u16, format));
                }
                let mismatches = output[0]
                    .chunks_exact(4)
                    .zip(expected.chunks_exact(4))
                    .enumerate()
                    .filter(|(_, (a, b))| a != b)
                    .map(|(i, (a, b))| (i, a, b))
                    .collect::<Vec<_>>();
                if !mismatches.is_empty() {
                    failures.push(format!(
                        "{name} {format:?} chunk{chunk:?}: {} different pixels, first{:?}",
                        mismatches.len(),
                        &mismatches[..mismatches.len().min(8)]
                    ));
                }
                let expected_z = z
                    .iter()
                    .flat_map(|&z| crate::render::native_z::stored_depth(z).to_le_bytes())
                    .collect::<Vec<_>>();
                if output[1] != expected_z {
                    failures.push(format!("depth changed: {name} {format:?}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
