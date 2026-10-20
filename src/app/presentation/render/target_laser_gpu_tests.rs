//! A Unit's tail stores (the target laser, `UnitClass::Draw_It 0x0073D43A`)
//! through its retained Ground Display slot, the shared SurfaceLines snapshot
//! and real GPU output: XSurface's opaque words land over any Z, and Z keeps
//! its words.

use crate::app::presentation::render::draw_plan_lowering::{
    NativeDisplayOrder, PlannedObjectInstance, lower_object_instances,
};
use crate::app::presentation::render::merge_passes::draw_native_object_pass;
use crate::render::batch::BatchRenderer;
use crate::render::surface_line::{SurfaceDraw, SurfaceLineViewport};
use crate::render::tactical_draw_plan::SpriteEncoding;
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, encoded, seed_depth_grid};
use crate::sim::world::display_layers::{DisplayLayer, DisplayLayers};

#[test]
#[ignore = "requires GPU; tail stores through the Ground Display slot"]
fn tail_stores_reach_the_gpu_over_any_z_without_writing_it() {
    const SIZE: [u32; 2] = [32, 16];
    const BACKGROUND: u16 = 0x001F;
    const LASER: u16 = 0xF800;
    let gpu = Gpu::new();
    // A 3x3 endpoint box and a [1,0] dash along row 8, across a Z boundary:
    // the left half nearest the viewer, the right half farthest.
    let mut draws = vec![SurfaceDraw::OpaqueRect {
        rect: [2, 2, 3, 3],
        word: LASER,
    }];
    draws.extend((4..28).step_by(2).map(|x| SurfaceDraw::OpaqueRect {
        rect: [x, 8, 1, 1],
        word: LASER,
    }));
    let z: Vec<u16> = (0..SIZE[0] * SIZE[1])
        .map(|i| if i % SIZE[0] < SIZE[0] / 2 { 0 } else { 0xFFFF })
        .collect();
    let mut expected = vec![BACKGROUND; z.len()];
    for draw in &draws {
        let SurfaceDraw::OpaqueRect {
            rect: [x, y, w, h], ..
        } = *draw
        else {
            unreachable!()
        };
        for py in y..y + h {
            for px in x..x + w {
                expected[(py * SIZE[0] as i32 + px) as usize] = LASER;
            }
        }
    }
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        batch.write_camera(&gpu.queue, camera(SIZE));
        let color = gpu.target(SIZE, format);
        let cv = color.create_view(&Default::default());
        let depth = gpu.target(SIZE, wgpu::TextureFormat::Depth32Float);
        let dv = depth.create_view(&Default::default());
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        terrain.prepare(&gpu.device, &color, &dv, batch.camera_uniform());
        terrain.begin_surface_lines();
        let mut display = DisplayLayers::default();
        assert!(display.submit(7, Some(DisplayLayer::GROUND), &|_| 0));
        let order = NativeDisplayOrder::from_display(&display);
        let mut unit = PlannedObjectInstance::object(
            order.object_draw(7, SpriteEncoding::Plain).unwrap(),
            Vec::new(),
        );
        unit.tail_draws = draws.clone();
        let mut layers = lower_object_instances(vec![unit]);
        let ground = &mut layers[2];
        assert_eq!(ground.surface_draws, [draws.clone()]);
        for stores in &ground.surface_draws {
            ground.line_batches.push(terrain.prepare_surface_lines(
                &gpu.device,
                &gpu.queue,
                std::iter::empty(),
                stores.iter().copied(),
                &[],
                &[],
                &[],
                SurfaceLineViewport {
                    camera: [0; 2],
                    clip: [0, 0, SIZE[0] as i32, SIZE[1] as i32],
                    z_origin_y: 0,
                    zoom: 1.,
                },
                |_| false,
                |_| panic!("opaque stores never query A"),
            ));
        }
        let initial = std::iter::repeat_n(encoded(BACKGROUND, format), z.len())
            .flatten()
            .collect::<Vec<_>>();
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &color,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &initial,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIZE[0] * 4),
                rows_per_image: Some(SIZE[1]),
            },
            color.size(),
        );
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        seed_depth_grid(&gpu, &mut encoder, &dv, &z, SIZE[0]);
        draw_native_object_pass(
            &mut encoder,
            crate::app::render::merge_passes::ObjectDrawPhase::Display,
            &cv,
            &dv,
            &mut terrain,
            [0, 0, SIZE[0], SIZE[1]],
            &batch,
            None,
            ground,
            None,
            None,
            None,
            None,
            None,
            batch.default_zshape_bind_group(),
            None,
            None,
        );
        let reads = [
            gpu.read(&mut encoder, &color),
            gpu.read(&mut encoder, &depth),
        ];
        let output = gpu.finish(encoder, &reads, SIZE);
        assert_eq!(
            output[0],
            expected
                .iter()
                .flat_map(|&word| encoded(word, format))
                .collect::<Vec<_>>(),
            "{format:?}: the stores over both Z halves"
        );
        assert_eq!(
            output[1],
            z.iter()
                .flat_map(|&word| crate::render::native_z::stored_depth(word).to_le_bytes())
                .collect::<Vec<_>>(),
            "{format:?}: Z unchanged"
        );
    }
}
