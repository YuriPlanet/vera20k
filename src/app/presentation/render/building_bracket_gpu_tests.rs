//! Bounded original bracket controls through the production Display/line path.
//! Destination Z is either a declared kernel input or original ground/body
//! output. The latter tests bracket occlusion, not the GPU building-body port.

use super::draw_plan_lowering::{PlannedObjectInstance, lower_ground_object_instances};
use super::merge_passes::draw_native_object_pass;
use crate::app::presentation::selection_brackets::building_bracket_lines;
use crate::render::batch::BatchRenderer;
use crate::render::surface_line::SurfaceLineViewport;
use crate::render::tactical_draw_plan::{BlitPolicy, ObjectDraw, SpriteEncoding, TacticalLayer};
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, encoded, seed_depth_grid};
use serde_json::Value;

fn array<const N: usize>(value: &Value) -> [i32; N] {
    std::array::from_fn(|index| value[index].as_i64().unwrap() as i32)
}

#[test]
#[ignore = "requires GPU; original GAPOWR lines through Display lowering and packed A/Z compositor"]
fn native_bracket_pixels_reach_display_replay_without_writing_z() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/building_brackets.json",
    ))
    .unwrap();
    let geometry = &native["geometry"];
    let gpu = Gpu::new();
    let size = [160, 160];
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        batch.write_camera(&gpu.queue, camera(size));
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        for row in native["raster_controls"].as_array().unwrap() {
            let input = &row["input"];
            let mut camera = array::<2>(&geometry["input"]["camera"]);
            // Translate the native camera into the projection owner's VERA
            // world-row frame, without exposing its private origin constant.
            camera[1] += crate::util::lepton::absolute_leptons_to_screen(0, 0, 0).1 as i32;
            let lines = building_bracket_lines(
                array(&geometry["type_result"]["center_coords"]),
                array(&geometry["type_result"]["dimensions"]),
                camera,
                geometry["passes"][0]["segments"][0]["packed_color"]
                    .as_u64()
                    .unwrap() as u16,
            );
            let mut parent = PlannedObjectInstance::building(
                ObjectDraw {
                    id: 1,
                    layer: TacticalLayer(2),
                    display_order: 0,
                    policy: BlitPolicy::z_read(SpriteEncoding::Rle),
                },
                vec![],
            );
            parent.brackets = Some(lines);
            let mut layer = lower_ground_object_instances(vec![parent]);
            terrain.begin_surface_lines();
            let viewport = SurfaceLineViewport {
                camera,
                clip: array(&input["clip"]),
                z_origin_y: 0,
                zoom: 1.0,
            };
            for lines in &layer.before_lines {
                layer.line_batches.push(terrain.prepare_surface_lines(
                    &gpu.device,
                    &gpu.queue,
                    std::iter::empty(),
                    lines,
                    &[],
                    viewport,
                    || false,
                    |[x, y]| {
                        if input["alpha"] == "mixed" {
                            [0, 1, 63, 127, 255][((x / 9 + y / 7) % 5) as usize]
                        } else {
                            127
                        }
                    },
                ));
            }
            let initial = encoded(input["background"].as_u64().unwrap() as u16, format)
                .repeat((size[0] * size[1]) as usize);
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
            let mut z: Vec<u16> = (0..size[1])
                .flat_map(|y| (0..size[0]).map(move |x| (x, y)))
                .map(|(x, y)| {
                    if let Some(rect) = input["occluding_rect"].as_array() {
                        let rect: [i32; 4] =
                            std::array::from_fn(|i| rect[i].as_i64().unwrap() as i32);
                        if (rect[0]..rect[2]).contains(&(x as i32))
                            && (rect[1]..rect[3]).contains(&(y as i32))
                        {
                            return input["occluding_z"].as_u64().unwrap() as u16;
                        }
                    }
                    input["z"].as_u64().unwrap() as u16
                })
                .collect();
            if let Some(runs) = input["native_z_runs"].as_array() {
                for run in runs {
                    let [x, y, length, word] = array::<4>(run);
                    assert!(x >= 0 && y >= 0 && length > 0);
                    assert!(x + length <= size[0] as i32 && y < size[1] as i32);
                    let offset = y as usize * size[0] as usize + x as usize;
                    z[offset..offset + length as usize].fill(word as u16);
                }
            }
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            seed_depth_grid(&gpu, &mut encoder, &dv, &z, size[0]);
            draw_native_object_pass(
                &mut encoder,
                &cv,
                &dv,
                &mut terrain,
                [0, 0, size[0], size[1]],
                &batch,
                None,
                &layer,
                None,
                None,
                None,
                None,
                None,
                batch.default_zshape_bind_group(),
            );
            let reads = [
                gpu.read(&mut encoder, &color),
                gpu.read(&mut encoder, &depth),
            ];
            let output = gpu.finish(encoder, &reads, size);
            let mut expected = initial;
            for pixel in row["pixels"].as_array().unwrap() {
                let [x, y, word] = array::<3>(pixel);
                let offset = (y as usize * size[0] as usize + x as usize) * 4;
                expected[offset..offset + 4].copy_from_slice(&encoded(word as u16, format));
            }
            let differences: Vec<_> = output[0]
                .chunks_exact(4)
                .zip(expected.chunks_exact(4))
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .take(8)
                .collect();
            assert!(
                differences.is_empty(),
                "{} {format:?}: {differences:?}",
                row["name"]
            );
            for (index, actual) in output[1].chunks_exact(4).enumerate() {
                assert_eq!(
                    crate::render::native_z::stored_z(f32::from_le_bytes(
                        actual.try_into().unwrap()
                    )),
                    z[index],
                    "{} {format:?}: Z pixel{index}",
                    row["name"]
                );
            }
        }
    }
}
