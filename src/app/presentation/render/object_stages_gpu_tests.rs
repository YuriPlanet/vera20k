//! Overlap-ordered stages through the production object replay: a base of
//! animated buildings, and combat lights and surface stores between sprites,
//! must replay in a fixed number of passes, with colour and Z identical to
//! strict parent order. Renderer controls, not native scenes.

use super::draw_plan_lowering::{
    ObjectBodyDraw, ObjectPieceInstance, PlannedObjectInstance, lower_object_instances,
};
use super::merge_passes::{ObjectDrawPhase, draw_native_object_pass};
use super::object_stages::SERIAL;
use crate::render::batch::{BatchRenderer, InstanceBufferPool, SpriteInstance};
use crate::render::combat_light::{CombatLightDrawRecord, CombatLightRenderer};
use crate::render::draw_state::DrawState;
use crate::render::sprite_atlas::{SpriteAtlas, SpriteAtlasPage};
use crate::render::surface_line::{SurfaceDraw, SurfaceLineViewport};
use crate::render::tactical_draw_plan::{
    BlitPolicy, ObjectDraw, RenderZPolicy, SpriteEncoding, TacticalLayer,
};
use crate::render::terrain_draw::{TerrainDrawRenderer, TerrainPiece};
use crate::render::terrain_draw_gpu_tests::{Gpu, GpuFrameTimer, camera, clear, sprite};
use crate::sim::projectile::ProjectileCoord;

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

/// Each group: a Z-writing building, then an Anim parent with a translucent
/// body (25, 50 or 75% by group) and its shadow, both Z-tested on even
/// groups and no-Z on odd. A last sprite lands on the first Anim and must
/// stay above it.
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
        body.draw_state = DrawState::from_native_selector(0, [2, 4, 6][group % 3]);
        let render_z = if group % 2 == 0 {
            RenderZPolicy::ReadOnly
        } else {
            RenderZPolicy::None
        };
        let mut shadow = sprite([x + 24.0, y + 30.0], [40.0, 24.0], -3.0);
        shadow.draw_state = DrawState::from_native_selector(0, 1);
        parents.push(parent(
            2 * group as u64 + 1,
            vec![
                piece(ObjectTexture::ShpPage(1), render_z, body),
                piece(
                    ObjectTexture::SpriteAtlasEdit(2, TerrainPiece::Shadow),
                    render_z,
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
    // Ordinary buildings, packed bodies of every selector and Z policy (one
    // wave: 2 passes, 3 where WebGL2 snapshots apart), Z-tested shadows (a
    // read-only interval: 3 passes, 4 where WebGL2 reduces it), no-Z shadows
    // (packed), the last sprite: one transaction each, independent of the Anim
    // count. The no-Z shadows' resolve joins the last sprite's pass.
    let (packed, read_only) = if crate::render::gpu::webgl_fallback(&gpu.device) {
        (3, 4)
    } else {
        (2, 3)
    };
    assert!(serial > GROUPS * 5, "strict order: {serial}");
    assert_eq!(staged, 1 + packed + read_only + (packed - 1) + 1);
}

/// One light record whose quad centre projects to `center` under a zero
/// camera: `project_combat_light_anchor` adds 15 rows to the planar point.
fn light_at(center: [i32; 2], surface_index: i32) -> CombatLightDrawRecord {
    let difference = center[0] * 256 / 30;
    let sum = (center[1] - 15) * 256 / 15;
    CombatLightDrawRecord {
        coord: ProjectileCoord::new((sum + difference) / 2, (sum - difference) / 2, 0),
        surface_index,
        flags: 1,
    }
}

fn effect_parent(id: u64, body: ObjectBodyDraw) -> PlannedObjectInstance {
    let mut parent = parent(id, Vec::new());
    parent.body_draw = Some(body);
    parent
}

/// Twelve disjoint groups: a Z-writing building, a combat light over it, a
/// surface store over both and a later sprite above the light. Then an
/// offscreen light and an unsupported native index, neither visible.
fn effect_scene() -> Vec<PlannedObjectInstance> {
    let mut parents = Vec::new();
    for group in 0..12 {
        let [cx, cy] = [150 + (group % 3) * 350, 90 + (group / 3) * 190];
        let [x, y] = [cx as f32, cy as f32];
        let id = 4 * group as u64;
        parents.push(parent(
            id,
            vec![piece(
                ObjectTexture::ShpPage(0),
                RenderZPolicy::ReadWrite,
                sprite([x - 32.0, y - 32.0], [64.0, 64.0], -10.0),
            )],
        ));
        parents.push(effect_parent(
            id + 1,
            ObjectBodyDraw::CombatLight(light_at([cx, cy], 40)),
        ));
        parents.push(effect_parent(
            id + 2,
            ObjectBodyDraw::Surface(SurfaceDraw::OpaqueRect {
                rect: [cx - 8, cy - 40, 48, 24],
                word: 0xf800,
            }),
        ));
        parents.push(parent(
            id + 3,
            vec![piece(
                ObjectTexture::ShpPage(0),
                RenderZPolicy::None,
                sprite([x + 20.0, y - 10.0], [40.0, 30.0], 0.0),
            )],
        ));
    }
    parents.push(effect_parent(
        100,
        ObjectBodyDraw::CombatLight(light_at([-2000, 300], 40)),
    ));
    parents.push(effect_parent(
        101,
        ObjectBodyDraw::CombatLight(light_at([500, 280], -1)),
    ));
    parents
}

#[test]
#[ignore = "requires GPU; production replay of lights and surface stores between sprites, overlap stages against strict order"]
fn effects_between_sprites_replay_in_fixed_passes_with_strict_order_pixels() {
    let gpu = Gpu::new();
    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    batch.write_camera(&gpu.queue, camera(SIZE));
    let mut lights = CombatLightRenderer::new_with_device(&gpu.device, &gpu.queue, format);
    let mut layers = lower_object_instances(effect_scene());
    let layer = &mut layers[2];
    assert_eq!(layer.light_records.len(), 14);
    lights.prepare_with_device(
        &gpu.device,
        &gpu.queue,
        &layer.light_records,
        SIZE.map(|extent| extent as f32),
        [0.0; 2],
        1.0,
    );
    let color = lights.composition_texture();
    let cv = lights.composition_view();
    let depth = gpu.target(SIZE, wgpu::TextureFormat::Depth32Float);
    let dv = depth.create_view(&Default::default());
    let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
    terrain.prepare(&gpu.device, color, &dv, batch.camera_uniform());
    terrain.begin_surface_lines();
    let viewport = SurfaceLineViewport {
        camera: [0; 2],
        clip: [0, 0, SIZE[0] as i32, SIZE[1] as i32],
        z_origin_y: 0,
        zoom: 1.0,
    };
    layer.line_batches = layer
        .surface_draws
        .iter()
        .map(|draws| {
            terrain.prepare_surface_lines(
                &gpu.device,
                &gpu.queue,
                std::iter::empty(),
                draws.iter().copied(),
                &[],
                &[],
                &[],
                viewport,
                |_| false,
            )
        })
        .collect();
    assert_eq!(layer.line_batches.len(), 12, "each store follows a light");
    let layer = &layers[2];
    let page = |rgba: &[u8], index: &[u8]| SpriteAtlasPage {
        texture: batch.create_texture_on_device(&gpu.device, &gpu.queue, rgba, 2, 1, Some(index)),
    };
    let atlas =
        SpriteAtlas::from_test_pages(vec![page(&[40, 90, 200, 255, 30, 160, 60, 255], &[1, 2])]);
    let mut pool = InstanceBufferPool::new();
    pool.upload_on_device(&gpu.device, &gpu.queue, "effects", &layer.instances);
    let mut replay = |serial: bool| -> (usize, Vec<Vec<u8>>) {
        SERIAL.set(serial);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        clear(
            &mut encoder,
            &cv,
            &dv,
            0,
            wgpu::LoadOp::Clear(wgpu::Color {
                r: 0.25,
                g: 0.3,
                b: 0.2,
                a: 1.0,
            }),
        );
        gpu.queue.submit([encoder.finish()]);
        terrain.prepare(&gpu.device, color, &dv, batch.camera_uniform());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        draw_native_object_pass(
            &mut encoder,
            ObjectDrawPhase::Display,
            &cv,
            &dv,
            &mut terrain,
            [0, 0, SIZE[0], SIZE[1]],
            &batch,
            pool.get("effects"),
            layer,
            None,
            None,
            None,
            Some(&atlas),
            None,
            batch.default_zshape_bind_group(),
            None,
            Some(&lights),
        );
        SERIAL.set(false);
        let reads = [
            gpu.read(&mut encoder, color),
            gpu.read(&mut encoder, &depth),
        ];
        let passes = terrain.closed_passes();
        (passes, gpu.finish(encoder, &reads, SIZE))
    };
    let (serial, expected) = replay(true);
    let (staged, output) = replay(false);
    assert!(
        output[0] == expected[0],
        "colour equals strict parent order"
    );
    assert!(output[1] == expected[1], "Z equals strict parent order");
    // Strict order: per group one light pass and a snapshot/blend pair, with
    // each later sprite joining the next building's pass.
    assert_eq!(serial, 12 * 4 + 1);
    // Staged: buildings, all lights, all stores (one pair), later sprites.
    // The offscreen light and the unsupported index draw nothing.
    assert_eq!(staged, 1 + 1 + 2 + 1);
}
