//! Final-window FPS readout, hidden at every launch until Ctrl+Alt+F or the
//! Options page's FPS Counter box shows it.
//! Independent of tactical cameras, optional dev overlays, retail fonts, and
//! shell/offscreen composition.

use std::sync::Mutex;
use std::time::Instant;

use crate::render::batch::{BatchRenderer, CameraUniform, InstanceBufferPool};
use crate::render::bit_font::BitFont;
use crate::render::gpu::GpuContext;

use super::state::FrameTimer;

pub(crate) struct FpsOverlay {
    visible: bool,
    font: BitFont,
    camera_buffer: wgpu::Buffer,
    camera_group: wgpu::BindGroup,
    frame: Mutex<OverlayFrame>,
}

struct OverlayFrame {
    timer: FrameTimer,
    pool: InstanceBufferPool,
    size: [u32; 2],
    depth: wgpu::TextureView,
    logged_first_frame: bool,
    simulation: Option<crate::app::presentation::interpolation::SimulationReadout>,
}

impl FpsOverlay {
    pub(crate) fn new(gpu: &GpuContext, batch: &BatchRenderer) -> Self {
        let (camera_buffer, camera_group) = batch.create_ui_camera(&gpu.device);
        Self {
            visible: false,
            font: BitFont::fallback_5x7(&gpu.device, &gpu.queue, batch),
            camera_buffer,
            camera_group,
            frame: Mutex::new(OverlayFrame {
                timer: FrameTimer::new(),
                pool: InstanceBufferPool::new(),
                size: [gpu.config.width, gpu.config.height],
                depth: gpu.create_depth_texture(),
                logged_first_frame: false,
                simulation: None,
            }),
        }
    }

    pub(crate) fn set_simulation_readout(
        &self,
        simulation: Option<crate::app::presentation::interpolation::SimulationReadout>,
    ) {
        self.frame
            .lock()
            .expect("FPS overlay frame lock")
            .simulation = simulation;
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.visible
    }

    /// The Options page's FPS Counter box.
    pub(crate) fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Ctrl+Alt+F: show or hide the readout; returns whether it is now shown.
    pub(crate) fn toggle(&mut self) -> bool {
        self.visible = !self.visible;
        self.visible
    }

    /// Call exactly once after all content has reached the acquired window
    /// surface, including synchronous loading repaints, before submission.
    /// The dedicated camera avoids rewriting uniforms used by earlier passes.
    pub(crate) fn encode(
        &self,
        gpu: &GpuContext,
        batch: &BatchRenderer,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::Texture,
    ) {
        let mut frame = self.frame.lock().expect("FPS overlay frame lock");
        // Keep measuring while hidden, so the first shown frame has a rate.
        frame.timer.sample(Instant::now());
        if !self.visible {
            return;
        }
        let text = simulation_readout_text(frame.timer.frame_ms_mean(), frame.simulation);
        let size = [destination.width(), destination.height()];
        if frame.size != size {
            frame.depth = gpu.create_depth_texture();
            frame.size = size;
        }
        let camera = CameraUniform {
            screen_size: [size[0] as f32, size[1] as f32],
            camera_pos: [0.0; 2],
            zoom: 1.0,
            world_origin_y: 0.0,
            world_height: 1.0,
            _pad: 0.0,
            native_z_origin_y: 0.0,
            _native_z_pad: 0.0,
        };
        gpu.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera));
        let mut instances = Vec::with_capacity(text.len() * 5);
        for (dx, dy, tint) in [
            (-1.0, 0.0, [0.0; 3]),
            (1.0, 0.0, [0.0; 3]),
            (0.0, -1.0, [0.0; 3]),
            (0.0, 1.0, [0.0; 3]),
            (0.0, 0.0, [1.0, 1.0, 0.25]),
        ] {
            instances.extend(self.font.build_text(
                &text,
                8.0 + dx,
                8.0 + dy,
                2.0,
                0.0,
                tint,
                [0.0; 2],
            ));
        }
        frame.pool.upload(gpu, "window_fps", &instances);
        if !frame.logged_first_frame {
            log::info!(
                "FPS overlay: final window surface {}x{}, {} glyph quads",
                size[0],
                size[1],
                instances.len()
            );
            frame.logged_first_frame = true;
        }
        let view = destination.create_view(&wgpu::TextureViewDescriptor::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Window FPS"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &frame.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if let Some((buffer, count)) = frame.pool.get("window_fps") {
            batch.draw_passthrough_range_bound(
                &mut pass,
                self.font.atlas(),
                buffer,
                &self.camera_group,
                0,
                count,
            );
        }
    }
}

fn readout_text(frame_ms: f32) -> String {
    if !frame_ms.is_finite() || frame_ms <= 0.0 {
        "FPS --".to_owned()
    } else {
        // The built-in font is uppercase-only and has no decimal point.
        format!("FPS {:.0}  {:.0} MS", 1000.0 / frame_ms, frame_ms)
    }
}

fn simulation_readout_text(
    frame_ms: f32,
    simulation: Option<crate::app::presentation::interpolation::SimulationReadout>,
) -> String {
    let mut text = readout_text(frame_ms);
    if let Some(simulation) = simulation {
        text.push_str(&format!(
            "  SIM {:.0} TPS  INTERP {}",
            simulation.tps,
            if simulation.interpolation && simulation.sampling {
                "ON"
            } else if simulation.interpolation {
                "BYPASS"
            } else {
                "OFF"
            },
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fps_overlay_distinguishes_present_rate_from_logic_and_interpolation() {
        let readout = crate::app::presentation::interpolation::SimulationReadout {
            tps: 30.0,
            interpolation: true,
            sampling: true,
        };
        assert_eq!(
            simulation_readout_text(1000.0 / 60.0, Some(readout)),
            "FPS 60  17 MS  SIM 30 TPS  INTERP ON"
        );
        assert!(
            simulation_readout_text(
                16.0,
                Some(crate::app::presentation::interpolation::SimulationReadout {
                    sampling: false,
                    ..readout
                })
            )
            .ends_with("INTERP BYPASS")
        );
        let supported = crate::render::bit_font::fallback_5x7_glyphs();
        for (interpolation, sampling) in [(true, true), (false, false), (true, false)] {
            for ch in simulation_readout_text(
                16.0,
                Some(crate::app::presentation::interpolation::SimulationReadout {
                    interpolation,
                    sampling,
                    ..readout
                }),
            )
            .chars()
            {
                assert!(
                    supported.iter().any(|(glyph, _)| *glyph == ch),
                    "missing {ch}"
                );
            }
        }
    }

    #[test]
    fn fps_overlay_has_a_visible_label_before_measurement() {
        for value in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert_eq!(readout_text(value), "FPS --");
        }
    }

    #[test]
    fn fps_overlay_reports_render_rate_not_simulation_ticks() {
        assert_eq!(readout_text(1000.0 / 60.0), "FPS 60  17 MS");
        assert_eq!(readout_text(100.0), "FPS 10  100 MS");
    }

    #[test]
    fn fps_overlay_font_covers_every_readout_character() {
        let supported = crate::render::bit_font::fallback_5x7_glyphs();
        for frame_ms in [0.0, 0.5, 1000.0 / 60.0, 100.0, 1234.0] {
            for ch in readout_text(frame_ms).chars() {
                assert!(
                    supported.iter().any(|(glyph, _)| *glyph == ch),
                    "missing {ch}"
                );
            }
        }
    }
}
