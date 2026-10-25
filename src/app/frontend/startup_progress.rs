//! Horizon-only progress surface for synchronous process startup.

use anyhow::{Context, Result};

use crate::render::batch::{BatchRenderer, BatchTexture, DepthAxis, SpriteInstance};
use crate::render::gpu::GpuContext;
use crate::render::shell_surface_present::ShellSurfacePresenter;

const TRACK_COLOR: [f32; 3] = [0.02, 0.12, 0.9];
const FILL_COLOR: [f32; 3] = [0.9, 0.025, 0.035];
const BAR_HEIGHT: u32 = 18;

pub(crate) struct StartupProgressPresentation {
    white_texture: BatchTexture,
}

impl StartupProgressPresentation {
    pub(crate) fn new(gpu: &GpuContext, batch: &BatchRenderer) -> Self {
        Self {
            white_texture: batch.create_texture(gpu, &[255; 4], 1, 1),
        }
    }

    /// Compose into the shared sRGB shell target, then present through the
    /// shell presenter so unorm Horizon swapchains receive encoded bytes.
    pub(crate) fn render_and_present(
        &self,
        gpu: &GpuContext,
        batch: &BatchRenderer,
        presenter: &ShellSurfacePresenter,
        depth: &wgpu::TextureView,
        fraction: f32,
    ) -> Result<bool> {
        let width = gpu.config.width.max(1);
        let height = gpu.config.height.max(1);
        let bar_width = (width * 3 / 4).clamp(1, 960);
        let bar_height = BAR_HEIGHT.min(height);
        let x = (width - bar_width) / 2;
        let y = (height - bar_height) / 2;
        let fill_width = (bar_width as f32 * fraction.clamp(0.0, 1.0)).round() as u32;

        let mut instances = vec![SpriteInstance {
            position: [x as f32, y as f32],
            size: [bar_width as f32, bar_height as f32],
            tint: TRACK_COLOR,
            alpha: 1.0,
            ..Default::default()
        }];
        if fill_width > 0 {
            instances.push(SpriteInstance {
                position: [x as f32, y as f32],
                size: [fill_width.min(bar_width) as f32, bar_height as f32],
                tint: FILL_COLOR,
                alpha: 1.0,
                ..Default::default()
            });
        }
        let (instance_buffer, count) = batch
            .create_instance_buffer(gpu, &instances)
            .context("create startup progress bar instances")?;

        batch.update_camera(
            gpu,
            width as f32,
            height as f32,
            0.0,
            0.0,
            1.0,
            DepthAxis::NONE,
        );
        let Some(output) = gpu
            .acquire_frame()
            .context("acquire startup progress surface")?
        else {
            return Ok(false);
        };
        let source_view = presenter.source_render_view();
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Startup Progress"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Startup Progress Composition"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &source_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
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
            batch.draw_with_buffer_ui_passthrough(
                &mut pass,
                &self.white_texture,
                &instance_buffer,
                count,
            );
        }
        presenter.encode_present(&mut encoder, &output.texture);
        gpu.queue.submit(std::iter::once(encoder.finish()));
        crate::render::present::present(output);
        Ok(true)
    }
}
