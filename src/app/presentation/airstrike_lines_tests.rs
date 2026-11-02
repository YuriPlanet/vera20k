use super::*;
use crate::sim::rng::MainRng;
use crate::util::save_codec;
use serde_json::Value;

fn native() -> &'static Value {
    crate::test_fixture::json("tools/procedural_drawing_oracle/airstrike_beam.json")
}
fn point<const N: usize>(v: &Value) -> [i32; N] {
    std::array::from_fn(|i| v[i].as_i64().unwrap() as i32)
}
#[cfg(not(feature = "phobos"))]
fn coord(v: &Value) -> DriveCoord {
    let [x, y, z] = point(v);
    DriveCoord { x, y, z }
}
fn viewport(v: &Value) -> SurfaceLineViewport {
    let mut camera = point(&v["camera"]);
    camera[1] += 15;
    SurfaceLineViewport {
        camera,
        clip: point(&v["clip"]),
        z_origin_y: 0,
        zoom: 1.,
    }
}
fn style() -> FlareStyle {
    FlareStyle {
        #[cfg(feature = "phobos")]
        color: [255, 0, 0],
        #[cfg(feature = "phobos")]
        z_adjust: 0,
    }
}
#[cfg(not(feature = "phobos"))]
fn draws(row: &Value) -> Vec<SurfaceDraw> {
    let input = &row["input"];
    let view = viewport(input);
    let main = MainRng::new(input["seed"].as_u64().unwrap());
    let opaque = |key: &str| {
        let Some(value) = input.get(key) else {
            return Vec::new();
        };
        let a: [i32; 2] = point(&value[0]);
        let b: [i32; 2] = point(&value[1]);
        let rgb = crate::render::native_surface_format::RGB565
            .unpack_rgb8(value[2].as_u64().unwrap() as u16);
        let rgba = crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION
            .quantize_rgba8([rgb[0], rgb[1], rgb[2], 255]);
        let mut sprites = vec![crate::render::batch::SpriteInstance {
            position: [
                (a[0].min(b[0]) + view.camera[0]) as f32,
                (a[1].min(b[1]) + view.camera[1]) as f32,
            ],
            size: [
                ((a[0] - b[0]).abs() + 1) as f32,
                ((a[1] - b[1]).abs() + 1) as f32,
            ],
            tint: [rgba[0], rgba[1], rgba[2]].map(|v| f32::from(v) / 255.),
            alpha: 1.,
            ..Default::default()
        }];
        let mut calls = Vec::new();
        drain_opaque(&mut calls, &mut sprites, view.camera);
        calls
            .into_iter()
            .map(|call| match call {
                TacticalLineCall::Opaque { rect, word } => SurfaceDraw::OpaqueRect { rect, word },
                _ => unreachable!(),
            })
            .collect::<Vec<_>>()
    };
    let mut result = opaque("opaque_before");
    result.extend(geometry(
        coord(&input["source"]),
        coord(&input["target"]),
        main.draws().ranged(190, 270),
        style(),
        view,
    ));
    result.extend(opaque("opaque_after"));
    result
}

#[cfg(not(feature = "phobos"))]
#[test]
fn original_beam_commands_match_color_boundaries_offsets_strength_and_cumulative_depth() {
    assert_eq!(native()["cases"].as_array().unwrap().len(), 18);
    for row in native()["cases"].as_array().unwrap() {
        let expected: Vec<_> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| match e["kind"].as_str().unwrap() {
                "line" => Some(SurfaceDraw::Line(SurfaceLine {
                    from: point(&e["from_xy"]),
                    to: point(&e["to_xy"]),
                    z_adjust: point(&e["z_adjust"]),
                    blend: SurfaceLineBlend::Mix {
                        rgb: std::array::from_fn(|i| e["rgb"][i].as_u64().unwrap() as u8),
                        strength: e["intensity"].as_i64().unwrap() as i32,
                    },
                })),
                "solid" => {
                    let a: [i32; 2] = point(&e["from_xy"]);
                    let b: [i32; 2] = point(&e["to_xy"]);
                    Some(SurfaceDraw::OpaqueRect {
                        rect: [
                            a[0].min(b[0]),
                            a[1].min(b[1]),
                            (a[0] - b[0]).abs() + 1,
                            (a[1] - b[1]).abs() + 1,
                        ],
                        word: e["word"].as_u64().unwrap() as u16,
                    })
                }
                _ => None,
            })
            .collect();
        assert_eq!(draws(row), expected, "{}", row["input"]);
    }
}

#[test]
fn wobble_uses_original_table_and_truncates_after_adding_the_building_coordinate() {
    let target = DriveCoord {
        x: 5504,
        y: 5248,
        z: 0,
    };
    for row in native()["tactical"].as_array().unwrap() {
        let Some(beam) = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["kind"] == "beam")
        else {
            continue;
        };
        let expected = DriveCoord {
            x: beam["coords"][3].as_i64().unwrap() as i32,
            y: beam["coords"][4].as_i64().unwrap() as i32,
            z: beam["coords"][5].as_i64().unwrap() as i32,
        };
        assert_eq!(
            wobble(target, row["input"]["wall_ms"].as_u64().unwrap_or(0) as u32),
            expected,
            "{}",
            row["input"]
        );
    }
}

#[test]
fn composite_preserves_opaque_beam_order_and_advances_only_the_shared_main_draws() {
    let main = MainRng::new(31);
    let reference = MainRng::new(31);
    let from = DriveCoord {
        x: 5248,
        y: 5248,
        z: 100,
    };
    let to = DriveCoord {
        x: 5504,
        y: 5248,
        z: 0,
    };
    let opaque = SurfaceDraw::OpaqueRect {
        rect: [25, 25, 3, 1],
        word: 0xFFFF,
    };
    let calls = [
        TacticalLineCall::Opaque {
            rect: [25, 25, 3, 1],
            word: 0xFFFF,
        },
        TacticalLineCall::Designation {
            id: 7,
            coords: [from, to],
            style: style(),
        },
        TacticalLineCall::Opaque {
            rect: [25, 25, 3, 1],
            word: 0xFFFF,
        },
    ];
    let view = viewport(&native()["cases"][0]["input"]);
    let mut shades = AirstrikeShades::default();
    for ms in [0, 128] {
        shades.step(&calls, &main.draws());
        let result = composite(&calls, ms, &shades, view);
        // Phobos's hook at 0x007058F6 resumes past the stock draw at
        // 0x00705905 and draws only its own scale.
        #[cfg(not(feature = "phobos"))]
        reference.draws().ranged(190, 270);
        #[cfg(feature = "phobos")]
        reference.draws().ranged(745, 1000);
        assert_eq!(result.first(), Some(&opaque));
        assert_eq!(result.last(), Some(&opaque));
        assert_eq!(
            save_codec::serialize(&main).unwrap(),
            save_codec::serialize(&reference).unwrap()
        );
    }
}

/// Phobos `TechnoClass_DrawAirstrikeFlare_LineColor` (`0x007058F6`) and
/// `_DotColor` (`0x0070597A`): every line takes the scaled base colour and one
/// depth, `min(zSrc, zDest) + AirstrikeLineZAdjust`, and the dot keeps the
/// unscaled word even though the target end here is the lower one, where the
/// stock draw skips it.
#[cfg(feature = "phobos")]
#[test]
fn phobos_flare_scales_the_lines_flattens_their_depth_and_always_draws_the_dot() {
    use crate::util::native_x87::adjust_for_z_standard;
    let from = DriveCoord {
        x: 5248,
        y: 5248,
        z: 100,
    };
    let to = DriveCoord {
        x: 5504,
        y: 5248,
        z: 0,
    };
    let style = FlareStyle {
        color: [12, 200, 45],
        z_adjust: 7,
    };
    let draws = geometry(
        from,
        to,
        745,
        style,
        viewport(&native()["cases"][0]["input"]),
    );
    // `(BYTE)(base * 0.745)` truncates 8.94, 149.0 and 33.525.
    let line_rgb = [8, 149, 33];
    let z = (-32 - adjust_for_z_standard(100)).min(-32 - adjust_for_z_standard(0)) + 7;
    let dot_word = crate::render::native_surface_format::RGB565.pack_rgb8(style.color);
    let (mut lines, mut dots) = (0, 0);
    for draw in &draws {
        match draw {
            SurfaceDraw::OpaqueRect { word, .. } => {
                dots += 1;
                assert_eq!(*word, dot_word);
            }
            SurfaceDraw::Line(line) => {
                lines += 1;
                assert_eq!(line.z_adjust, [z, z]);
                let SurfaceLineBlend::Mix { rgb, .. } = line.blend else {
                    panic!("the airstrike beam is a mixed-color line");
                };
                assert_eq!(rgb, line_rgb);
            }
            _ => {}
        }
    }
    assert_eq!((lines, dots), (12, 4));
}

#[cfg(not(feature = "phobos"))]
#[test]
#[ignore = "requires GPU; original Boris705860 pixels through shared4BEAC0 pipeline"]
fn production_airstrike_beam_matches_original_pixels_and_preserves_depth() {
    use crate::render::batch::BatchRenderer;
    use crate::render::terrain_draw::TerrainDrawRenderer;
    use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded};
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
        for row in native()["cases"].as_array().unwrap() {
            let input = &row["input"];
            let old_z = input["old_z"].as_u64().unwrap() as u16;
            let bytes = encoded(0x39E7, format).repeat((size[0] * size[1]) as usize);
            gpu.queue.write_texture(
                color.as_image_copy(),
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
                draws(row).into_iter(),
                &[],
                &[],
                &[],
                viewport(input),
                |_| false,
                |[x, y]| match input["alpha"].as_str().unwrap() {
                    "clear" => 127,
                    "black" => 0,
                    _ => [0, 1, 63, 127, 255][((x / 9 + y / 7) % 5) as usize],
                },
            );
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            clear(&mut encoder, &cv, &dv, old_z, wgpu::LoadOp::Load);
            terrain.draw_surface_lines(&mut encoder, &cv, prepared);
            let reads = [
                gpu.read(&mut encoder, &color),
                gpu.read(&mut encoder, &depth),
            ];
            let output = gpu.finish(encoder, &reads, size);
            let mut expected = bytes;
            for p in row["pixels"].as_array().unwrap() {
                let [x, y, word] = point(p);
                let offset = (y as usize * 160 + x as usize) * 4;
                expected[offset..offset + 4].copy_from_slice(&encoded(word as u16, format));
            }
            let diff = output[0]
                .as_chunks::<4>()
                .0
                .iter()
                .zip(expected.as_chunks::<4>().0)
                .position(|(a, b)| a != b);
            assert_eq!(diff, None, "{format:?} {}", input["name"]);
            assert_eq!(
                output[1],
                crate::render::native_z::stored_depth(old_z)
                    .to_le_bytes()
                    .repeat((size[0] * size[1]) as usize)
            );
        }
    }
}
