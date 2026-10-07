//! Original executed packed-material words through retained production replay.
use super::super::draw_plan_lowering::{
    ObjectPieceInstance, PlannedObjectInstance, lower_object_instances,
};
use super::*;
use crate::render::batch::{InstanceBufferPool, SpriteInstance};
use crate::render::draw_state::{DrawState, FX_CLOAK};
use crate::render::palette_light::PaletteLight;
use crate::render::shroud_buffer::ShroudBuffer;
use crate::render::tactical_draw_plan::{BlitPolicy, ObjectDraw, SpriteEncoding, TacticalLayer};
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded};
use serde_json::Value;

fn native_light(row: &Value) -> PaletteLight {
    let brightness = row["brightness"].as_i64().unwrap() as i32;
    match row["profile"].as_str().unwrap() {
        "plain1" => PaletteLight::plain(1, brightness),
        "plain53" => PaletteLight::plain(53, brightness),
        "light27" => PaletteLight::new([1000; 3], 27, brightness, false),
        "scheme53" => PaletteLight::color_scheme([1000; 3], brightness),
        other => panic!("unknown native palette input {other}"),
    }
}

#[test]
#[ignore = "requires GPU; original packed translucency through actual SHP/VXL replay"]
fn retained_mobile_translucency_matches_original_packed_words() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/translucent_blitter_a.json",
    ))
    .unwrap();
    let gpu = Gpu::new();
    for collection in ["cases", "replay_controls"] {
        for route in ["shp", "voxel"] {
            let mut groups = std::collections::BTreeMap::<[u8; 3], Vec<&Value>>::new();
            for row in native[collection]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["route"] == route)
            {
                let rgb = std::array::from_fn(|i| row["rgb"][i].as_u64().unwrap() as u8);
                groups.entry(rgb).or_default().push(row);
            }
            assert!(!groups.is_empty());
            for (rgb, all_cases) in groups {
                for cases in all_cases.chunks(512) {
                    let size = [8, cases.len() as u32 * 3];
                    // Native inputs contain a guarded nineteen-word destination window.
                    // An eight-word pitch and a three-row block preserve the +/-8 controls
                    // without treating a signed surface-word offset as a horizontal UV.
                    let indices: Vec<u8> = cases
                        .iter()
                        .flat_map(|row| {
                            let index = row["index"].as_u64().unwrap() as u8;
                            let middle = row
                                .get("overlap_sources")
                                .map_or(0, |source| source[1].as_u64().unwrap() as u8);
                            (0..24).map(move |word| match word {
                                10 | 12 => index,
                                11 => middle,
                                _ => 0,
                            })
                        })
                        .collect();
                    let a: Vec<u8> = cases
                        .iter()
                        .flat_map(|row| [row["a"].as_u64().unwrap() as u8; 24])
                        .collect();
                    for format in [
                        wgpu::TextureFormat::Bgra8UnormSrgb,
                        wgpu::TextureFormat::Rgba8UnormSrgb,
                    ] {
                        let mut batch =
                            BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
                        batch.write_camera(&gpu.queue, camera(size));
                        let fog =
                            ShroudBuffer::fixture(&gpu.device, &gpu.queue, size, [0.0; 2], &a);
                        batch.bind_shroud(&gpu.device, Some(&fog));
                        let rgba: Vec<u8> = indices
                            .iter()
                            .flat_map(|&index| {
                                [rgb[0], rgb[1], rgb[2], if index == 0 { 0 } else { 255 }]
                            })
                            .collect();
                        let shp = SpriteAtlas::from_test_pages(vec![
                            crate::render::sprite_atlas::SpriteAtlasPage {
                                texture: batch.create_texture_on_device(
                                    &gpu.device,
                                    &gpu.queue,
                                    &rgba,
                                    size[0],
                                    size[1],
                                    Some(&indices),
                                ),
                            },
                        ]);
                        let voxel = batch.create_unit_atlas_texture_on_device(
                            &gpu.device,
                            &gpu.queue,
                            size[0],
                            size[1],
                            &indices,
                        );
                        let palettes = PaletteSet::new_on_device(
                            &gpu.device,
                            &gpu.queue,
                            &crate::assets::pal_file::Palette {
                                colors: [crate::assets::pal_file::Color::rgb(
                                    rgb[0], rgb[1], rgb[2],
                                ); 256],
                            },
                            &crate::rules::house_colors::HouseColorRamps::from_schemes(&[]),
                            &[],
                        );
                        let parents = cases
                            .iter()
                            .enumerate()
                            .flat_map(|(y, row)| {
                                let bits = row["selector_bits"].as_u64().unwrap();
                                let opacity = match bits & 6 {
                                    2 => 0.75,
                                    4 => 0.5,
                                    6 => 0.25,
                                    _ => unreachable!(),
                                };
                                let mut parent = PlannedObjectInstance::object(
                                    ObjectDraw {
                                        id: y as u64,
                                        layer: TacticalLayer(3),
                                        display_order: y as u64,
                                        policy: BlitPolicy::z_read(SpriteEncoding::Plain),
                                    },
                                    vec![ObjectPieceInstance {
                                        target: if route == "shp" {
                                            ObjectTexture::ShpPage(0)
                                        } else {
                                            ObjectTexture::UnitPose
                                        },
                                        render_z: if row["writes_depth"] == true {
                                            RenderZPolicy::ReadWrite
                                        } else {
                                            RenderZPolicy::ReadOnly
                                        },
                                        instance: SpriteInstance {
                                            position: [2.0, (y * 3 + 1) as f32],
                                            size: [3.0, 1.0],
                                            uv_origin: [0.25, (y * 3 + 1) as f32 / size[1] as f32],
                                            uv_size: [3.0 / 8.0, 1.0 / size[1] as f32],
                                            tint: [1.0; 3],
                                            alpha: 1.0,
                                            z_adjust: (4096 - (32768 - (y * 3 + 1) as i32)) as f32,
                                            palette_light: native_light(row),
                                            draw_state: DrawState {
                                                fx_flags: FX_CLOAK,
                                                native_offset_words: row["displacement"]
                                                    .as_i64()
                                                    .unwrap()
                                                    as i32,
                                                fx_params: [opacity, bits as f32, 1.0, 0.0],
                                                ..Default::default()
                                            },
                                            ..Default::default()
                                        },
                                    }],
                                );
                                if route == "voxel" && row.get("overlap_sources").is_some() {
                                    let piece = &parent.pieces[0];
                                    parent.pieces.push(ObjectPieceInstance {
                                        target: piece.target,
                                        render_z: piece.render_z,
                                        instance: piece.instance,
                                    });
                                }
                                let replays = row["replay_count"].as_u64().unwrap_or(1) as usize;
                                (0..replays)
                                    .map(|repeat| {
                                        PlannedObjectInstance::object(
                                            ObjectDraw {
                                                id: (y * 4 + repeat) as u64,
                                                layer: parent.parent.layer,
                                                display_order: (y * 4 + repeat) as u64,
                                                policy: parent.parent.policy,
                                            },
                                            parent
                                                .pieces
                                                .iter()
                                                .map(|piece| ObjectPieceInstance {
                                                    target: piece.target,
                                                    render_z: piece.render_z,
                                                    instance: piece.instance,
                                                })
                                                .collect(),
                                        )
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .collect();
                        let layers = lower_object_instances(parents);
                        let layer = &layers[3];
                        let mut pool = InstanceBufferPool::new();
                        pool.upload_on_device(
                            &gpu.device,
                            &gpu.queue,
                            "packed_native",
                            &layer.instances,
                        );
                        let white = batch.create_texture_on_device(
                            &gpu.device,
                            &gpu.queue,
                            &[255; 4],
                            1,
                            1,
                            Some(&[1]),
                        );
                        let seeds: Vec<_> = cases
                            .iter()
                            .enumerate()
                            .flat_map(|(y, row)| {
                                row["prior_depths"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, depth)| depth.as_u64().unwrap() < 65535)
                                    .map(move |(x, depth)| SpriteInstance {
                                        position: [(2 + x) as f32, (y * 3 + 1) as f32],
                                        size: [1.0; 2],
                                        uv_size: [1.0; 2],
                                        tint: [1.0; 3],
                                        alpha: 1.0,
                                        z_adjust: (depth.as_u64().unwrap() as i32
                                            - (32768 - (y * 3 + 1) as i32))
                                            as f32,
                                        palette_light: PaletteLight::plain(1, 1000),
                                        ..Default::default()
                                    })
                            })
                            .collect();
                        let mut raw = Vec::new();
                        for (y, row) in cases.iter().enumerate() {
                            let word = row["raw"].as_u64().unwrap() as u16;
                            let seed = |position, size, word| {
                                let rgb = encoded(word, wgpu::TextureFormat::Rgba8UnormSrgb);
                                SpriteInstance {
                                    position,
                                    size,
                                    uv_size: [1.0; 2],
                                    tint: std::array::from_fn(|i| f32::from(rgb[i]) / 255.0),
                                    alpha: 1.0,
                                    ..Default::default()
                                }
                            };
                            raw.push(seed([0.0, (y * 3) as f32], [8.0, 3.0], word));
                            let displacement = row["displacement"].as_i64().unwrap() as i32;
                            if displacement != 0 {
                                let guard = 10 + displacement;
                                raw.push(seed(
                                    [(guard % 8) as f32, (y * 3) as f32 + (guard / 8) as f32],
                                    [1.0; 2],
                                    word ^ 0x07e0,
                                ));
                            }
                        }
                        pool.upload_on_device(&gpu.device, &gpu.queue, "depth_seeds", &seeds);
                        pool.upload_on_device(&gpu.device, &gpu.queue, "raw_seeds", &raw);
                        let color = gpu.target(size, format);
                        let cv = color.create_view(&Default::default());
                        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
                        let dv = depth.create_view(&Default::default());
                        let mut terrain = crate::render::terrain_draw::TerrainDrawRenderer::new(
                            &gpu.device,
                            &gpu.queue,
                            format,
                            &batch,
                        );
                        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
                        let mut encoder = gpu.device.create_command_encoder(&Default::default());
                        clear(
                            &mut encoder,
                            &cv,
                            &dv,
                            65535,
                            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        );
                        {
                            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: &cv,
                                    resolve_target: None,
                                    depth_slice: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Load,
                                        store: wgpu::StoreOp::Store,
                                    },
                                })],
                                depth_stencil_attachment: Some(
                                    wgpu::RenderPassDepthStencilAttachment {
                                        view: &dv,
                                        depth_ops: Some(wgpu::Operations {
                                            load: wgpu::LoadOp::Load,
                                            store: wgpu::StoreOp::Store,
                                        }),
                                        stencil_ops: None,
                                    },
                                ),
                                ..Default::default()
                            });
                            let (seed, count) = pool.get("depth_seeds").unwrap();
                            batch.draw_zsprite_range(
                                &mut pass,
                                &white,
                                batch.default_zshape_bind_group(),
                                seed,
                                0,
                                count,
                                true,
                            );
                            let (raw, count) = pool.get("raw_seeds").unwrap();
                            batch.draw_with_buffer_passthrough(&mut pass, &white, raw, count);
                        }
                        draw_native_object_pass(
                            &mut encoder,
                            &cv,
                            &dv,
                            &mut terrain,
                            [0, 0, size[0], size[1]],
                            &batch,
                            pool.get("packed_native"),
                            layer,
                            None,
                            None,
                            Some(&voxel),
                            &VxlSlopeTransitionCache::default(),
                            Some(&shp),
                            Some(&palettes),
                            batch.default_zshape_bind_group(),
                        );
                        let reads = [
                            gpu.read(&mut encoder, &color),
                            gpu.read(&mut encoder, &depth),
                        ];
                        let output = gpu.finish(encoder, &reads, size);
                        for (y, row) in cases.iter().enumerate() {
                            for x in 0..3 {
                                let offset = ((y * 3 + 1) * 8 + 2 + x) * 4;
                                assert_eq!(
                                    &output[0][offset..offset + 4],
                                    &encoded(row["colors"][x].as_u64().unwrap() as u16, format),
                                    "{collection} {route} {format:?} row{y} pixel{x} {row}"
                                );
                                assert_eq!(
                                    crate::render::native_z::stored_z(f32::from_le_bytes(
                                        output[1][offset..offset + 4].try_into().unwrap()
                                    )),
                                    row["depths"][x].as_u64().unwrap() as u16,
                                    "{route} row{y} pixel{x} depth"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "requires GPU; distant packed parents at a supported 4K viewport"]
fn retained_packed_high_resolution_dispatch_preserves_native_pixels() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/procedural_drawing_oracle/translucent_blitter_a.json",
    ))
    .unwrap();
    let row = native["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["route"] == "shp"
                && row["profile"] == "plain1"
                && row["selector_bits"] == 4
                && row["a"] == 0
                && row["raw"] == 0
                && row["brightness"] == 1000
                && row["writes_depth"] == false
        })
        .unwrap();
    let rgb: [u8; 3] = std::array::from_fn(|i| row["rgb"][i].as_u64().unwrap() as u8);
    let gpu = Gpu::new();
    let size = [3840, 2160];
    // Two distant objects, rather than a large sprite, exercise the actual
    // production batching union that exceeded Limits::default() on one axis.
    let positions = [[0.0, 0.0], [3839.0, 2159.0]];
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        batch.write_camera(&gpu.queue, camera(size));
        let fog = ShroudBuffer::fixture(
            &gpu.device,
            &gpu.queue,
            size,
            [0.0; 2],
            &vec![0; (size[0] * size[1]) as usize],
        );
        batch.bind_shroud(&gpu.device, Some(&fog));
        let shp =
            SpriteAtlas::from_test_pages(vec![crate::render::sprite_atlas::SpriteAtlasPage {
                texture: batch.create_texture_on_device(
                    &gpu.device,
                    &gpu.queue,
                    &[rgb[0], rgb[1], rgb[2], 255],
                    1,
                    1,
                    Some(&[row["index"].as_u64().unwrap() as u8]),
                ),
            }]);
        let layers = lower_object_instances(
            positions
                .iter()
                .enumerate()
                .map(|(i, &position)| {
                    PlannedObjectInstance::object(
                        ObjectDraw {
                            id: i as u64,
                            layer: TacticalLayer(3),
                            display_order: i as u64,
                            policy: BlitPolicy::z_read(SpriteEncoding::Plain),
                        },
                        vec![ObjectPieceInstance {
                            target: ObjectTexture::ShpPage(0),
                            render_z: RenderZPolicy::ReadOnly,
                            instance: SpriteInstance {
                                position,
                                size: [1.0; 2],
                                uv_size: [1.0; 2],
                                tint: [1.0; 3],
                                alpha: 1.0,
                                palette_light: native_light(row),
                                draw_state: DrawState {
                                    fx_flags: FX_CLOAK,
                                    fx_params: [0.5, 4.0, 1.0, 0.0],
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        }],
                    )
                })
                .collect(),
        );
        let mut pool = InstanceBufferPool::new();
        pool.upload_on_device(&gpu.device, &gpu.queue, "packed_4k", &layers[3].instances);
        let color = gpu.target(size, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        let mut terrain = crate::render::terrain_draw::TerrainDrawRenderer::new(
            &gpu.device,
            &gpu.queue,
            format,
            &batch,
        );
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(
            &mut encoder,
            &cv,
            &dv,
            65535,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        let stats = draw_native_object_pass(
            &mut encoder,
            &cv,
            &dv,
            &mut terrain,
            [0, 0, size[0], size[1]],
            &batch,
            pool.get("packed_4k"),
            &layers[3],
            None,
            None,
            None,
            &VxlSlopeTransitionCache::default(),
            Some(&shp),
            None,
            batch.default_zshape_bind_group(),
        );
        let read = gpu.read(&mut encoder, &color);
        let output = gpu.finish(encoder, &[read], size);
        assert_eq!(stats.pieces, 2);
        assert_eq!(stats.waves, 1);
        let expected = encoded(row["colors"][0].as_u64().unwrap() as u16, format);
        for [x, y] in positions {
            let index = ((y as usize * size[0] as usize) + x as usize) * 4;
            assert_eq!(&output[0][index..index + 4], &expected);
        }
        let middle = ((size[1] / 2 * size[0] + size[0] / 2) * 4) as usize;
        assert_eq!(&output[0][middle..middle + 4], &[0, 0, 0, 255]);
    }
}

/// Production replay cost, including continuation submissions. This reports
/// encode and completed wall time, not ordinary gameplay FPS or GPU timestamps.
#[test]
#[ignore = "requires GPU; packed replay workload at 1,000 and 20,000 parents"]
fn retained_packed_workload_timing() {
    let gpu = Gpu::new();
    let size = [1280, 720];
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera(size));
    let fog = ShroudBuffer::fixture(
        &gpu.device,
        &gpu.queue,
        size,
        [0.0; 2],
        &vec![0; (size[0] * size[1]) as usize],
    );
    batch.bind_shroud(&gpu.device, Some(&fog));
    let shp = SpriteAtlas::from_test_pages(vec![crate::render::sprite_atlas::SpriteAtlasPage {
        texture: batch.create_texture_on_device(
            &gpu.device,
            &gpu.queue,
            &[200, 100, 50, 255],
            1,
            1,
            Some(&[1]),
        ),
    }]);
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    let mut terrain = crate::render::terrain_draw::TerrainDrawRenderer::new(
        &gpu.device,
        &gpu.queue,
        format,
        &batch,
    );
    for count in [1_000u32, 20_000] {
        let parents = (0..count)
            .map(|i| {
                PlannedObjectInstance::object(
                    ObjectDraw {
                        id: u64::from(i),
                        layer: TacticalLayer(3),
                        display_order: u64::from(i),
                        policy: BlitPolicy::z_read(SpriteEncoding::Plain),
                    },
                    vec![ObjectPieceInstance {
                        target: ObjectTexture::ShpPage(0),
                        render_z: RenderZPolicy::ReadOnly,
                        instance: SpriteInstance {
                            position: [(i % 80 * 16) as f32, ((i / 80) % 45 * 16) as f32],
                            size: [16.0; 2],
                            uv_size: [1.0; 2],
                            tint: [1.0; 3],
                            alpha: 1.0,
                            palette_light: PaletteLight::plain(1, 1000),
                            draw_state: DrawState {
                                fx_flags: FX_CLOAK,
                                fx_params: [0.5, 4.0, 1.0, 0.0],
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                    }],
                )
            })
            .collect();
        let layers = lower_object_instances(parents);
        let layer = &layers[3];
        let mut pool = InstanceBufferPool::new();
        pool.upload_on_device(&gpu.device, &gpu.queue, "packed_workload", &layer.instances);
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        let start = std::time::Instant::now();
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(
            &mut encoder,
            &cv,
            &dv,
            65535,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        );
        let stats = draw_native_object_pass(
            &mut encoder,
            &cv,
            &dv,
            &mut terrain,
            [0, 0, size[0], size[1]],
            &batch,
            pool.get("packed_workload"),
            layer,
            None,
            None,
            None,
            &VxlSlopeTransitionCache::default(),
            Some(&shp),
            None,
            batch.default_zshape_bind_group(),
        );
        let command = encoder.finish();
        let encode_ms = start.elapsed().as_secs_f64() * 1000.0;
        let submission = gpu.queue.submit([command]);
        gpu.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(60)),
            })
            .unwrap();
        eprintln!(
            "PACKED_WORKLOAD count={count} encode_ms={encode_ms:.3} completed_wall_ms={:.3} waves={} passes={} debug_assertions={}",
            start.elapsed().as_secs_f64() * 1000.0,
            stats.waves,
            stats.passes,
            cfg!(debug_assertions),
        );
        assert_eq!(stats.pieces, count as usize);
    }
}
