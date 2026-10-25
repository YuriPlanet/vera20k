//! The ripple through the production admission and renderer against
//! `tools/superweapon_oracle.json` `ion_blast_draw`: the original DrawAll on
//! fixture surfaces (a 192x128 tactical view over 144 rows, background and Z
//! words and ripple bytes from the formulas below, which the oracle shares).

use super::*;
use crate::render::batch::BatchRenderer;
use crate::render::terrain_draw::TerrainDrawRenderer;
use crate::render::terrain_draw_gpu_tests::{Gpu, camera, encoded};
use crate::sim::ion_blast_ripple::FRAME_COUNT;
use crate::util::fnv::{FNV1A64_OFFSET_BASIS, fnv1a64_fold_bytes};
use serde_json::Value;

const VIEW: [u32; 2] = [192, 128];
const SIZE: [u32; 2] = [192, 144];
/// Every byte the generator writes (`ion_blast_ripple` frames' values).
const RIPPLE_BYTES: [u8; 8] = [0xff, 0x00, 4, 15, 34, 61, 96, 0x8b];

fn background(x: u32, y: u32) -> u16 {
    (x * 0x0123 + y * 0x0b57 + 0x1f) as u16
}

fn ripple_frames() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(FRAME_COUNT * 512 * 256);
    for n in 0..FRAME_COUNT {
        for sy in 0..256 {
            for sx in 0..512 {
                bytes.push(RIPPLE_BYTES[(sx * 3 + sy * 5 + n * 7) % RIPPLE_BYTES.len()]);
            }
        }
    }
    bytes
}

fn word(pixel: &[u8], format: wgpu::TextureFormat) -> u16 {
    let (r, b) = if format == wgpu::TextureFormat::Bgra8UnormSrgb {
        (pixel[2], pixel[0])
    } else {
        (pixel[0], pixel[2])
    };
    (u16::from(r >> 3) << 11) | (u16::from(pixel[1] >> 2) << 5) | u16::from(b >> 3)
}

/// The fixture Z: `z_origin - y`, offset by -2..2.
fn seed_depth(
    gpu: &Gpu,
    encoder: &mut wgpu::CommandEncoder,
    depth: &wgpu::TextureView,
    z_origin: i32,
) {
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("IonBlast Z fixture"),
            source: wgpu::ShaderSource::Wgsl(
                crate::render::tactical_shader::source(&format!(
                    "@vertex fn vs(@builtin(vertex_index) i:u32)->@builtin(position) vec4f {{ \
                 let p=array<vec2f,3>(vec2f(-1.,-1.),vec2f(3.,-1.),vec2f(-1.,3.)); \
                 return vec4f(p[i],0.,1.); }} \
                 @fragment fn fs(@builtin(position) p:vec4f)->@builtin(frag_depth) f32 {{ \
                 let x=i32(p.x); let y=i32(p.y); \
                 return stored_native_depth({z_origin} - y + (x*7 + y*3) % 5 - 2); }}"
                ))
                .into(),
            ),
        });
    let pipeline = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("IonBlast Z fixture"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("IonBlast Z fixture"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(&pipeline);
    pass.draw(0..3, 0..1);
}

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

/// Each row's blasts, view origin and Z drawn through [`ion_blast_draws`]
/// and the renderer at zoom 1: the destination words' FNV-1a and changed
/// count match the original's. VERA's world rows carry +15 over native's, so
/// the camera is the view origin 15 rows down.
#[test]
#[ignore = "requires GPU; original IonBlast DrawAll pixels on fixture surfaces"]
fn production_ripple_matches_original_draw_all() {
    let oracle: Value =
        serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap();
    let rows = oracle["ion_blast_draw"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    let gpu = Gpu::new();
    let frames = ripple_frames();
    let background: Vec<u16> = (0..SIZE[1])
        .flat_map(|y| (0..SIZE[0]).map(move |x| background(x, y)))
        .collect();
    for format in [
        wgpu::TextureFormat::Bgra8UnormSrgb,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, format);
        let mut terrain = TerrainDrawRenderer::new(&gpu.device, &gpu.queue, format, &batch);
        batch.write_camera(&gpu.queue, camera(SIZE));
        let color = gpu.target(SIZE, format);
        let color_view = color.create_view(&Default::default());
        let depth = gpu.target(SIZE, wgpu::TextureFormat::Depth32Float);
        let depth_view = depth.create_view(&Default::default());
        terrain.prepare(&gpu.device, &color, &depth_view, batch.camera_uniform());
        terrain.bind_ion_blast_frames_for_test(&gpu.device, &gpu.queue, &frames);
        let bytes: Vec<u8> = background
            .iter()
            .flat_map(|&word| encoded(word, format))
            .collect();
        for row in rows {
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &color,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIZE[0] * 4),
                    rows_per_image: Some(SIZE[1]),
                },
                color.size(),
            );
            let blasts: Vec<([i32; 3], i32)> = row["blasts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|blast| {
                    (
                        [int(&blast[0]), int(&blast[1]), int(&blast[2])],
                        int(&blast[3]),
                    )
                })
                .collect();
            let origin = &row["view_origin"];
            let draws = ion_blast_draws(
                blasts.into_iter(),
                [int(&origin[0]) as f32, (int(&origin[1]) + 15) as f32],
                VIEW.map(|v| v as f32),
                int(&row["detail"]) as u32,
            );
            terrain.prepare_ion_blasts(&gpu.device, &gpu.queue, &draws, 1.0);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            seed_depth(&gpu, &mut encoder, &depth_view, int(&row["z_origin"]));
            terrain.draw_ion_blasts(&mut encoder, &color_view);
            let reads = [gpu.read(&mut encoder, &color)];
            let output = gpu.finish(encoder, &reads, SIZE);
            let words: Vec<u16> = output[0]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| word(pixel, format))
                .collect();
            let changed = words
                .iter()
                .zip(&background)
                .filter(|(new, old)| new != old)
                .count();
            let digest = fnv1a64_fold_bytes(
                FNV1A64_OFFSET_BASIS,
                &words
                    .iter()
                    .flat_map(|w| w.to_le_bytes())
                    .collect::<Vec<_>>(),
            );
            let first: Vec<[u32; 4]> = words
                .iter()
                .zip(&background)
                .enumerate()
                .filter(|(_, (new, old))| new != old)
                .take(16)
                .map(|(i, (&new, &old))| {
                    [
                        i as u32 % SIZE[0],
                        i as u32 / SIZE[0],
                        u32::from(old),
                        u32::from(new),
                    ]
                })
                .collect();
            assert_eq!(
                (format!("{digest:#018x}"), changed),
                (
                    row["fnv1a64"].as_str().unwrap().to_owned(),
                    row["changed"].as_u64().unwrap() as usize
                ),
                "{format:?} {}: first changed {first:?}, native {}",
                row["name"],
                row["first_changed"]
            );
        }
    }
}
