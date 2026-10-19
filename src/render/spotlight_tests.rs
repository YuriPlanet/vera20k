//! Original4BBCA0 store order, candidate Z and final pixels through production.
use super::*;
#[test]
fn spotlight_brightening_uses_original_clipped_three_axis_visits_without_alpha() {
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/spotlight.json");
    for row in native["lines"].as_array().unwrap() {
        let a = |v: &serde_json::Value| serde_json::from_value::<[i32; 2]>(v.clone()).unwrap();
        let clip: [i32; 4] = serde_json::from_value(row["clip"].clone()).unwrap();
        let mut visits = Vec::new();
        raster_draw(
            SurfaceDraw::Line(SurfaceLine {
                from: a(&row["first"]),
                to: a(&row["last"]),
                z_adjust: a(&row["z_adjust"]),
                blend: SurfaceLineBlend::Brighten { amount: 75 },
            }),
            SurfaceLineViewport {
                camera: [0, 0],
                clip,
                z_origin_y: row["z_origin_y"].as_i64().unwrap() as i32,
                zoom: 1.,
            },
            [clip[2] as u32, clip[3] as u32],
            &mut |_| panic!("original4BBCA0 never samples A"),
            &mut |ops| {
                for &(point, op) in ops {
                    assert_eq!((op.mode, op.strength, op.alpha), (8, 75, 127));
                    visits.push([point[0], point[1], op.z as i32]);
                }
            },
        )
        .unwrap();
        let expected = serde_json::from_value::<Vec<[i32; 3]>>(row["visits"].clone()).unwrap();
        if a(&row["first"])[0] < clip[0] {
            // Preserve the existing shared7BC2B0 owner's documented nearest-f64
            // edge residual. This selected native clip differs by one row/Z;
            // uncut controls below remain exact. No caller-specific clip fork.
            assert_eq!(visits.len(), expected.len());
            for (actual, native) in visits.iter().zip(&expected) {
                assert_eq!(actual[0], native[0]);
                assert!((actual[1] - native[1]).abs() <= 1 && (actual[2] - native[2]).abs() <= 1);
            }
        } else {
            assert_eq!(visits, expected, "{row}");
        }
    }
}

#[test]
#[ignore = "requires GPU; original DSurface4BBCA0 packed pixels and unchanged shared depth"]
fn production_spotlight_brightening_pixels_match_original() {
    use crate::render::batch::BatchRenderer;
    use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded};

    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/spotlight.json");
    let gpu = Gpu::new();
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut renderer = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        let mut compared = 0;
        for row in native["lines"].as_array().unwrap() {
            let from: [i32; 2] = serde_json::from_value(row["first"].clone()).unwrap();
            let to: [i32; 2] = serde_json::from_value(row["last"].clone()).unwrap();
            let clip: [i32; 4] = serde_json::from_value(row["clip"].clone()).unwrap();
            if from[0] < clip[0] {
                // This retained clipped row has the shared7BC2B0 one-row/Z
                // residual documented above. It remains covered by the CPU
                // test; the uncut controls have exact native pixel goldens.
                continue;
            }
            let size: [u32; 2] = serde_json::from_value(row["size"].clone()).unwrap();
            let background = row["background"].as_u64().unwrap() as u16;
            let old_z = row["old_z"].as_u64().unwrap() as u16;
            assert_eq!(row["z_unchanged"], true);
            assert_eq!(row["alpha_unchanged"], true);
            batch.write_camera(&gpu.queue, camera(size));
            let color = gpu.target(size, format);
            let color_view = color.create_view(&Default::default());
            let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
            let depth_view = depth.create_view(&Default::default());
            renderer.prepare(&gpu.device, &color, &depth_view, batch.camera_uniform());
            let initial = encoded(background, format).repeat((size[0] * size[1]) as usize);
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
            renderer.begin_surface_lines();
            let prepared = renderer.prepare_surface_lines(
                &gpu.device,
                &gpu.queue,
                std::iter::empty(),
                std::iter::once(SurfaceDraw::Line(SurfaceLine {
                    from,
                    to,
                    z_adjust: serde_json::from_value(row["z_adjust"].clone()).unwrap(),
                    blend: SurfaceLineBlend::Brighten {
                        amount: row["amount"].as_i64().unwrap() as i32,
                    },
                })),
                &[],
                &[],
                &[],
                SurfaceLineViewport {
                    camera: [0, 0],
                    clip,
                    z_origin_y: row["z_origin_y"].as_i64().unwrap() as i32,
                    zoom: 1.,
                },
                |_| true,
                |_| panic!("original4BBCA0 must not read A"),
            );
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            clear(
                &mut encoder,
                &color_view,
                &depth_view,
                old_z,
                wgpu::LoadOp::Load,
            );
            renderer.draw_surface_lines(&mut encoder, &color_view, prepared);
            let reads = [
                gpu.read(&mut encoder, &color),
                gpu.read(&mut encoder, &depth),
            ];
            let output = gpu.finish(encoder, &reads, size);
            let mut expected = initial;
            for pixel in row["pixels"].as_array().unwrap() {
                let offset = (pixel[1].as_u64().unwrap() * u64::from(size[0])
                    + pixel[0].as_u64().unwrap()) as usize
                    * 4;
                expected[offset..offset + 4]
                    .copy_from_slice(&encoded(pixel[2].as_u64().unwrap() as u16, format));
            }
            assert_eq!(output[0], expected, "4BBCA0 {format:?} {from:?}->{to:?}");
            assert_eq!(
                output[1],
                crate::render::native_z::stored_depth(old_z)
                    .to_le_bytes()
                    .repeat((size[0] * size[1]) as usize),
                "4BBCA0 must not write Z: {format:?} {from:?}->{to:?}"
            );
            compared += 1;
        }
        assert_eq!(compared, 3, "all three uncut native controls are compared");
    }
}
