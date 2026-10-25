//! Overlap-ordered stages through the production object replay: a base of
//! animated buildings must replay in a fixed number of passes, with colour and
//! Z identical to strict parent order. Renderer control, not a native scene.

use super::draw_plan_lowering::{
    ObjectPieceInstance, PlannedObjectInstance, lower_object_instances,
};
use super::merge_passes::{ObjectDrawPhase, draw_native_object_pass};
use super::object_stages::SERIAL;
use crate::render::batch::{BatchRenderer, InstanceBufferPool, SpriteInstance};
use crate::render::draw_state::DrawState;
use crate::render::sprite_atlas::{SpriteAtlas, SpriteAtlasPage};
use crate::render::tactical_draw_plan::{
    BlitPolicy, ObjectDraw, RenderZPolicy, SpriteEncoding, TacticalLayer,
};
use crate::render::terrain_draw::{TerrainDrawRenderer, TerrainPiece};
use crate::render::terrain_draw_gpu_tests::{Gpu, GpuFrameTimer, camera, clear, sprite};

use super::draw_plan_lowering::ObjectTexture;

const SIZE: [u32; 2] = [1024, 768];
const GROUPS: usize = 24;

fn parent(id: u64, pieces: Vec<ObjectPieceInstance>) -> PlannedObjectInstance {
    PlannedObjectInstance::object(
        ObjectDraw {
            id,
            layer: TacticalLayer(2),
            display_order: id,
            policy: BlitPolicy::z_read(SpriteEncoding::Plain),
        },
        pieces,
    )
}

fn piece(
    target: ObjectTexture,
    render_z: RenderZPolicy,
    instance: SpriteInstance,
) -> ObjectPieceInstance {
    ObjectPieceInstance {
        target,
        render_z,
        instance,
    }
}

/// Each group: a Z-writing building, then an Anim parent with a 50%
/// translucent body and its shadow (Z-tested on even groups, no-Z on odd).
/// A last sprite lands on the first Anim and must stay above it.
fn base_scene() -> Vec<PlannedObjectInstance> {
    let mut parents = Vec::new();
    for group in 0..GROUPS {
        let x = 20.0 + (group % 8) as f32 * 120.0;
        let y = 40.0 + (group / 8) as f32 * 200.0;
        parents.push(parent(
            2 * group as u64,
            vec![piece(
                ObjectTexture::ShpPage(0),
                RenderZPolicy::ReadWrite,
                sprite([x, y], [64.0, 64.0], -10.0),
            )],
        ));
        let mut body = sprite([x + 12.0, y - 20.0], [40.0, 48.0], -2.0);
        body.draw_state = DrawState::from_native_selector(0, 2);
        let shadow_z = if group % 2 == 0 {
            RenderZPolicy::ReadOnly
        } else {
            RenderZPolicy::None
        };
        let mut shadow = sprite([x + 24.0, y + 30.0], [40.0, 24.0], -3.0);
        shadow.draw_state = DrawState::from_native_selector(0, 1);
        parents.push(parent(
            2 * group as u64 + 1,
            vec![
                piece(ObjectTexture::ShpPage(1), RenderZPolicy::ReadOnly, body),
                piece(
                    ObjectTexture::SpriteAtlasEdit(2, TerrainPiece::Shadow),
                    shadow_z,
                    shadow,
                ),
            ],
        ));
    }
    parents.push(parent(
        2 * GROUPS as u64,
        vec![piece(
            ObjectTexture::ShpPage(0),
            RenderZPolicy::None,
            sprite([40.0, 30.0], [16.0, 16.0], 0.0),
        )],
    ));
    parents
}

#[test]
#[ignore = "requires GPU; production object replay of a 24-Anim base, overlap stages against strict order"]
fn animated_base_replays_in_fixed_passes_with_strict_order_pixels() {
    let gpu = Gpu::with_features(wgpu::Features::TIMESTAMP_QUERY);
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera(SIZE));
    let color = gpu.target(SIZE, format);
    let cv = color.create_view(&Default::default());
    let depth = gpu.target(SIZE, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
    terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
    let page = |rgba: &[u8], index: &[u8]| SpriteAtlasPage {
        texture: batch.create_texture_on_device(&gpu.device, &gpu.queue, rgba, 2, 1, Some(index)),
    };
    let atlas = SpriteAtlas::from_test_pages(vec![
        page(&[40, 90, 200, 255, 30, 160, 60, 255], &[1, 2]),
        page(&[240, 200, 40, 255, 200, 40, 40, 255], &[3, 4]),
        page(&[0, 0, 0, 255, 0, 0, 0, 255], &[1, 0]),
    ]);
    let layers = lower_object_instances(base_scene());
    let layer = &layers[2];
    let mut pool = InstanceBufferPool::new();
    pool.upload_on_device(&gpu.device, &gpu.queue, "base", &layer.instances);
    let timer = GpuFrameTimer::new(&gpu);
    let mut replay = |serial: bool| -> (usize, f64, Vec<Vec<u8>>) {
        SERIAL.set(serial);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(
            &mut encoder,
            &cv,
            &dv,
            0,
            wgpu::LoadOp::Clear(wgpu::Color::WHITE),
        );
        gpu.queue.submit([encoder.finish()]);
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        let (_, _, gpu_ms) = timer.measure(&gpu, |encoder| {
            draw_native_object_pass(
                encoder,
                ObjectDrawPhase::Display,
                &cv,
                &dv,
                &mut terrain,
                [0, 0, SIZE[0], SIZE[1]],
                &batch,
                pool.get("base"),
                layer,
                None,
                None,
                None,
                Some(&atlas),
                None,
                batch.default_zshape_bind_group(),
                None,
                None,
            )
        });
        SERIAL.set(false);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let reads = [
            gpu.read(&mut encoder, &color),
            gpu.read(&mut encoder, &depth),
        ];
        let passes = terrain.closed_passes();
        (passes, gpu_ms, gpu.finish(encoder, &reads, SIZE))
    };
    let (serial, serial_ms, expected) = replay(true);
    let (staged, staged_ms, output) = replay(false);
    eprintln!(
        "serial: {} passes, {serial_ms:.3} ms; staged: {} passes, {staged_ms:.3} ms",
        serial, staged
    );
    assert!(
        expected[0] != vec![255; expected[0].len()],
        "the scene draws"
    );
    assert!(
        output[0] == expected[0],
        "colour equals strict parent order"
    );
    assert!(output[1] == expected[1], "Z equals strict parent order");
    // Ordinary buildings, packed bodies, Z-tested shadows, no-Z shadows, the
    // last sprite: one transaction each, independent of the Anim count.
    assert!(serial > GROUPS * 9, "strict order: {serial}");
    assert_eq!(staged, 1 + 4 + 4 + 4 + 1);
}
