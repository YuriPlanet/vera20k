//! The ordinary object submission route must preserve the no-Z index mask,
//! source holes and subsequent Display draws. This uses the existing native
//! packed-half words; it is a renderer control, not a full native scene.

use super::super::draw_plan_lowering::{
    ObjectPieceInstance, PlannedObjectInstance, lower_object_instances,
};
use super::*;
use crate::render::batch::InstanceBufferPool;
use crate::render::draw_state::DrawState;
use crate::render::tactical_draw_plan::{BlitPolicy, ObjectDraw, SpriteEncoding, TacticalLayer};
use crate::render::terrain_draw::{TerrainDrawRenderer, TerrainPiece};
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, clear, encoded, sprite};

#[test]
#[ignore = "requires GPU; actual object submission of no-Z and Z-tested Anim shadow masks"]
fn ordinary_anim_no_z_shadow_preserves_holes_depth_and_display_fences() {
    let gpu = Gpu::new();
    let size = [4, 2];
    let half_words =
        crate::test_fixture::bytes("tools/terrain_draw_oracle/fixtures/half-rgb565.bin");
    let half_white = u16::from_le_bytes(half_words[131070..131072].try_into().unwrap());
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
        let atlas = SpriteAtlas::from_test_pages(vec![
            crate::render::sprite_atlas::SpriteAtlasPage {
                texture: batch.create_texture_on_device(
                    &gpu.device,
                    &gpu.queue,
                    &[17, 231, 49, 0, 248, 0, 0, 255],
                    2,
                    1,
                    Some(&[1, 0]),
                ),
            },
            crate::render::sprite_atlas::SpriteAtlasPage {
                texture: batch.create_texture_on_device(
                    &gpu.device,
                    &gpu.queue,
                    &[248, 0, 0, 255],
                    1,
                    1,
                    Some(&[1]),
                ),
            },
        ]);
        let parents = [RenderZPolicy::None, RenderZPolicy::ReadOnly]
            .into_iter()
            .enumerate()
            .map(|(index, policy)| {
                let mut instance = sprite([0.0, 0.0], [4.0, 2.0], 65535.0);
                instance.draw_state = DrawState::from_native_selector(0, 1);
                PlannedObjectInstance::object(
                    ObjectDraw {
                        id: index as u64,
                        layer: TacticalLayer(2),
                        display_order: index as u64,
                        policy: BlitPolicy::z_read(SpriteEncoding::Plain),
                    },
                    vec![ObjectPieceInstance {
                        target: ObjectTexture::SpriteAtlasEdit(0, TerrainPiece::Shadow),
                        render_z: policy,
                        instance,
                    }],
                )
            });
        let last = PlannedObjectInstance::object(
            ObjectDraw {
                id: 2,
                layer: TacticalLayer(2),
                display_order: 2,
                policy: BlitPolicy::z_none(SpriteEncoding::Plain),
            },
            vec![ObjectPieceInstance {
                target: ObjectTexture::ShpPage(1),
                render_z: RenderZPolicy::None,
                instance: sprite([0.0, 0.0], [1.0, 1.0], 0.0),
            }],
        );
        let layers = lower_object_instances(parents.chain(std::iter::once(last)).collect());
        let layer = &layers[2];
        assert_eq!(layer.runs[0].packed_parent, Some(0));
        assert_eq!(layer.runs[1].packed_parent, None);
        let mut pool = InstanceBufferPool::new();
        pool.upload_on_device(&gpu.device, &gpu.queue, "anim_shadow", &layer.instances);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(
            &mut encoder,
            &cv,
            &dv,
            0,
            wgpu::LoadOp::Clear(wgpu::Color::WHITE),
        );
        draw_native_object_pass(
            &mut encoder,
            ObjectDrawPhase::Display,
            &cv,
            &dv,
            &mut terrain,
            [0, 0, size[0], size[1]],
            &batch,
            pool.get("anim_shadow"),
            layer,
            None,
            None,
            None,
            Some(&atlas),
            None,
            batch.default_zshape_bind_group(),
            None,
            None,
        );
        let reads = [
            gpu.read(&mut encoder, &color),
            gpu.read(&mut encoder, &depth),
        ];
        let output = gpu.finish(encoder, &reads, size);
        for y in 0..2usize {
            for x in 0..4usize {
                let word = if (x, y) == (0, 0) {
                    0xf800
                } else if x < 2 {
                    half_white
                } else {
                    0xffff
                };
                let at = (y * 4 + x) * 4;
                assert_eq!(
                    &output[0][at..at + 4],
                    encoded(word, format),
                    "{format:?} ({x},{y})"
                );
                assert_eq!(
                    crate::render::native_z::stored_z(f32::from_le_bytes(
                        output[1][at..at + 4].try_into().unwrap()
                    )),
                    0
                );
            }
        }
    }
}
