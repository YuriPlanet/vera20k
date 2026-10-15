//! Actual stock bomb body through pose-page upload, retained lowering and
//! UnitPose replay. Packed raster bytes have native tests; these comparisons
//! establish connected GPU composition, not a native final-frame golden.
use super::super::draw_plan_lowering::{NativeDisplayOrder, lower_object_instances};
use super::*;
use crate::app::presentation::instances::{
    admitted_kirov_bomb, launched_stock_submarine_torpedo, projectile_draw_instance,
};
use crate::render::batch::{DepthAxis, InstanceBufferPool};
use crate::render::palette_light::PaletteLight;
use crate::render::palette_textures::PaletteSet;
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded, seed_depth_grid};
use crate::render::unit_pose_cache::VxlPoseFrameCache;
use crate::render::vxl_raster::{VxlLighting, projectile_voxel_matrix};
use crate::rules::house_colors::HouseColorIndex;
use crate::sim::projectile::ProjectileVelocity;

/// Original 468090 ->46B0C0 ->490B90 ->4AF2A0 ->4373B0 ->494B60,
/// with physical SUBT/HVA/VPL and the original selected ColorScheme Convert.
/// Velocity, screen point, clip and destination buffers are supplied render
/// boundaries. The admitted Bullet comes from the ordinary stock launch owner.
#[test]
#[ignore = "requires GPU and retail assets; Flat SUBT upload/Surface replay versus original RGB565/A/Z controls"]
fn stock_flat_torpedo_pose_upload_and_surface_replay_match_original_pixels() {
    let Some((rules, assets, mut sim, id)) = launched_stock_submarine_torpedo(false) else {
        return;
    };
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/bullet_flat_fixtures.json",
    ))
    .unwrap();
    for asset in native["assets"]
        .as_object()
        .unwrap()
        .values()
        .chain(std::iter::once(&native["color_scheme"]["palette"]))
    {
        let bytes = assets.get_ref(asset["name"].as_str().unwrap()).unwrap();
        assert_eq!(bytes.len() as u64, asset["bytes"]);
        assert_eq!(crate::util::sha256::sha256_hex(bytes), asset["sha256"]);
    }
    let size = [
        native["surface"]["width"].as_u64().unwrap() as u32,
        native["surface"]["height"].as_u64().unwrap() as u32,
    ];
    let pixels = (size[0] * size[1]) as usize;
    assert_eq!(native["surface"]["y_origin"], 0);
    assert_eq!(native["surface"]["row_stride_words"], size[0]);
    let palette =
        crate::assets::pal_file::Palette::from_bytes(assets.get_ref("UNITTEM.PAL").unwrap())
            .unwrap();
    let gpu = Gpu::new();
    let mut cache = VxlPoseFrameCache::default();
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let mut batch =
            crate::render::batch::BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let palettes = PaletteSet::new_on_device(
            &gpu.device,
            &gpu.queue,
            &palette,
            &rules.house_color_ramps,
            &[HouseColorIndex(0)],
        );
        for row in native["rows"].as_array().unwrap() {
            let name = row["name"].as_str().unwrap();
            let input = &row["input"];
            let mut kind = rules.projectile("Torpedo").unwrap().clone();
            // One declared authored render-header control isolates the
            // non-Flat leaf. Ordinary encounter tests never override Flat.
            kind.flat = input["flat"].as_bool().unwrap();
            let physical_z = native_surface_words(input, "old_z", "z_patches", pixels);
            let physical_a = native_surface_words(input, "a_value", "a_patches", pixels);
            let scroll = input["scroll_rows"].as_u64().unwrap_or(0) as u32;
            // The GPU fixture takes a logical viewport. Original A/Z buffers
            // are circular: leaf offsets below establish this input mapping.
            let physical_index = |x: u32, y: u32| (((y + scroll) % size[1]) * size[0] + x) as usize;
            let logical = |words: &[u16]| {
                (0..size[1])
                    .flat_map(|y| (0..size[0]).map(move |x| words[physical_index(x, y)]))
                    .collect::<Vec<_>>()
            };
            let z_words = logical(&physical_z);
            let a_words = logical(&physical_a);
            let point = &input["point"];
            let bullet = sim.projectiles.get(id).unwrap();
            let (x, y) = crate::util::lepton::absolute_leptons_to_screen(
                bullet.position.x,
                bullet.position.y,
                bullet.position.z,
            );
            let cam = [
                x - point[0].as_f64().unwrap() as f32,
                y - point[1].as_f64().unwrap() as f32,
            ];
            let mut uniform = camera(size);
            uniform.camera_pos = cam;
            // Existing native-shape GPU fixtures transport supplied buffer
            // baselines through this origin, preserving the producer's bias.
            uniform.native_z_origin_y = input["depth_baseline"].as_i64().unwrap() as f32 - 32768.0;
            batch.write_camera(&gpu.queue, uniform);
            let fog = crate::render::shroud_buffer::ShroudBuffer::fixture(
                &gpu.device,
                &gpu.queue,
                size,
                cam,
                &a_words
                    .iter()
                    .map(|&word| u8::try_from(word).unwrap())
                    .collect::<Vec<_>>(),
            );
            batch.bind_shroud(&gpu.device, Some(&fog));
            cache.begin_frame();
            let mut prepared = Vec::new();
            for draw in row["draws"].as_array().unwrap() {
                assert_eq!(draw["point"], *point, "{name}: one supplied camera");
                let velocity = ProjectileVelocity::from_native(std::array::from_fn(|axis| {
                    crate::util::native_x87::NativeF64Bits::from_bits(
                        u64::from_str_radix(draw["velocity_bits"][axis].as_str().unwrap(), 16)
                            .unwrap(),
                    )
                }));
                sim.projectiles.redirect(id, velocity);
                let matrix = projectile_voxel_matrix(velocity).unwrap();
                let matrix_bits: Vec<_> = (0..3)
                    .flat_map(|row| (0..4).map(move |col| matrix.col(col)[row].to_bits()))
                    .collect();
                assert_eq!(
                    serde_json::json!(matrix_bits),
                    draw["body"]["input_matrix_bits"],
                    "{name}"
                );
                let body = cache
                    .render_matrix(&assets, "SUBT", matrix, 0, true, VxlLighting::Diffuse)
                    .unwrap();
                let rect = &draw["body"]["rect"];
                assert_eq!(
                    serde_json::json!([
                        body.offset_x as i32,
                        body.offset_y as i32,
                        body.pixel_size[0] as i32,
                        body.pixel_size[1] as i32
                    ]),
                    serde_json::json!([rect[0], rect[1], rect[4], rect[5]]),
                    "{name}"
                );
                let before = sim.state_hash();
                let order = NativeDisplayOrder::from_display(sim.display_layers());
                let object = projectile_draw_instance(
                    sim.projectiles.get(id).unwrap(),
                    &kind,
                    "Torpedo",
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
                );
                assert_eq!(sim.state_hash(), before, "read-only draw: {name}");
                if let Some(object) = &object {
                    assert_eq!(object.parent.layer.0, 1, "retained Surface1: {name}");
                    assert_eq!(object.pieces.len(), 1);
                    let piece = &object.pieces[0];
                    let expected_z = if kind.flat {
                        crate::render::tactical_draw_plan::RenderZPolicy::ReadOnly
                    } else {
                        crate::render::tactical_draw_plan::RenderZPolicy::None
                    };
                    assert_eq!(object.parent.policy.render_z, expected_z);
                    assert_eq!(piece.render_z, expected_z);
                    assert_eq!(
                        serde_json::json!(piece.instance.z_adjust as i32),
                        draw["palette_call"]["z_adjust"]
                    );
                    assert_eq!(
                        serde_json::json!(piece.instance.z_gradient),
                        draw["palette_call"]["gradient"]
                    );
                }
                let layers = lower_object_instances(object.into_iter().collect());
                assert!(
                    layers
                        .iter()
                        .enumerate()
                        .all(|(i, l)| i == 1 || l.instances.is_empty())
                );
                prepared.push((layers, body));
            }
            // Build all poses before the single upload, as the production frame.
            cache.upload(&gpu.device, &gpu.queue, &batch);
            let color = gpu.target(size, format);
            let cv = color.create_view(&Default::default());
            let depth = gpu.target(size, wgpu::TextureFormat::Depth32Float);
            let dv = depth.create_view(&Default::default());
            let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
            terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
            let background = input["background"].as_u64().unwrap() as u16;
            assert_eq!(background, u16::MAX, "native fixture supplies white");
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            clear(
                &mut encoder,
                &cv,
                &dv,
                z_words[0],
                wgpu::LoadOp::Clear(wgpu::Color::WHITE),
            );
            seed_depth_grid(&gpu, &mut encoder, &dv, &z_words, size[0]);
            let mut pool = InstanceBufferPool::new();
            for (index, ((layers, body), draw)) in prepared
                .iter()
                .zip(row["draws"].as_array().unwrap())
                .enumerate()
            {
                let rect = &draw["body"]["rect"];
                let mut source = vec![0u8; 65536];
                for pixel in draw["body"]["pixels"].as_array().unwrap() {
                    source[pixel[0].as_u64().unwrap() as usize] = pixel[1].as_u64().unwrap() as u8;
                }
                let mut cropped = Vec::new();
                for y in
                    rect[3].as_u64().unwrap()..rect[3].as_u64().unwrap() + rect[5].as_u64().unwrap()
                {
                    let start = (y * 256 + rect[2].as_u64().unwrap()) as usize;
                    cropped.extend_from_slice(
                        &source[start..start + rect[4].as_u64().unwrap() as usize],
                    );
                }
                assert_eq!(
                    gpu.read_uint_texels(
                        &cache.texture().unwrap().view,
                        body.uv_origin.map(|v| (v * 1024.0) as u32),
                        body.pixel_size.map(|v| v as u32)
                    ),
                    cropped,
                    "{name}/{index}: actual latest pose upload versus native raster"
                );
                for leaf in draw["leaves"].as_array().unwrap() {
                    let at = physical_index(
                        leaf["x"].as_u64().unwrap() as u32,
                        leaf["y"].as_u64().unwrap() as u32,
                    );
                    assert_eq!(leaf["z_offset_words"].as_u64().unwrap() as usize, at);
                    assert_eq!(leaf["a_offset_words"].as_u64().unwrap() as usize, at);
                }
                let label = "flat-native";
                pool.upload_on_device(&gpu.device, &gpu.queue, label, &layers[1].instances);
                let clip = &draw["clip"];
                let tactical = std::array::from_fn(|i| clip[i].as_u64().unwrap() as u32);
                let stats = draw_native_object_pass(
                    &mut encoder,
                    &cv,
                    &dv,
                    &mut terrain,
                    tactical,
                    &batch,
                    pool.get(label),
                    &layers[1],
                    None,
                    None,
                    cache.texture(),
                    &crate::render::unit_slope_transition_cache::VxlSlopeTransitionCache::default(),
                    None,
                    Some(&palettes),
                    batch.default_zshape_bind_group(),
                );
                if name == "negative_candidate" {
                    assert!(
                        stats.passes > 0 && stats.pieces > 0,
                        "signed-negative body must reach the manual destination owner"
                    );
                } else {
                    assert_eq!(
                        stats.passes, 0,
                        "{name}: ordinary u16-safe bodies retain batched drawing"
                    );
                }
                let reads = [
                    gpu.read(&mut encoder, &color),
                    gpu.read(&mut encoder, &depth),
                ];
                let output = gpu.finish(encoder, &reads, size);
                let after = &draw["after_draw"];
                assert_eq!(after["z_unchanged"], true);
                assert_eq!(after["a_unchanged"], true);
                let mut expected = vec![background; pixels];
                for pixel in after["changed_pixels"].as_array().unwrap() {
                    let at = (pixel[1].as_u64().unwrap() * u64::from(size[0])
                        + pixel[0].as_u64().unwrap()) as usize;
                    expected[at] = pixel[2].as_u64().unwrap() as u16;
                }
                let expected_bytes: Vec<_> = expected
                    .iter()
                    .flat_map(|word| word.to_le_bytes())
                    .collect();
                assert_eq!(
                    crate::util::sha256::sha256_hex(&expected_bytes),
                    after["color_sha256"],
                    "{name}/{index}: complete native RGB565"
                );
                for (at, (&word, pixel)) in expected
                    .iter()
                    .zip(output[0].as_chunks::<4>().0)
                    .enumerate()
                {
                    assert_eq!(
                        *pixel,
                        encoded(word, format),
                        "{format:?}/{name}/{index}: RGB ({},{})",
                        at % size[0] as usize,
                        at / size[0] as usize
                    );
                    assert_eq!(
                        crate::render::native_z::stored_z(f32::from_le_bytes(
                            output[1][at * 4..at * 4 + 4].try_into().unwrap()
                        )),
                        z_words[at],
                        "{format:?}/{name}/{index}: no Z write at{at}"
                    );
                }
                encoder = gpu.device.create_command_encoder(&Default::default());
            }
        }
    }
}

fn native_surface_words(
    input: &serde_json::Value,
    initial: &str,
    patches: &str,
    count: usize,
) -> Vec<u16> {
    let mut words = vec![input[initial].as_u64().unwrap() as u16; count];
    if let Some(patches) = input[patches].as_array() {
        for patch in patches {
            words[patch[0].as_u64().unwrap() as usize] = patch[1].as_u64().unwrap() as u16;
        }
    }
    words
}

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
                let stats = draw_native_object_pass(
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
                assert_eq!(
                    stats.passes, 0,
                    "non-Flat Kirov keeps its ordinary passthrough batch"
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
