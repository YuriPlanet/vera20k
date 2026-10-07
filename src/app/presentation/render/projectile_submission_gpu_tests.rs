//! Production replay must submit before Metal's native command-buffer pool
//! fills, including when ordinary Unit/SHP draws fence every Bullet span.
use super::super::draw_plan_lowering::{
    ObjectPieceInstance, PlannedObjectInstance, lower_object_instances,
};
use super::*;
use crate::render::batch::InstanceBufferPool;
use crate::render::tactical_draw_plan::{BlitPolicy, ObjectDraw, SpriteEncoding, TacticalLayer};
use crate::render::terrain_draw::{TerrainDrawRenderer, TerrainPiece};
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded, sprite};

#[test]
#[ignore = "requires GPU; production replay exceeds the former unsubmitted Metal buffer limit"]
fn many_ordinary_fences_submit_in_order_and_preserve_native_destination_words() {
    let gpu = Gpu::new();
    let size = [12, 4];
    // Each round fences two Bullet edits with actual UnitAtlas and SHP draws.
    // The resulting >3200 render passes exceeded the pinned backend's 4096
    // native-buffer pool before any submission in the former implementation.
    const ROUNDS: usize = 640;
    let native_half =
        crate::test_fixture::bytes("tools/projectile_oracle/bridge_render_pixels.rgb565.bin");
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
        let shp = SpriteAtlas::from_test_pages(
            [[80, 180, 80, 255], [248, 0, 0, 255]]
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
        let units = UnitAtlas::from_test_pages(vec![crate::render::unit_atlas::UnitAtlasPage {
            texture: batch.create_unit_atlas_texture_on_device(
                &gpu.device,
                &gpu.queue,
                1,
                1,
                &[33],
            ),
        }]);
        let palette = crate::assets::pal_file::Palette {
            colors: [crate::assets::pal_file::Color::rgb(0, 252, 0); 256],
        };
        let ramps = crate::rules::house_colors::HouseColorRamps::from_schemes(&[]);
        let palettes = PaletteSet::new_on_device(&gpu.device, &gpu.queue, &palette, &ramps, &[]);
        let mut parents = Vec::with_capacity(ROUNDS * 4);
        for round in 0..ROUNDS {
            for piece in 0..4 {
                let (target, render_z, mut instance) = match piece {
                    0 => (
                        ObjectTexture::ProjectileShp(0, TerrainPiece::Body),
                        RenderZPolicy::ReadOnly,
                        sprite([0., 1.], [8., 2.], 3.),
                    ),
                    1 => (
                        ObjectTexture::UnitAtlasPage(0),
                        RenderZPolicy::ReadOnly,
                        sprite([4., 1.], [2., 2.], -1.),
                    ),
                    2 => (
                        ObjectTexture::ShpPage(1),
                        RenderZPolicy::None,
                        sprite([2., 1.], [2., 2.], 0.),
                    ),
                    _ => (
                        ObjectTexture::ProjectileShp(0, TerrainPiece::Shadow),
                        RenderZPolicy::ReadOnly,
                        sprite([0., 1.], [8., 2.], 3.),
                    ),
                };
                if piece == 1 {
                    instance.z_gradient = crate::render::native_z::pack_voxel_z_gradient(
                        crate::render::native_z::ZGradient::Vertical,
                        false,
                    );
                    instance.zshape_origin = [1., 2.];
                }
                let id = (round * 4 + piece) as u64;
                parents.push(PlannedObjectInstance::object(
                    ObjectDraw {
                        id,
                        layer: TacticalLayer(3),
                        display_order: id,
                        policy: BlitPolicy::z_read(SpriteEncoding::Plain),
                    },
                    vec![ObjectPieceInstance {
                        target,
                        render_z,
                        instance,
                    }],
                ));
            }
        }
        let layers = lower_object_instances(parents);
        let layer = &layers[3];
        assert_eq!(layer.runs.len(), ROUNDS * 4);
        let mut pool = InstanceBufferPool::new();
        pool.upload_on_device(&gpu.device, &gpu.queue, "fenced_bullets", &layer.instances);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(
            &mut encoder,
            &cv,
            &dv,
            65535,
            wgpu::LoadOp::Clear(wgpu::Color::WHITE),
        );
        let stats = draw_native_object_pass(
            &mut encoder,
            &cv,
            &dv,
            &mut terrain,
            [0, 0, size[0], size[1]],
            &batch,
            pool.get("fenced_bullets"),
            layer,
            None,
            Some(&units),
            None,
            &VxlSlopeTransitionCache::default(),
            Some(&shp),
            Some(&palettes),
            batch.default_zshape_bind_group(),
        );
        assert_eq!(stats.pieces, ROUNDS * 2);
        assert_eq!(stats.passes, (ROUNDS + 1) * 4);
        assert_eq!(stats.tile_dependencies, 0);
        assert!(
            terrain.submissions() > 0,
            "closed passes must be submitted before encoder finish"
        );
        let reads = [
            gpu.read(&mut encoder, &color),
            gpu.read(&mut encoder, &depth),
        ];
        let output = gpu.finish(encoder, &reads, size);
        for y in 0..size[1] as usize {
            for x in 0..size[0] as usize {
                let word = if (1..3).contains(&y) && x < 8 {
                    let before_shadow = match x {
                        2..=3 => 0xf800,
                        4..=5 => 0x07e0,
                        _ => 0x55aa,
                    };
                    u16::from_le_bytes(
                        native_half[before_shadow * 2..before_shadow * 2 + 2]
                            .try_into()
                            .unwrap(),
                    )
                } else {
                    0xffff
                };
                let p = (y * size[0] as usize + x) * 4;
                assert_eq!(
                    &output[0][p..p + 4],
                    encoded(word, format),
                    "{format:?} ({x},{y})"
                );
                assert_eq!(
                    crate::render::native_z::stored_z(f32::from_le_bytes(
                        output[1][p..p + 4].try_into().unwrap()
                    )),
                    65535
                );
            }
        }
        eprintln!(
            "{format:?}: {ROUNDS} ordinary Unit/SHP fences, {} destination passes, {} bounded submissions; full attachment color/depth match",
            stats.passes,
            terrain.submissions()
        );
    }
}
