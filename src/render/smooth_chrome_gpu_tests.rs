//! VERA's smooth online client (`BatchRenderer::draw_chrome`), read back from
//! the production pipeline. Expected bytes follow from the shader's own rules:
//! the box filter and 40% of xBR's rounding, on square-root-encoded colours.

use super::batch::{BatchRenderer, DepthAxis, SpriteInstance};
use super::frame_readback::PendingTextureReadback;
use crate::render::gpu::block_on;
use std::time::Duration;
use wgpu::util::DeviceExt;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const CLEAR: [u8; 4] = [0; 4];
const WHITE: [u8; 4] = [255; 4];
const BLACK: [u8; 4] = [0, 0, 0, 255];

struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    batch: BatchRenderer,
}

impl Renderer {
    fn new() -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("this test requires a wgpu adapter");
        let (device, queue) =
            block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let batch = BatchRenderer::new_with_device(&device, &queue, FORMAT);
        Self {
            device,
            queue,
            batch,
        }
    }

    /// `page`'s RGBA pixels drawn by `quad` over black; the window's RGBA.
    fn draw(
        &self,
        page: &[[u8; 4]],
        page_size: [u32; 2],
        quad: SpriteInstance,
        window: [u32; 2],
        smooth: bool,
    ) -> Vec<[u8; 4]> {
        let target = |format, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: window[0],
                    height: window[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color = target(
            FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = target(
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let (color_view, depth_view) = (
            color.create_view(&Default::default()),
            depth.create_view(&Default::default()),
        );
        let texture = self.batch.create_texture_on_device(
            &self.device,
            &self.queue,
            page.as_flattened(),
            page_size[0],
            page_size[1],
            None,
        );
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::bytes_of(&quad),
                usage: wgpu::BufferUsages::VERTEX,
            });
        self.batch.update_camera_on_queue(
            &self.queue,
            window[0] as f32,
            window[1] as f32,
            0.0,
            0.0,
            1.0,
            DepthAxis::NONE,
        );
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
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
        self.batch
            .draw_chrome(&mut pass, &texture, &buffer, 1, smooth);
        drop(pass);
        let readback = PendingTextureReadback::encode(
            &self.device,
            &mut encoder,
            &color,
            FORMAT,
            window[0],
            window[1],
        )
        .unwrap();
        let submission = self.queue.submit([encoder.finish()]);
        readback
            .finish(&self.device, submission, Duration::from_secs(20))
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| [p[2], p[1], p[0], p[3]])
            .collect()
    }
}

fn quad(size: [f32; 2], uv_origin: [f32; 2], uv_size: [f32; 2]) -> SpriteInstance {
    SpriteInstance {
        size,
        uv_origin,
        uv_size,
        tint: [1.0; 3],
        alpha: 1.0,
        ..Default::default()
    }
}

fn close(actual: [u8; 4], expected: [u8; 3]) -> bool {
    (0..3).all(|channel| actual[channel].abs_diff(expected[channel]) <= 2)
}

#[test]
#[ignore = "requires a wgpu adapter"]
fn a_quad_at_its_own_size_draws_the_same_smoothed() {
    let renderer = Renderer::new();
    let page = [RED, BLUE, GREEN, CLEAR, WHITE, BLACK, BLUE, RED, GREEN];
    let draw = |smooth| {
        renderer.draw(
            &page,
            [3, 3],
            quad([3.0, 3.0], [0.0, 0.0], [1.0, 1.0]),
            [3, 3],
            smooth,
        )
    };
    assert_eq!(draw(true), draw(false));
}

#[test]
#[ignore = "requires a wgpu adapter"]
fn an_enlarged_quad_averages_what_it_straddles_inside_its_own_rectangle() {
    let renderer = Renderer::new();
    // Two pixels of a page, drawn 2.5 times as wide. The third screen pixel
    // straddles them; green outside the rectangle never reaches the screen.
    let two_pixels = |page: &[[u8; 4]]| {
        renderer.draw(
            page,
            [4, 1],
            quad([5.0, 1.0], [0.25, 0.0], [0.5, 1.0]),
            [5, 1],
            true,
        )
    };
    let shown = two_pixels(&[GREEN, RED, BLUE, GREEN]);
    assert!(close(shown[0], [255, 0, 0]) && close(shown[1], [255, 0, 0]));
    // Half each on square roots: 0.25 of each in light, stored as 137.
    assert!(close(shown[2], [137, 0, 137]), "{:?}", shown[2]);
    assert!(close(shown[3], [0, 0, 255]) && close(shown[4], [0, 0, 255]));
    // Beside a clear pixel the colour stays red and only its cover halves:
    // half of red's light, stored as 188, not a darkened fringe.
    let shown = two_pixels(&[GREEN, RED, CLEAR, GREEN]);
    assert!(close(shown[1], [255, 0, 0]));
    assert!(close(shown[2], [188, 0, 0]), "{:?}", shown[2]);
    assert!(close(shown[4], [0, 0, 0]));
}

#[test]
#[ignore = "requires a wgpu adapter"]
fn an_enlarged_staircase_rounds_each_corner_by_forty_percent() {
    let renderer = Renderer::new();
    let page: Vec<[u8; 4]> = (0..64)
        .map(|index| {
            if index % 8 + index / 8 < 8 {
                WHITE
            } else {
                BLACK
            }
        })
        .collect();
    let draw = |smooth| {
        renderer.draw(
            &page,
            [8, 8],
            quad([24.0, 24.0], [0.0, 0.0], [1.0, 1.0]),
            [24, 24],
            smooth,
        )
    };
    let at = |shown: &[[u8; 4]], x: usize, y: usize| shown[y * 24 + x];
    let shown = draw(true);
    // Source pixel (4, 3) is white with black to its right and below. The
    // diagonal crosses its corner sub-pixel 0.75 of the way to black: 1 - 0.4
    // x 0.75 = 0.7 on square roots, stored as 186; the middle stays white.
    assert!(
        close(at(&shown, 14, 11), [186; 3]),
        "{:?}",
        at(&shown, 14, 11)
    );
    assert!(close(at(&shown, 13, 10), [255; 3]));
    // The black pixel (5, 3) rounds toward white the same way: 0.3, as 85.
    assert!(close(at(&shown, 15, 9), [85; 3]), "{:?}", at(&shown, 15, 9));
    assert!(close(at(&shown, 16, 10), [0; 3]));
    // Blocks are what the option changes.
    let blocks = draw(false);
    assert!(close(at(&blocks, 14, 11), [255; 3]));
    assert!(close(at(&blocks, 15, 9), [0; 3]));
}
