//! Final-window FPS panel, hidden at every launch until Ctrl+Alt+F or the
//! Options page's FPS Counter box shows it, and the replay/recovery status
//! line below it.
//!
//! The panel reads like the game's message line: GAME.FNT rows on black
//! boxes, centered at the top of the battlefield at the match zoom, or of the
//! window elsewhere. Its rows are in [`panel`]: rates and units, the engine
//! areas that took the most of the last second, the load on each core and
//! the newest logged warning or error. Before GAME.FNT loads, a built-in font
//! draws the first row alone. The status line is localized text
//! ([`crate::i18n`]), so it draws with the caller's GAME.FNT font. Both are
//! independent of tactical cameras, optional dev overlays, and
//! shell/offscreen composition.

mod panel;

use crate::util::time::Instant;
use std::sync::Mutex;

use crate::render::batch::{
    BatchRenderer, BatchTexture, CameraUniform, InstanceBufferPool, SpriteInstance,
};
use crate::render::bit_font::BitFont;
use crate::render::gpu::GpuContext;

use super::state::FrameTimer;
use panel::{GREEN, HOT, PanelStats, Part, TRACK, WARM};

/// The status line's top when the panel is hidden.
const STATUS_TOP: f32 = 8.0;
/// A core bar, in font pixels: its width, the gap to the next, its height and
/// its drop below the row's text top.
const BAR_WIDTH: f32 = 6.0;
const BAR_GAP: f32 = 2.0;
const BAR_HEIGHT: f32 = 12.0;
const BAR_DROP: f32 = 2.0;
const ELLIPSIS: &str = "...";

pub(crate) struct FpsOverlay {
    visible: bool,
    font: BitFont,
    /// One white texel, tinted into the row boxes and the core bars.
    white: BatchTexture,
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
    stats: PanelStats,
    /// Where the status line starts: below the panel while it shows.
    status_top: f32,
}

/// Where the panel draws, in window pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelPlacement {
    /// The x the rows center on.
    pub(crate) center_x: f32,
    /// The top of the first row's box.
    pub(crate) top: f32,
    /// Window pixels per font pixel: the match zoom, as the game's messages.
    pub(crate) scale: f32,
    /// The widest a row may draw; a longer row loses its end.
    pub(crate) max_width: f32,
}

impl PanelPlacement {
    /// Loading and menu screens: the window's top center at its own pixels.
    fn window([width, _]: [u32; 2]) -> Self {
        Self {
            center_x: width as f32 / 2.0,
            top: 0.0,
            scale: 1.0,
            max_width: width.saturating_sub(16) as f32,
        }
    }
}

/// A match the panel reads its unit count and Most CPU areas from.
pub(crate) struct MatchPanel<'a> {
    pub(crate) placement: PanelPlacement,
    pub(crate) simulation: Option<&'a crate::sim::world::Simulation>,
}

/// The panel over the battlefield of the match on screen, if one is.
pub(crate) fn match_panel(state: &crate::app::AppState) -> Option<MatchPanel<'_>> {
    if state.frontend.screen != crate::ui::game_screen::GameScreen::InGame {
        return None;
    }
    let (tactical_width, _) = crate::app::input::camera::tactical_viewport_size(state);
    let (scale, (left, top)) = state.tactical_zoom().map_or((1.0, (0, 0)), |zoom| {
        (zoom.percent as f32 / 100.0, zoom.origin)
    });
    let width = tactical_width as f32 * scale;
    Some(MatchPanel {
        placement: PanelPlacement {
            center_x: left as f32 + width / 2.0,
            top: top as f32,
            scale,
            max_width: (width - 16.0).max(0.0),
        },
        simulation: state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|runtime| &runtime.simulation),
    })
}

impl FpsOverlay {
    pub(crate) fn new(gpu: &GpuContext, batch: &BatchRenderer) -> Self {
        let (camera_buffer, camera_group) = batch.create_ui_camera(&gpu.device);
        Self {
            visible: false,
            font: BitFont::fallback_5x7(&gpu.device, &gpu.queue, batch),
            white: batch.create_texture(gpu, &[255; 4], 1, 1),
            camera_buffer,
            camera_group,
            frame: Mutex::new(OverlayFrame {
                timer: FrameTimer::new(),
                pool: InstanceBufferPool::new(),
                size: [gpu.config.width, gpu.config.height],
                depth: gpu.create_depth_texture(),
                logged_first_frame: false,
                simulation: None,
                stats: PanelStats::default(),
                status_top: STATUS_TOP,
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

    /// One finished in-game render's phase spans, for the Most CPU row.
    pub(crate) fn record_render_phases(
        &self,
        phases: crate::app::presentation::render::RenderPhases,
    ) {
        if self.visible {
            self.frame
                .lock()
                .expect("FPS overlay frame lock")
                .stats
                .record_render(phases);
        }
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.visible
    }

    /// The Options page's FPS Counter box.
    pub(crate) fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Ctrl+Alt+F: show or hide the panel; returns whether it is now shown.
    pub(crate) fn toggle(&mut self) -> bool {
        self.visible = !self.visible;
        self.visible
    }

    /// Call exactly once after all content has reached the acquired window
    /// surface, including synchronous loading repaints, before submission.
    /// `font` is the process GAME.FNT font; `in_match` places the panel over
    /// a match's battlefield. The dedicated camera avoids rewriting uniforms
    /// used by earlier passes.
    pub(crate) fn encode(
        &self,
        gpu: &GpuContext,
        batch: &BatchRenderer,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::Texture,
        font: &BitFont,
        in_match: Option<MatchPanel<'_>>,
    ) {
        let mut frame = self.frame.lock().expect("FPS overlay frame lock");
        let now = Instant::now();
        // Keep measuring while hidden, so the first shown frame has a rate.
        frame.timer.sample(now);
        if !self.visible {
            frame.stats = PanelStats::default();
            frame.status_top = STATUS_TOP;
            return;
        }
        let size = [destination.width(), destination.height()];
        let (placement, simulation) = match in_match {
            Some(panel) => (panel.placement, panel.simulation),
            None => (PanelPlacement::window(size), None),
        };
        if frame.stats.due(now) {
            let frame_ms = frame.timer.frame_ms_mean();
            let readout = frame.simulation;
            frame.stats.close_window(
                now,
                frame_ms,
                readout,
                simulation.map(match_sample),
                crate::util::logging::latest_problem(),
            );
        }
        // The 5x7 fallback has no darken strip: GAME.FNT has not loaded yet.
        let (font, scale, mut rows) = if font.darken_texture().is_some() {
            (font, placement.scale, panel::rows(&frame.stats.shown))
        } else {
            (
                &self.font,
                placement.scale * 2.0,
                panel::fallback_rows(&frame.stats.shown),
            )
        };
        let mut instances = Vec::new();
        let mut glyphs = Vec::new();
        let bottom = layout_rows(
            font,
            &mut rows,
            PanelPlacement { scale, ..placement },
            &mut instances,
            &mut glyphs,
        );
        frame.status_top = bottom + 4.0;
        let solid = instances.len() as u32;
        instances.append(&mut glyphs);
        if !frame.logged_first_frame {
            log::info!(
                "FPS panel: final window surface {}x{}, {} quads",
                size[0],
                size[1],
                instances.len()
            );
            frame.logged_first_frame = true;
        }
        let total = instances.len() as u32;
        self.draw(
            gpu,
            batch,
            encoder,
            destination,
            &mut frame,
            "window_fps",
            &instances,
            &[
                (&self.white, 0, solid),
                (font.atlas(), solid, total - solid),
            ],
        );
    }

    /// Persistent replay/recovery facts share the final-window camera. `font`
    /// is the process GAME.FNT font, which covers localized text.
    /// This call neither samples nor changes the FPS panel's visibility.
    pub(crate) fn encode_status(
        &self,
        gpu: &GpuContext,
        batch: &BatchRenderer,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::Texture,
        font: &BitFont,
        text: &str,
    ) {
        let mut frame = self.frame.lock().expect("window status overlay frame lock");
        let top = if self.visible {
            frame.status_top
        } else {
            STATUS_TOP
        };
        let mut instances = Vec::with_capacity(text.len() * 5);
        let layout = font.wrap_layout(text, destination.width().saturating_sub(16));
        let line_height = font.cell_height() + 4.0;
        for (line_index, line) in layout.lines.iter().enumerate() {
            for (dx, dy, tint) in [
                (-1.0, 0.0, [0.0; 3]),
                (1.0, 0.0, [0.0; 3]),
                (0.0, -1.0, [0.0; 3]),
                (0.0, 1.0, [0.0; 3]),
                (0.0, 0.0, [1.0, 1.0, 0.25]),
            ] {
                instances.extend(font.build_text(
                    &text[line.start_byte..line.end_byte],
                    8.0 + dx,
                    top + line_index as f32 * line_height + dy,
                    1.0,
                    0.0,
                    tint,
                    [0.0; 2],
                ));
            }
        }
        let count = instances.len() as u32;
        self.draw(
            gpu,
            batch,
            encoder,
            destination,
            &mut frame,
            "window_status",
            &instances,
            &[(font.atlas(), 0, count)],
        );
    }

    /// Upload `instances` and draw each `(texture, start, count)` range of
    /// them in order over the window surface.
    #[allow(clippy::too_many_arguments)]
    fn draw(
        &self,
        gpu: &GpuContext,
        batch: &BatchRenderer,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::Texture,
        frame: &mut OverlayFrame,
        key: &'static str,
        instances: &[SpriteInstance],
        ranges: &[(&BatchTexture, u32, u32)],
    ) {
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
        frame.pool.upload(gpu, key, instances);
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
            multiview_mask: None,
        });
        // The passthrough pipeline compares Always and writes no depth, so
        // the ranges paint in order: boxes and bars under the text.
        if let Some((buffer, _)) = frame.pool.get(key) {
            for &(texture, start, count) in ranges {
                batch.draw_passthrough_range_bound(
                    &mut pass,
                    texture,
                    buffer,
                    &self.camera_group,
                    start,
                    count,
                );
            }
        }
    }
}

fn match_sample(simulation: &crate::sim::world::Simulation) -> panel::MatchSample {
    let entities = simulation.entities();
    let structures = entities
        .values_of_category(crate::map::entities::EntityCategory::Structure)
        .count();
    panel::MatchSample {
        units: entities.len().saturating_sub(structures),
        profile: simulation.frame_profile().totals_ns(),
    }
}

/// Lay `rows` out down from `placement.top`, each centered on its black box
/// as the game's message line draws them, and return the bottom edge. Box and
/// bar quads go to `solid`, text to `glyphs`.
fn layout_rows(
    font: &BitFont,
    rows: &mut [Vec<Part>],
    placement: PanelPlacement,
    solid: &mut Vec<SpriteInstance>,
    glyphs: &mut Vec<SpriteInstance>,
) -> f32 {
    let scale = placement.scale;
    let row_height = (font.cell_height() + 2.0) * scale;
    let mut box_top = placement.top.round();
    for row in rows.iter_mut() {
        fit_row(font, row, placement.max_width / scale);
        let width = row_width(font, row) * scale;
        let left = (placement.center_x - width / 2.0).round().max(scale);
        solid.push(quad(
            [left - scale, box_top],
            [width + 2.0 * scale, row_height],
            [0; 3],
        ));
        let text_top = box_top + scale;
        let mut x = left;
        for part in row.iter() {
            match part {
                Part::Text(text, rgb) => {
                    glyphs.extend(font.build_text(
                        text,
                        x,
                        text_top,
                        scale,
                        0.0,
                        tint(*rgb),
                        [0.0; 2],
                    ));
                    x += font.text_width(text) as f32 * scale;
                }
                Part::Bars(loads) => {
                    let bar_top = text_top + BAR_DROP * scale;
                    for (index, &load) in loads.iter().enumerate() {
                        let bar_x = x + index as f32 * (BAR_WIDTH + BAR_GAP) * scale;
                        let size = [BAR_WIDTH * scale, BAR_HEIGHT * scale];
                        solid.push(quad([bar_x, bar_top], size, TRACK));
                        let filled = (BAR_HEIGHT * load.clamp(0.0, 100.0) / 100.0)
                            .round()
                            .max(1.0);
                        let color = if load >= 90.0 {
                            HOT
                        } else if load >= 75.0 {
                            WARM
                        } else {
                            GREEN
                        };
                        solid.push(quad(
                            [bar_x, bar_top + (BAR_HEIGHT - filled) * scale],
                            [size[0], filled * scale],
                            color,
                        ));
                    }
                    x += bars_width(loads.len()) * scale;
                }
            }
        }
        box_top += row_height;
    }
    box_top
}

/// A row's width in font pixels.
fn row_width(font: &BitFont, row: &[Part]) -> f32 {
    row.iter()
        .map(|part| match part {
            Part::Text(text, _) => font.text_width(text) as f32,
            Part::Bars(loads) => bars_width(loads.len()),
        })
        .sum()
}

/// `count` core bars side by side, in font pixels.
fn bars_width(count: usize) -> f32 {
    (count as f32 * (BAR_WIDTH + BAR_GAP) - BAR_GAP).max(0.0)
}

/// Cut the end of a row wider than `max_width` font pixels: its last text
/// loses characters behind an ellipsis.
fn fit_row(font: &BitFont, row: &mut [Part], max_width: f32) {
    let width = row_width(font, row);
    if width <= max_width {
        return;
    }
    let Some(Part::Text(text, _)) = row.last_mut() else {
        return;
    };
    let mut keep =
        max_width - (width - font.text_width(text) as f32) - font.text_width(ELLIPSIS) as f32;
    let mut buffer = [0u8; 4];
    let end = text
        .char_indices()
        .find(|&(_, ch)| {
            keep -= font.text_width(ch.encode_utf8(&mut buffer)) as f32;
            keep < 0.0
        })
        .map_or(text.len(), |(index, _)| index);
    text.truncate(end);
    text.push_str(ELLIPSIS);
}

fn quad(position: [f32; 2], size: [f32; 2], rgb: [u8; 3]) -> SpriteInstance {
    SpriteInstance {
        position,
        size,
        uv_origin: [0.0; 2],
        uv_size: [1.0; 2],
        depth: 0.0,
        tint: tint(rgb),
        alpha: 1.0,
        ..Default::default()
    }
}

/// The batch shader writes tints straight to the 8-bit surface.
fn tint(rgb: [u8; 3]) -> [f32; 3] {
    rgb.map(|channel| f32::from(channel) / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_too_wide_for_the_battlefield_loses_the_end_of_its_last_text() {
        let font = crate::render::bit_font::tests::make_test_font(
            &[(u16::from(b'a'), 4), (u16::from(b'.'), 1)],
            3,
        );
        let mut row = vec![
            Part::Text("aa".to_owned(), GREEN),
            Part::Bars(vec![50.0]),
            Part::Text("aaaaaaaa".to_owned(), WARM),
        ];
        let fits = row_width(&font, &row) - 9.0;
        fit_row(&font, &mut row, fits);
        assert!(row_width(&font, &row) <= fits);
        let Part::Text(text, _) = &row[2] else {
            panic!("the last part stays text");
        };
        assert!(text.ends_with(ELLIPSIS) && text.len() < "aaaaaaaa...".len());
    }
}
