//! Actual stock bomb body through pose-page upload, retained lowering and
//! UnitPose replay. Packed raster bytes have native tests; these comparisons
//! establish connected GPU composition, not a native final-frame golden.
use super::super::draw_plan_lowering::{NativeDisplayOrder, lower_object_instances};
use super::*;
use crate::app::presentation::instances::{admitted_kirov_bomb, projectile_draw_instance};
use crate::render::batch::{DepthAxis, InstanceBufferPool};
use crate::render::palette_light::PaletteLight;
use crate::render::palette_textures::PaletteSet;
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded};
use crate::render::unit_pose_cache::VxlPoseFrameCache;
use crate::render::vxl_raster::{VxlLighting, projectile_voxel_matrix};
use crate::rules::house_colors::HouseColorIndex;
use crate::sim::projectile::ProjectileVelocity;

#[test]
#[ignore = "requires GPU and retail assets; Kirov two-frame pose upload and UnitPose palette/depth replay"]
fn kirov_voxel_pose_upload_and_retained_replay_survive_a_changed_second_frame() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let rules = &battle.rules;
    let kind = rules.projectile("BlimpBombP").unwrap();
    let palette =
        crate::assets::pal_file::Palette::from_bytes(assets.get_ref("UNITTEM.PAL").unwrap())
            .unwrap();
    let vpl = crate::assets::vpl_file::VplFile::from_bytes(assets.get_ref("VOXELS.VPL").unwrap())
        .unwrap();
    let model = crate::render::unit_atlas::UnitModel::load(&assets, "ZBOMB", None).unwrap();
    let gpu = Gpu::new();
    let size = [96, 96];
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut batch =
            crate::render::batch::BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        // No active house uses LightGold. The loader's type-color demand
        // supplies this row independently (its CPU regression checks that).
        let palettes = PaletteSet::new_on_device(
            &gpu.device,
            &gpu.queue,
            &palette,
            &rules.house_color_ramps,
            &[HouseColorIndex(0)],
        );
        let (mut sim, id) = admitted_kirov_bomb(rules, "BlimpBomb");
        let position = sim.projectiles.get(id).unwrap().position;
        let (x, y) =
            crate::util::lepton::absolute_leptons_to_screen(position.x, position.y, position.z);
        let cam = [x - 48.0, y - 48.0];
        let mut uniform = camera(size);
        uniform.camera_pos = cam;
        batch.write_camera(&gpu.queue, uniform);
        let fog = crate::render::shroud_buffer::ShroudBuffer::fixture(
            &gpu.device,
            &gpu.queue,
            size,
            cam,
            &vec![127; (size[0] * size[1]) as usize],
        );
        batch.bind_shroud(&gpu.device, Some(&fog));
        let mut cache = VxlPoseFrameCache::default();
        let mut previous_pixels = None;
        for velocity in [
            ProjectileVelocity::new(0, 0, -1),
            ProjectileVelocity::new(1, 0, 0),
        ] {
            sim.projectiles.get_mut(id).unwrap().velocity = velocity;
            cache.begin_frame();
            let matrix = projectile_voxel_matrix(velocity).unwrap();
            let (expected, _) = model
                .render_matrix(matrix, 0, true, VxlLighting::Diffuse, Some(&vpl))
                .unwrap();
            if let Some(previous) = previous_pixels.replace(expected.palette_indices.clone()) {
                assert_ne!(
                    previous, expected.palette_indices,
                    "the second frame must demand changed texels"
                );
            }
            let order = NativeDisplayOrder::from_display(sim.display_layers());
            let object = projectile_draw_instance(
                sim.projectiles.get(id).unwrap(),
                kind,
                "BlimpBombP",
                HouseColorIndex(8),
                None,
                sim.resolved_terrain.as_ref().unwrap(),
                cam,
                size.map(|n| n as f32),
                DepthAxis::NONE,
                &order,
                |image, matrix| {
                    cache.render_matrix(&assets, image, matrix, 0, true, VxlLighting::Diffuse)
                },
            )
            .unwrap();
            let instance = object.pieces[0].instance;
            let reused = cache
                .render_matrix(&assets, "ZBOMB", matrix, 0, true, VxlLighting::Diffuse)
                .unwrap();
            assert_eq!(
                instance.uv_origin, reused.uv_origin,
                "equal simultaneous bombs reuse the same page slot"
            );
            // One upload after all requests, as the production frame builder.
            cache.upload(&gpu.device, &gpu.queue, &batch);
            let texels = gpu.read_uint_texels(
                &cache.texture().unwrap().view,
                instance.uv_origin.map(|v| (v * 1024.0) as u32),
                [expected.width, expected.height],
            );
            assert_eq!(
                texels, expected.palette_indices,
                "latest frame's full pose page contents"
            );
            let layers = lower_object_instances(vec![object]);
            let layer = &layers[3];
            let mut pool = InstanceBufferPool::new();
            pool.upload_on_device(&gpu.device, &gpu.queue, "kirov", &layer.instances);
            for stored in [0u16, 65535] {
                let color = gpu.target(size, format);
                let cv = color.create_view(&Default::default());
                let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
                let dv = depth.create_view(&Default::default());
                let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
                terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                clear(
                    &mut encoder,
                    &cv,
                    &dv,
                    stored,
                    wgpu::LoadOp::Clear(wgpu::Color::BLUE),
                );
                draw_native_object_pass(
                    &mut encoder,
                    &cv,
                    &dv,
                    &mut terrain,
                    [0, 0, size[0], size[1]],
                    &batch,
                    pool.get("kirov"),
                    layer,
                    None,
                    None,
                    cache.texture(),
                    &crate::render::unit_slope_transition_cache::VxlSlopeTransitionCache::default(),
                    None,
                    Some(&palettes),
                    batch.default_zshape_bind_group(),
                );
                let reads = [
                    gpu.read(&mut encoder, &color),
                    gpu.read(&mut encoder, &depth),
                ];
                let output = gpu.finish(encoder, &reads, size);
                let left = (instance.position[0] - cam[0]).round() as i32;
                let top = (instance.position[1] - cam[1]).round() as i32;
                let light = PaletteLight::new([1000; 3], 53, 1000, true);
                let background = if format == wgpu::TextureFormat::Bgra8UnormSrgb {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                };
                let mut drawn = 0;
                for row in 0..size[1] as i32 {
                    for col in 0..size[0] as i32 {
                        let sx = col - left;
                        let sy = row - top;
                        let index = if sx >= 0
                            && sy >= 0
                            && sx < expected.width as i32
                            && sy < expected.height as i32
                        {
                            expected.palette_indices
                                [(sy as u32 * expected.width + sx as u32) as usize]
                        } else {
                            0
                        };
                        let color = if index == 0 {
                            background
                        } else {
                            drawn += 1;
                            let rgb = if (16..32).contains(&index) {
                                rules.house_color_ramps.ramp(HouseColorIndex(0))
                                    [(index - 16) as usize]
                            } else {
                                palette.colors[index as usize]
                            };
                            encoded(light.rgb565([rgb.r, rgb.g, rgb.b], index, 127), format)
                        };
                        let at = ((row as u32 * size[0] + col as u32) * 4) as usize;
                        assert_eq!(
                            &output[0][at..at + 4],
                            &color,
                            "{format:?} stored{stored} ({col},{row})"
                        );
                        assert_eq!(
                            crate::render::native_z::stored_z(f32::from_le_bytes(
                                output[1][at..at + 4].try_into().unwrap()
                            )),
                            stored,
                            "the non-Flat Bullet body neither reads nor writes depth"
                        );
                    }
                }
                assert!(drawn > 0, "non-Flat0x800 must draw even over rejecting Z");
            }
        }
    }
}
