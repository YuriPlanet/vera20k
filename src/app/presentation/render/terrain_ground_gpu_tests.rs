//! Production Ground lowering/replay regression with synthetic atlas pages.
//! Native row/leaf goldens are checked separately; this gate checks caller
//! ordering, fresh destination snapshots and real pipeline read/write policies.
use super::super::draw_plan_lowering::{
    GroundPieceInstance, PlannedGroundObjectInstance, lower_ground_object_instances,
};
use super::*;
use crate::render::tactical_draw_plan::{BlitPolicy, ObjectDraw, SpriteEncoding, TacticalLayer};
use crate::render::terrain_draw::{TerrainDrawRenderer, TerrainPiece};
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded, extent, sprite};
use wgpu::util::DeviceExt;

#[test]
#[ignore = "requires GPU; retained native Display history through production lowering and draw replay"]
fn retained_ground_history_controls_overlapping_atlas_pixels() {
    use super::super::draw_plan_lowering::NativeGroundOrder;
    use crate::sim::world::display_layers::{DisplayLayer, DisplayLayers};

    // Execute the same relocation/sort history as the original-instruction
    // corpus. Stop after one pass: a fresh full sort has a different overlap.
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../../tools/spatial_oracle/crate_ground_membership.json"
    ))
    .unwrap();
    let row = rows
        .iter()
        .find(|row| row["input"]["name"] == "one_adjacent_sort_pass_per_call")
        .unwrap();
    let mut display = DisplayLayers::default();
    let mut keys: Vec<i32> = row["input"]["actors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|actor| 5376 + actor["delta"][0].as_i64().unwrap() as i32)
        .collect();
    for (step, observed) in row["input"]["steps"]
        .as_array()
        .unwrap()
        .iter()
        .zip(row["after_steps"].as_array().unwrap())
    {
        let id = step["actor"].as_u64().map(|index| index + 1);
        match step["op"].as_str().unwrap() {
            "submit" => {
                display.submit(id.unwrap(), Some(DisplayLayer::GROUND), &|id| {
                    keys[id as usize - 1]
                });
            }
            "coordinates" => {
                keys[id.unwrap() as usize - 1] =
                    (step["xyz"][0].as_i64().unwrap() + step["xyz"][1].as_i64().unwrap()) as i32
            }
            "sort" => {
                display.sort_ground_pass(&|id| keys[id as usize - 1]);
                let expected: Vec<_> = observed["layers"][2]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| id.as_u64().unwrap() + 1)
                    .collect();
                assert_eq!(display.members(DisplayLayer::GROUND), expected);
                break;
            }
            op => panic!("unexpected {op}"),
        }
    }
    // Save/restore must not replace retained history with freshly sorted keys.
    let display: DisplayLayers =
        bincode::deserialize(&bincode::serialize(&display).unwrap()).unwrap();
    let order = NativeGroundOrder::new(display.members(DisplayLayer::GROUND));
    let gpu = Gpu::new();
    let size = [4, 2];
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera(size));
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, format, &batch);
    terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
    let shp = SpriteAtlas::from_test_pages(
        [[0, 252, 0, 255], [248, 0, 0, 255], [0, 0, 248, 255]]
            .into_iter()
            .map(|rgba| crate::render::sprite_atlas::SpriteAtlasPage {
                texture: batch.create_texture_on_device(
                    &gpu.device,
                    &gpu.queue,
                    &rgba,
                    1,
                    1,
                    Some(&[1]),
                ),
            })
            .collect(),
    );
    let ground = lower_ground_object_instances(
        (1..=4)
            .rev()
            .map(|id| {
                let (x, page) = match id {
                    1 => (0., 0),
                    2 => (2., 1),
                    3 => (2., 2),
                    4 => (0., 1),
                    _ => unreachable!(),
                };
                PlannedGroundObjectInstance::object(
                    order.object_draw(id, SpriteEncoding::Plain).unwrap(),
                    vec![GroundPieceInstance {
                        target: GroundTexture::ShpPage(page),
                        render_z: RenderZPolicy::None,
                        instance: sprite([x, 0.], [2., 2.], 0.),
                    }],
                )
            })
            .collect(),
    );
    assert_eq!(ground.owners, [2, 3, 4, 1]);
    let mut pool = InstanceBufferPool::new();
    pool.upload_on_device(&gpu.device, &gpu.queue, "ground_objects", &ground.instances);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    clear(
        &mut encoder,
        &cv,
        &dv,
        65535,
        wgpu::LoadOp::Clear(wgpu::Color::WHITE),
    );
    draw_native_ground_object_pass(
        &mut encoder,
        &cv,
        &dv,
        &mut terrain,
        [0, 0, 4, 2],
        &batch,
        &pool,
        &ground,
        None,
        None,
        &VxlSlopeTransitionCache::default(),
        Some(&shp),
        None,
        batch.default_zshape_bind_group(),
    );
    let reads = [gpu.read(&mut encoder, &color)];
    let output = gpu.finish(encoder, &reads, size);
    assert_eq!(
        &output[0][0..4],
        encoded(0x07e0, format),
        "last retained left sprite"
    );
    assert_eq!(
        &output[0][8..12],
        encoded(0x001f, format),
        "retained order draws blue; full Y-sort would draw red"
    );
}

#[test]
#[ignore = "requires GPU; actual Ground lowering, atlas upload and voxel body/shadow replay"]
fn vehicle_shadow_after_body_preserves_body_and_clipped_read_only_depth() {
    let gpu = Gpu::new();
    let size = [12, 8];
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera(size));
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, format, &batch);
    terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
    let mut mask = vec![1; 24];
    for y in 0..2 {
        for x in 1..5 {
            mask[y * 6 + x] = 0;
        }
    }
    let units = UnitAtlas::from_test_pages(vec![
        crate::render::unit_atlas::UnitAtlasPage {
            texture: batch.create_unit_atlas_texture_on_device(
                &gpu.device,
                &gpu.queue,
                1,
                1,
                &[33],
            ),
        },
        crate::render::unit_atlas::UnitAtlasPage {
            texture: batch.create_unit_atlas_texture_on_device(
                &gpu.device,
                &gpu.queue,
                6,
                4,
                &mask,
            ),
        },
    ]);
    let palette = crate::assets::pal_file::Palette {
        colors: [crate::assets::pal_file::Color::rgb(0, 252, 0); 256],
    };
    let ramps = crate::rules::house_colors::HouseColorRamps::from_schemes(&[]);
    let palettes = PaletteSet::new_on_device(&gpu.device, &gpu.queue, &palette, &ramps, &[]);
    let parents = (0..2)
        .map(|i| {
            let mut body = sprite([1. + i as f32 * 4., 1.], [4., 4.], -1.);
            body.z_gradient = crate::render::native_z::pack_voxel_z_gradient(
                crate::render::native_z::ZGradient::Vertical,
                false,
            );
            body.zshape_origin = [1., 4.];
            let mut shadow = sprite([i as f32 * 4., 3.], [6., 4.], -1.);
            shadow.z_gradient = body.z_gradient;
            shadow.zshape_origin = [3., 4.];
            shadow.draw_state.fx_flags = crate::render::draw_state::FX_SHADOW;
            PlannedGroundObjectInstance::object(
                ObjectDraw {
                    id: i,
                    layer: TacticalLayer(2),
                    display_order: i,
                    policy: BlitPolicy::z_read(SpriteEncoding::Plain),
                },
                vec![
                    GroundPieceInstance {
                        target: GroundTexture::UnitAtlasPage(0),
                        render_z: RenderZPolicy::ReadOnly,
                        instance: body,
                    },
                    GroundPieceInstance {
                        target: GroundTexture::UnitAtlasPage(1),
                        render_z: RenderZPolicy::ReadOnly,
                        instance: shadow,
                    },
                ],
            )
        })
        .collect();
    let ground = lower_ground_object_instances(parents);
    assert_eq!(ground.owners, vec![0, 0, 1, 1]);
    assert_eq!(
        ground
            .instances
            .iter()
            .map(|s| s.draw_state.fx_flags & crate::render::draw_state::FX_SHADOW != 0)
            .collect::<Vec<_>>(),
        vec![false, true, false, true]
    );
    let mut pool = InstanceBufferPool::new();
    pool.upload_on_device(&gpu.device, &gpu.queue, "ground_objects", &ground.instances);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    clear(
        &mut encoder,
        &cv,
        &dv,
        65535,
        wgpu::LoadOp::Clear(wgpu::Color::WHITE),
    );
    let stats = draw_native_ground_object_pass(
        &mut encoder,
        &cv,
        &dv,
        &mut terrain,
        [2, 0, 8, 6],
        &batch,
        &pool,
        &ground,
        None,
        Some(&units),
        &VxlSlopeTransitionCache::default(),
        None,
        Some(&palettes),
        batch.default_zshape_bind_group(),
    );
    assert_eq!(
        stats.pieces, 0,
        "ordinary vehicle shadows add no destination-edit passes"
    );
    let reads = [
        gpu.read(&mut encoder, &color),
        gpu.read(&mut encoder, &depth),
    ];
    let output = gpu.finish(encoder, &reads, size);
    let pixel = |x: usize, y: usize| &output[0][(y * 12 + x) * 4..(y * 12 + x + 1) * 4];
    assert_eq!(
        pixel(5, 3),
        encoded(0x07e0, format),
        "later body covers earlier shadow"
    );
    assert_eq!(
        pixel(3, 2),
        encoded(0x07e0, format),
        "own body remains opaque"
    );
    assert_eq!(pixel(1, 5), [255; 4], "left tactical clip");
    assert_eq!(pixel(6, 6), [255; 4], "bottom tactical clip");
    assert!(
        pixel(2, 5)[1] < 200 && pixel(2, 5)[1] > 100,
        "one alpha shadow visibly darkens"
    );
    assert!(
        pixel(4, 5)[1] < pixel(2, 5)[1],
        "overlapping read-only shadows compound in order"
    );
    for p in output[1].chunks_exact(4) {
        assert_eq!(
            crate::render::native_z::stored_z(f32::from_le_bytes(p.try_into().unwrap())),
            65535
        );
    }
}

#[test]
#[ignore = "requires GPU; actual Ground lowering, pool upload and replay"]
fn tree_transactions_preserve_tmp_shp_voxel_overlap_and_coalesced_order() {
    let gpu = Gpu::new();
    let size = [96, 8];
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera(size));
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, format, &batch);
    terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
    let rgba = |rgb: [u8; 3]| {
        batch.create_texture_on_device(
            &gpu.device,
            &gpu.queue,
            &[rgb[0], rgb[1], rgb[2], 255],
            1,
            1,
            Some(&[1]),
        )
    };
    let overlay = OverlayAtlas::from_test_texture(rgba([80, 180, 80])); // plain1 -> native55aa
    let shp = SpriteAtlas::from_test_pages(vec![
        crate::render::sprite_atlas::SpriteAtlasPage {
            texture: rgba([248, 0, 0]),
        },
        crate::render::sprite_atlas::SpriteAtlasPage {
            texture: rgba([0, 0, 248]),
        },
    ]);
    let units = UnitAtlas::from_test_pages(vec![crate::render::unit_atlas::UnitAtlasPage {
        texture: batch.create_unit_atlas_texture_on_device(&gpu.device, &gpu.queue, 1, 1, &[33]),
    }]);
    let palette = crate::assets::pal_file::Palette {
        colors: [crate::assets::pal_file::Color::rgb(0, 252, 0); 256],
    };
    let ramps = crate::rules::house_colors::HouseColorRamps::from_schemes(&[]);
    let palettes = PaletteSet::new_on_device(&gpu.device, &gpu.queue, &palette, &ramps, &[]);
    let white = rgba([248, 252, 248]);
    let ztex = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("TMP zero depth source"),
            size: extent([1, 1]),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &[0],
    );
    let zview = ztex.create_view(&Default::default());
    let tmp_group = batch.create_zdepth_bind_group_on_device(&gpu.device, &white.view, &zview);
    let tmp = sprite([0.0, 4.0], [96.0, 1.0], 4.0); // actual TMP Z32768
    let tmp_buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("TMP actual ABI"),
            contents: bytemuck::bytes_of(&tmp),
            usage: wgpu::BufferUsages::VERTEX,
        });
    let steps = [
        (
            GroundTexture::TerrainStatic(TerrainPiece::Body),
            RenderZPolicy::ReadWrite,
            32767,
        ),
        (GroundTexture::ShpPage(0), RenderZPolicy::ReadOnly, 32766),
        (
            GroundTexture::TerrainStatic(TerrainPiece::Shadow),
            RenderZPolicy::ReadWrite,
            32766,
        ),
        (GroundTexture::ShpPage(1), RenderZPolicy::ReadWrite, 32765),
        (
            GroundTexture::TerrainStatic(TerrainPiece::Shadow),
            RenderZPolicy::ReadWrite,
            32765,
        ), // equality rejects
        (
            GroundTexture::UnitAtlasPage(0),
            RenderZPolicy::ReadOnly,
            32764,
        ),
        (
            GroundTexture::TerrainStatic(TerrainPiece::Shadow),
            RenderZPolicy::ReadWrite,
            32764,
        ),
        (
            GroundTexture::TerrainStatic(TerrainPiece::Shadow),
            RenderZPolicy::ReadWrite,
            -1,
        ),
        (
            GroundTexture::TerrainStatic(TerrainPiece::Shadow),
            RenderZPolicy::ReadWrite,
            -1,
        ),
    ];
    // Every prefix is replayed through the actual lowering/dispatch owner. This
    // observes intermediate writes, not just a final color that could mask an
    // omitted pass. Equal parent sort keys must retain registration order.
    let expected = [
        (0x55aa, 32767),
        (0xf800, 32767),
        (0x7800, 32766),
        (0x001f, 32765),
        (0x001f, 32765),
        (0x07e0, 32765),
        (0x03e0, 32764),
        (0x01e0, 65535),
        (0x00e0, 65535),
    ];
    let cache = VxlSlopeTransitionCache::default();
    let mut pool = InstanceBufferPool::new();
    for count in 1..=steps.len() {
        let entries = steps[..count]
            .iter()
            .enumerate()
            .flat_map(|(i, &(target, render_z, z))| {
                (0..3).map(move |lane| {
                    // Three disjoint tile lanes exercise real multi-member waves.
                    // Every ordinary run still separates the next native event.
                    let id = i * 3 + lane;
                    PlannedGroundObjectInstance::object(
                        ObjectDraw {
                            id: id as u64,
                            layer: TacticalLayer(2),
                            display_order: id as u64,
                            policy: BlitPolicy::z_read(SpriteEncoding::Plain),
                        },
                        vec![GroundPieceInstance {
                            target,
                            render_z,
                            instance: sprite(
                                [(lane * 32) as f32, 4.0],
                                [12.0, 1.0],
                                (z - 32764) as f32,
                            ),
                        }],
                    )
                })
            })
            .collect();
        let ground = lower_ground_object_instances(entries);
        assert_eq!(ground.owners, (0..(count * 3) as u64).collect::<Vec<_>>());
        if count == steps.len() {
            assert_eq!(ground.runs.last().unwrap().count, 9);
        }
        pool.upload_on_device(&gpu.device, &gpu.queue, "ground_objects", &ground.instances);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(
            &mut encoder,
            &cv,
            &dv,
            65535,
            wgpu::LoadOp::Clear(wgpu::Color::WHITE),
        );
        {
            let mut pass = crate::app::presentation::sidebar_render::begin_main_load_pass(
                &mut encoder,
                &cv,
                &dv,
            );
            batch.draw_with_buffer_zdepth(&mut pass, &tmp_group, &tmp_buffer, 1);
        }
        // Clip both edges; every restarted normal pass must restore this clip.
        let stats = draw_native_ground_object_pass(
            &mut encoder,
            &cv,
            &dv,
            &mut terrain,
            [1, 0, 74, 8],
            &batch,
            &pool,
            &ground,
            Some(&overlay),
            Some(&units),
            &cache,
            Some(&shp),
            Some(&palettes),
            batch.default_zshape_bind_group(),
        );
        let tree_steps = steps[..count]
            .iter()
            .filter(|step| matches!(step.0, GroundTexture::TerrainStatic(_)))
            .count();
        assert_eq!(stats.pieces, tree_steps * 3);
        assert_eq!(
            stats.waves, tree_steps,
            "each native step has three disjoint members, with ordinary fences and overlapping later steps"
        );
        let reads = [
            gpu.read(&mut encoder, &color),
            gpu.read(&mut encoder, &depth),
        ];
        let output = gpu.finish(encoder, &reads, size);
        for y in 0..8usize {
            for x in 0..96usize {
                let painted = (1..75).contains(&x)
                    && (0..3).any(|lane| (lane * 32..lane * 32 + 12).contains(&x));
                let (word, z) = if y == 4 && painted {
                    expected[count - 1]
                } else if y == 4 {
                    (0xffff, 32768)
                } else {
                    (0xffff, 65535)
                };
                let p = (y * 96 + x) * 4;
                assert_eq!(
                    output[0][p..p + 4],
                    encoded(word, format),
                    "prefix{count},({x},{y})"
                );
                assert_eq!(
                    crate::render::native_z::stored_z(f32::from_le_bytes(
                        output[1][p..p + 4].try_into().unwrap()
                    )),
                    z,
                    "prefix{count},({x},{y})"
                );
            }
        }
    }
}

#[test]
#[ignore = "requires GPU; actual20k ordinary UnitAtlas range remains outside TREE planner"]
fn tree_batching_ground_fences_do_not_plan_ordinary_unit_instances() {
    let gpu = Gpu::new();
    let size = [96, 8];
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera(size));
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, format, &batch);
    terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
    let overlay = OverlayAtlas::from_test_texture(batch.create_texture_on_device(
        &gpu.device,
        &gpu.queue,
        &[80, 180, 80, 255],
        1,
        1,
        Some(&[1]),
    ));
    let units = UnitAtlas::from_test_pages(vec![crate::render::unit_atlas::UnitAtlasPage {
        texture: batch.create_unit_atlas_texture_on_device(&gpu.device, &gpu.queue, 1, 1, &[0]),
    }]);
    let palette = crate::assets::pal_file::Palette {
        colors: [crate::assets::pal_file::Color::rgb(0, 252, 0); 256],
    };
    let ramps = crate::rules::house_colors::HouseColorRamps::from_schemes(&[]);
    let palettes = PaletteSet::new_on_device(&gpu.device, &gpu.queue, &palette, &ramps, &[]);
    let cache = VxlSlopeTransitionCache::default();
    let mut pool = InstanceBufferPool::new();
    for ordinary_count in [1usize, 20_000] {
        let entries = (0..ordinary_count + 2)
            .map(|i| {
                let (target, instance) = if i == 0 {
                    (
                        GroundTexture::TerrainStatic(TerrainPiece::Body),
                        sprite([0., 4.], [12., 1.], 3.),
                    )
                } else if i == ordinary_count + 1 {
                    (
                        GroundTexture::TerrainStatic(TerrainPiece::Shadow),
                        sprite([64., 4.], [12., 1.], 3.),
                    )
                } else {
                    // Real UnitAtlas draw range, deliberately outside this tiny
                    // attachment. This is a planner-work/fence gate, not20k FPS.
                    (
                        GroundTexture::UnitAtlasPage(0),
                        sprite([200., 200.], [1., 1.], 0.),
                    )
                };
                PlannedGroundObjectInstance::object(
                    ObjectDraw {
                        id: i as u64,
                        layer: TacticalLayer(2),
                        display_order: i as u64,
                        policy: BlitPolicy::z_read(SpriteEncoding::Plain),
                    },
                    vec![GroundPieceInstance {
                        target,
                        render_z: RenderZPolicy::ReadWrite,
                        instance,
                    }],
                )
            })
            .collect();
        let ground = lower_ground_object_instances(entries);
        assert_eq!(ground.runs.len(), 3);
        assert_eq!(ground.runs[1].count, ordinary_count as u32);
        pool.upload_on_device(&gpu.device, &gpu.queue, "ground_objects", &ground.instances);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(
            &mut encoder,
            &cv,
            &dv,
            65535,
            wgpu::LoadOp::Clear(wgpu::Color::WHITE),
        );
        let start = std::time::Instant::now();
        let stats = draw_native_ground_object_pass(
            &mut encoder,
            &cv,
            &dv,
            &mut terrain,
            [0, 0, 96, 8],
            &batch,
            &pool,
            &ground,
            Some(&overlay),
            Some(&units),
            &cache,
            None,
            Some(&palettes),
            batch.default_zshape_bind_group(),
        );
        let encode_us = start.elapsed().as_secs_f64() * 1e6;
        assert_eq!(
            stats,
            crate::render::terrain_draw::TerrainBatchStats {
                pieces: 2,
                waves: 2,
                tile_dependencies: 2
            }
        );
        let reads = [
            gpu.read(&mut encoder, &color),
            gpu.read(&mut encoder, &depth),
        ];
        let output = gpu.finish(encoder, &reads, size);
        for y in 0..8 {
            for x in 0..96 {
                let (word, z) = if y == 4 && x < 12 {
                    (0x55aa, 32767)
                } else if y == 4 && (64..76).contains(&x) {
                    (0x7bef, 32767)
                } else {
                    (0xffff, 65535)
                };
                let p = (y * 96 + x) * 4;
                assert_eq!(&output[0][p..p + 4], &encoded(word, format));
                assert_eq!(
                    crate::render::native_z::stored_z(f32::from_le_bytes(
                        output[1][p..p + 4].try_into().unwrap()
                    )),
                    z
                );
            }
        }
        eprintln!(
            "Ground planner gate: {ordinary_count} offscreen UnitAtlas instances, 2 TREE pieces, 2 dependency waves, 2 tile dependencies; one encoding sample {encode_us:.3}us (not FPS evidence)"
        );
    }
}
