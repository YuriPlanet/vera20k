//! Original Unit73BEA4 rectangle outputs through actual Ground lowering,
//! instance upload and voxel fragment clipping. Raster geometry is supplied
//! here; the physical AEGIS raster has its own original-instruction packet.
use super::*;
use crate::app::presentation::render::draw_plan_lowering::{
    ObjectPieceInstance, PlannedObjectInstance, lower_ground_object_instances,
};
use crate::render::batch::InstanceBufferPool;
use crate::render::sinking::{SinkingWaterlines, apply_waterline_clip, tests};
use crate::render::tactical_draw_plan::{BlitPolicy, ObjectDraw, SpriteEncoding, TacticalLayer};
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded, sprite};

#[test]
#[ignore = "requires GPU; original Unit waterline rectangles through production voxel replay"]
fn native_sinking_clip_survives_camera_scissor_and_ground_upload() {
    let gpu = Gpu::new();
    let size = [640, 480];
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    let color = gpu.target(size, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
    let units = UnitAtlas::from_test_pages(vec![crate::render::unit_atlas::UnitAtlasPage {
        texture: batch.create_unit_atlas_texture_on_device(&gpu.device, &gpu.queue, 1, 1, &[33]),
    }]);
    let palette = crate::assets::pal_file::Palette::from_rgb(|_| [0, 252, 0]);
    let ramps = crate::rules::house_colors::HouseColorRamps::from_schemes(&[]);
    let palettes =
        PaletteSet::new_on_device(&gpu.device, &gpu.queue, &palette, &palette, &ramps, &[]);
    let mut pool = InstanceBufferPool::new();
    let mut visits = 0;
    for row in tests::unit_rows() {
        let mut cache = SinkingWaterlines::default();
        cache.restore([(1, row["input"]["waterline"].as_i64().unwrap() as i16)]);
        let steps = row["sequence"]
            .as_array()
            .cloned()
            .unwrap_or_else(|| vec![row.clone()]);
        for step in steps {
            let input = &step["input"];
            let world_y = input["camera_world_y"].as_i64().unwrap() as f32;
            let mut view = camera(size);
            view.camera_pos[1] = world_y;
            batch.write_camera(&gpu.queue, view);
            terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
            // Fill the viewport so every pixel of the native clip rectangle
            // is exercised independently of the raster's opaque coverage.
            let mut body = sprite([0.0, world_y], [640.0, 480.0], -1.0);
            body.z_gradient = crate::render::native_z::pack_voxel_z_gradient(
                crate::render::native_z::ZGradient::Vertical,
                false,
            );
            body.zshape_origin = [world_y, 480.0];
            apply_waterline_clip(&mut body.draw_state, tests::draw_input(&mut cache, input));
            let ground = lower_ground_object_instances(vec![PlannedObjectInstance::object(
                ObjectDraw {
                    id: 1,
                    layer: TacticalLayer(2),
                    display_order: 0,
                    policy: BlitPolicy::z_read(SpriteEncoding::Plain),
                },
                vec![ObjectPieceInstance {
                    target: ObjectTexture::UnitAtlasPage(0),
                    render_z: RenderZPolicy::ReadOnly,
                    instance: body,
                }],
            )]);
            pool.upload_on_device(&gpu.device, &gpu.queue, "ground_objects", &ground.instances);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            clear(
                &mut encoder,
                &cv,
                &dv,
                65535,
                wgpu::LoadOp::Clear(wgpu::Color::WHITE),
            );
            let caller: [u32; 4] =
                std::array::from_fn(|i| input["caller_rect"][i].as_u64().unwrap() as u32);
            draw_native_object_pass(
                &mut encoder,
                crate::app::render::merge_passes::ObjectDrawPhase::Display,
                &cv,
                &dv,
                &mut terrain,
                caller,
                &batch,
                pool.get("ground_objects"),
                &ground,
                None,
                Some(&units),
                None,
                None,
                Some(&palettes),
                batch.default_zshape_bind_group(),
                None,
                None,
            );
            let reads = [
                gpu.read(&mut encoder, &color),
                gpu.read(&mut encoder, &depth),
            ];
            let output = gpu.finish(encoder, &reads, size);
            let clip: [i32; 4] =
                std::array::from_fn(|i| step["output"]["clip"][i].as_i64().unwrap() as i32);
            for y in 0..480 {
                for x in 0..640 {
                    let in_clip = x >= clip[0]
                        && x < clip[0] + clip[2]
                        && y >= clip[1]
                        && y < clip[1] + clip[3];
                    let offset = (y as usize * 640 + x as usize) * 4;
                    assert_eq!(
                        &output[0][offset..offset + 4],
                        if in_clip {
                            encoded(0x07e0, format)
                        } else {
                            [255; 4]
                        },
                        "{} visit{visits} pixel{x},{y}",
                        row["name"]
                    );
                    assert_eq!(
                        crate::render::native_z::stored_z(f32::from_le_bytes(
                            output[1][offset..offset + 4].try_into().unwrap()
                        )),
                        65535,
                        "sinking never changes terrain depth"
                    );
                }
            }
            visits += 1;
        }
    }
    assert_eq!(visits, 16);
}

/// Original physical Ship controls retain the same shared ground replay.
#[test]
#[ignore = "requires GPU and retail assets; original rolled DEST hull through pose upload and retained replay"]
fn squid_rolled_destroyer_pose_upload_and_replay_match_native_hull_pixels() {
    assert_ground_pose_upload_and_replay(
        "DEST",
        crate::render::vxl_raster::ship_pose_tests::native_ship_corpus(),
        2,
        false,
    );
}

/// Drive4AFF60 and actual GTNK body/turret/barrel pixels through the same
/// retained production pose-page replay, in both supported color formats.
#[test]
#[ignore = "requires GPU and retail assets; original rocking GTNK parts through pose upload and retained replay"]
fn drone_rocked_tank_pose_upload_and_replay_match_native_part_pixels() {
    assert_ground_pose_upload_and_replay(
        "MTNK",
        crate::render::vxl_raster::ship_pose_tests::native_drive_corpus(),
        6,
        false,
    );
}

/// Capacity regression through the same production upload/submission owner:
/// each real GTNK part comes from a nonzero page, on ordinary and packed paths.
#[test]
#[ignore = "requires GPU and retail assets; actual GTNK overflow pages through ordinary and packed replay"]
fn drone_ground_pose_overflow_pages_preserve_native_parts_on_both_replay_routes() {
    assert_ground_pose_upload_and_replay(
        "MTNK",
        crate::render::vxl_raster::ship_pose_tests::native_drive_corpus(),
        6,
        true,
    );
}

fn assert_ground_pose_upload_and_replay(
    type_id: &str,
    native: &serde_json::Value,
    expected_parts: usize,
    overflow: bool,
) {
    use crate::render::palette_light::PaletteLight;
    use crate::render::unit_atlas::{PoseTilt, VxlLayer};
    use crate::render::unit_pose_cache::VxlPoseFrameCache;
    use crate::render::vxl_raster::ship_pose_tests::native_ground_part;
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let rules = &battle.rules;
    let palette =
        crate::assets::pal_file::Palette::from_bytes(assets.get_ref("UNITTEM.PAL").unwrap())
            .unwrap();
    let global =
        crate::assets::pal_file::Palette::from_bytes(assets.get_ref("UNITSNO.PAL").unwrap())
            .unwrap();
    let gpu = Gpu::new();
    let size = [256, 256];
    let light = PaletteLight::color_scheme([1000; 3], 1000);
    let house = crate::rules::house_colors::HouseColorIndex(0);
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        batch.write_camera(&gpu.queue, camera(size));
        let fog = crate::render::shroud_buffer::ShroudBuffer::fixture(
            &gpu.device,
            &gpu.queue,
            size,
            [0.; 2],
            &vec![127; 65536],
        );
        batch.bind_shroud(&gpu.device, Some(&fog));
        let palettes = PaletteSet::new_on_device(
            &gpu.device,
            &gpu.queue,
            &palette,
            &global,
            &rules.house_color_ramps,
            &[house],
        );
        let mut cache = VxlPoseFrameCache::default();
        let mut comparisons = 0;
        for row in native["cases"].as_array().unwrap() {
            for (part_name, part_layer) in [
                ("body", VxlLayer::Body),
                ("turret", VxlLayer::Turret),
                ("barrel", VxlLayer::Barrel),
            ] {
                let Some(raster) = row
                    .get("rasters")
                    .and_then(|rasters| rasters.get(part_name))
                else {
                    continue;
                };
                cache.begin_frame();
                if overflow {
                    assert_eq!(cache.fill_test_page().page, 0);
                }
                let name = row["input"]["name"].as_str().unwrap();
                let (pose, key) = native_ground_part(type_id, row, part_layer);
                let entry = cache
                    .render(&assets, Some(rules), &key, PoseTilt::Ground(pose))
                    .unwrap();
                assert_eq!(entry.page > 0, overflow);
                cache.upload(&gpu.device, &gpu.queue, &batch);
                for manual in [false, true].into_iter().take(if overflow { 2 } else { 1 }) {
                    let rect: [i32; 6] =
                        std::array::from_fn(|i| raster["rect"][i].as_i64().unwrap() as i32);
                    // The supplied native visibility page is addressed from its own
                    // source crop; destination-relative bounds remain independent.
                    let anchor = [(rect[2] - rect[0]) as f32, (rect[3] - rect[1]) as f32];
                    let mut instance = sprite(
                        [anchor[0] + entry.offset_x, anchor[1] + entry.offset_y],
                        entry.pixel_size,
                        if manual { -40000. } else { 0. },
                    );
                    instance.uv_origin = entry.uv_origin;
                    instance.uv_size = entry.uv_size;
                    instance.palette_light = light;
                    instance.draw_state.remap_row = 1;
                    instance.z_gradient = crate::render::native_z::pack_voxel_z_gradient(
                        crate::render::native_z::ZGradient::Vertical,
                        false,
                    );
                    let layer = lower_ground_object_instances(vec![PlannedObjectInstance::object(
                        ObjectDraw {
                            id: 1,
                            layer: TacticalLayer(2),
                            display_order: 0,
                            policy: BlitPolicy::z_read(SpriteEncoding::Voxel),
                        },
                        vec![ObjectPieceInstance {
                            target: ObjectTexture::UnitPosePage(entry.page),
                            render_z: RenderZPolicy::ReadOnly,
                            instance,
                        }],
                    )]);
                    let mut pool = InstanceBufferPool::new();
                    pool.upload_on_device(
                        &gpu.device,
                        &gpu.queue,
                        "tilted-ground",
                        &layer.instances,
                    );
                    let color = gpu.target(size, format);
                    let cv = color.create_view(&Default::default());
                    let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
                    let dv = depth.create_view(&Default::default());
                    let mut terrain =
                        TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
                    terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
                    let mut encoder = gpu.device.create_command_encoder(&Default::default());
                    clear(
                        &mut encoder,
                        &cv,
                        &dv,
                        65535,
                        wgpu::LoadOp::Clear(wgpu::Color::BLUE),
                    );
                    let stats = draw_native_object_pass(
                        &mut encoder,
                        crate::app::render::merge_passes::ObjectDrawPhase::Display,
                        &cv,
                        &dv,
                        &mut terrain,
                        [0, 0, 256, 256],
                        &batch,
                        pool.get("tilted-ground"),
                        &layer,
                        None,
                        None,
                        Some(&cache),
                        None,
                        Some(&palettes),
                        batch.default_zshape_bind_group(),
                        None,
                        None,
                    );
                    assert_eq!(
                        stats.passes > 0,
                        manual,
                        "ordinary and packed pose page bindings"
                    );
                    let reads = [
                        gpu.read(&mut encoder, &color),
                        gpu.read(&mut encoder, &depth),
                    ];
                    let output = gpu.finish(encoder, &reads, size);
                    let mut indices = vec![0u8; 65536];
                    for pixel in raster["pixels"].as_array().unwrap() {
                        indices[pixel[0].as_u64().unwrap() as usize] =
                            pixel[1].as_u64().unwrap() as u8;
                    }
                    let background = if format == wgpu::TextureFormat::Bgra8UnormSrgb {
                        [255, 0, 0, 255]
                    } else {
                        [0, 0, 255, 255]
                    };
                    for (at, &index) in indices.iter().enumerate() {
                        let expected = if index == 0 {
                            background
                        } else {
                            let color = if (16..32).contains(&index) {
                                rules.house_color_ramps.ramp(house)[(index - 16) as usize]
                            } else {
                                palette.color(index)
                            };
                            encoded(
                                light.rgb565([color.r, color.g, color.b], index, 127),
                                format,
                            )
                        };
                        assert_eq!(
                            &output[0][at * 4..at * 4 + 4],
                            &expected,
                            "{type_id} {name} {part_name} {format:?} native pixel{at}"
                        );
                        assert_eq!(
                            crate::render::native_z::stored_z(f32::from_le_bytes(
                                output[1][at * 4..at * 4 + 4].try_into().unwrap()
                            )),
                            65535,
                            "{name}: read-only Z"
                        );
                    }
                    comparisons += 1;
                }
            }
        }
        assert_eq!(
            comparisons,
            expected_parts * if overflow { 2 } else { 1 },
            "native physical parts for opposite rolls on each format"
        );
    }
}
