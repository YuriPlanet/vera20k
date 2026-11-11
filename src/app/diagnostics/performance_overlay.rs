//! Final-window Performance Stats panel, hidden at every launch until
//! Ctrl+Alt+F or the Options page's Performance Stats box shows it, and the
//! replay/recovery status line below it.
//!
//! The panel reads like the game's message line: rows of text on black
//! boxes, at the top left of the battlefield at the match zoom, or of the
//! window elsewhere. While it shows, the status line and the match's message
//! list start below it. Its rows are in [`panel`]: rates and units, the
//! engine areas that took the most of the last second, the load on each core
//! and the GPU's share of each frame ([`crate::render::gpu_timer`], which
//! times the GPU only while the panel shows). The rows and the status line
//! are localized text ([`crate::i18n`]), so both draw with the process font, which draws
//! GAME.FNT's style without GAME.FNT until it loads ([`BitFont::without_fnt`]).
//! Both are independent of tactical cameras,
//! optional dev overlays, and shell/offscreen composition.

mod panel;

use crate::util::time::Instant;
use std::sync::Mutex;

use crate::render::batch::{
    BatchRenderer, BatchTexture, CameraUniform, InstanceBufferPool, SpriteInstance,
};
use crate::render::bit_font::BitFont;
use crate::render::gpu::GpuContext;

use super::state::FrameTimer;
use panel::{DOT, GREEN, HOT, PanelStats, Part, TRACK, WARM};

/// The status line's top when the panel is hidden.
const STATUS_TOP: f32 = 8.0;
/// The gap between the panel and the battlefield's or window's top and left
/// edges, and below it to the match's messages, in font pixels.
const EDGE: f32 = 4.0;
/// A core bar, in font pixels: its width, the gap to the next, its height and
/// its drop below the row's text top.
const BAR_WIDTH: f32 = 6.0;
const BAR_GAP: f32 = 2.0;
const BAR_HEIGHT: f32 = 12.0;
const BAR_DROP: f32 = 2.0;
const ELLIPSIS: &str = "...";

pub(crate) struct PerformanceOverlay {
    visible: bool,
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
    simulation_rate: SimulationRate,
    stats: PanelStats,
    /// Where the status line starts: below the panel while it shows.
    status_top: f32,
    /// In a match, how far down the battlefield the panel reaches: see
    /// [`PerformanceOverlay::message_top`]. 0 elsewhere.
    message_top: i32,
}

/// The Logic frames per second on the panel's Sim row, counted over
/// half-second windows of presented frames, so draws between two game frames
/// do not count.
#[derive(Default)]
struct SimulationRate {
    /// `None` outside a match, which hides the row.
    tps: Option<f32>,
    last_frame: Option<u32>,
    window_start_ms: u32,
    window_frames: u32,
}

impl SimulationRate {
    fn observe(&mut self, frame: u32, now_ms: u32, held: bool) {
        let mut tps = self.tps.unwrap_or(0.0);
        if self.last_frame != Some(frame) {
            match self.last_frame.map(|last| frame.wrapping_sub(last)) {
                Some(delta) if delta <= 300 => {
                    self.window_frames = self.window_frames.saturating_add(delta);
                }
                // A new match, or a jump no window could have held.
                _ => {
                    self.window_start_ms = now_ms;
                    self.window_frames = 0;
                    tps = 0.0;
                }
            }
            self.last_frame = Some(frame);
        }
        let elapsed = now_ms.wrapping_sub(self.window_start_ms);
        if held {
            self.window_start_ms = now_ms;
            self.window_frames = 0;
            tps = 0.0;
        } else if elapsed >= 500 {
            tps = self.window_frames as f32 * 1000.0 / elapsed as f32;
            self.window_start_ms = now_ms;
            self.window_frames = 0;
        }
        self.tps = Some(tps);
    }
}

/// Once per presented frame: the Logic frames since the last one, for the
/// panel's Sim row. Exact captures, pauses and a frozen window read 0.
pub(crate) fn observe_simulation(state: &crate::app::AppState, exact_capture: bool) {
    let overlay = &state.renderer.performance_overlay;
    if state.frontend.screen != crate::ui::game_screen::GameScreen::InGame {
        overlay.clear_simulation_rate();
        return;
    }
    let held = exact_capture
        || state
            .match_state
            .presentation_paused(&state.frontend.screen)
        || state.focus_freeze_active();
    if let Some(runtime) = state.match_state.sim_runtime.runtime() {
        overlay.observe_simulation_frame(
            runtime.simulation.session().binary_frame(),
            crate::app::match_runtime::frame_pacer::wall_clock_ms() as u32,
            held,
        );
    }
}

/// Where the panel draws, in window pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelPlacement {
    /// The left edge of every row's box.
    pub(crate) left: f32,
    /// The top edge of the first row's box.
    pub(crate) top: f32,
    /// Window pixels per font pixel: the match zoom, as the game's messages.
    pub(crate) scale: f32,
    /// The widest a row may draw; a longer row loses its end.
    pub(crate) max_width: f32,
}

impl PanelPlacement {
    /// `EDGE` in from the top left of an area `width` wide at (`left`,
    /// `top`), at `scale` window pixels per font pixel.
    fn top_left(left: f32, top: f32, width: f32, scale: f32) -> Self {
        Self {
            left: left + EDGE * scale,
            top: top + EDGE * scale,
            scale,
            max_width: (width - 2.0 * (EDGE + 1.0) * scale).max(0.0),
        }
    }

    /// The battlefield's top rows, in frame pixels, that a match panel ending
    /// at window y `bottom` takes, with an `EDGE` gap below it. A frame pixel
    /// is one font pixel at the match zoom.
    fn battlefield_rows(self, bottom: f32) -> i32 {
        ((bottom - self.top) / self.scale + 2.0 * EDGE).ceil() as i32
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
    // The battlefield is the frame beside the sidebar column and the command
    // bar's strip: its top left in the original, wherever a layout puts it.
    let (x, y, width, _) = crate::app::input::camera::tactical_viewport_px(state);
    let (scale, (left, top)) = state.tactical_zoom().map_or((1.0, (0, 0)), |zoom| {
        (zoom.percent as f32 / 100.0, zoom.origin)
    });
    Some(MatchPanel {
        placement: PanelPlacement::top_left(
            left as f32 + x as f32 * scale,
            top as f32 + y as f32 * scale,
            width as f32 * scale,
            scale,
        ),
        simulation: state
            .match_state
            .sim_runtime
            .runtime()
            .map(|runtime| &runtime.simulation),
    })
}

impl PerformanceOverlay {
    pub(crate) fn new(gpu: &GpuContext, batch: &BatchRenderer) -> Self {
        let (camera_buffer, camera_group) = batch.create_ui_camera(&gpu.device);
        Self {
            visible: false,
            white: batch.create_texture(gpu, &[255; 4], 1, 1),
            camera_buffer,
            camera_group,
            frame: Mutex::new(OverlayFrame {
                timer: FrameTimer::new(),
                pool: InstanceBufferPool::new(),
                size: [gpu.config.width, gpu.config.height],
                depth: gpu.create_depth_texture(),
                logged_first_frame: false,
                simulation_rate: SimulationRate::default(),
                stats: PanelStats::default(),
                status_top: STATUS_TOP,
                message_top: 0,
            }),
        }
    }

    /// Count a presented frame's Logic frames. `held` zeroes the rate.
    fn observe_simulation_frame(&self, binary_frame: u32, now_ms: u32, held: bool) {
        self.frame
            .lock()
            .expect("performance overlay frame lock")
            .simulation_rate
            .observe(binary_frame, now_ms, held);
    }

    /// Forget the rate: a match ended or a new one is loading.
    pub(crate) fn clear_simulation_rate(&self) {
        self.frame
            .lock()
            .expect("performance overlay frame lock")
            .simulation_rate = SimulationRate::default();
    }

    /// One finished in-game render's phase spans, for the Most CPU row.
    pub(crate) fn record_render_phases(
        &self,
        phases: crate::app::presentation::render::RenderPhases,
    ) {
        if self.visible {
            self.frame
                .lock()
                .expect("performance overlay frame lock")
                .stats
                .record_render(phases);
        }
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.visible
    }

    /// Whether the shown panel has new figures for the next draw.
    pub(crate) fn readings_due(&self, now: Instant) -> bool {
        self.visible
            && self
                .frame
                .lock()
                .expect("performance overlay frame lock")
                .stats
                .due(now)
    }

    /// The Options page's Performance Stats box.
    pub(crate) fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Ctrl+Alt+F: show or hide the panel; returns whether it is now shown.
    pub(crate) fn toggle(&mut self) -> bool {
        self.visible = !self.visible;
        self.visible
    }

    /// Where the match's message list starts, in frame pixels from the
    /// battlefield's top: below the shown panel, from its last drawn frame,
    /// and 0 while it is hidden.
    pub(crate) fn message_top(&self) -> i32 {
        if !self.visible {
            return 0;
        }
        self.frame
            .lock()
            .expect("performance overlay frame lock")
            .message_top
    }

    /// Call exactly once after all content has reached the acquired window
    /// surface, including synchronous loading repaints, before submission.
    /// `font` is the process font; `in_match` places the panel over a
    /// match's battlefield. The dedicated camera avoids rewriting uniforms
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
        let mut frame = self.frame.lock().expect("performance overlay frame lock");
        let now = Instant::now();
        // Keep measuring while hidden, so the first shown frame has a rate.
        frame.timer.sample(now);
        gpu.timer.set_active(self.visible);
        if !self.visible {
            frame.stats = PanelStats::default();
            frame.status_top = STATUS_TOP;
            frame.message_top = 0;
            return;
        }
        let size = [destination.width(), destination.height()];
        let over_battlefield = in_match.is_some();
        let (placement, simulation) = match in_match {
            Some(panel) => (panel.placement, panel.simulation),
            // Loading and menu screens: the window's own pixels.
            None => (
                PanelPlacement::top_left(0.0, 0.0, size[0] as f32, 1.0),
                None,
            ),
        };
        if frame.stats.due(now) {
            let frame_ms = frame.timer.frame_ms_mean();
            let tps = frame.simulation_rate.tps;
            let gpu_ms = gpu.timer.take_frame_ms();
            frame
                .stats
                .close_window(now, frame_ms, tps, simulation.map(match_sample), gpu_ms);
        }
        let mut rows = panel::rows(&frame.stats.shown);
        let mut instances = Vec::new();
        let mut glyphs = Vec::new();
        let bottom = layout_rows(font, &mut rows, placement, &mut instances, &mut glyphs);
        frame.status_top = bottom + 4.0;
        frame.message_top = if over_battlefield {
            placement.battlefield_rows(bottom)
        } else {
            0
        };
        let solid = instances.len() as u32;
        instances.append(&mut glyphs);
        if !frame.logged_first_frame {
            log::info!(
                "Performance panel: final window surface {}x{}, {} quads",
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
    /// is the process font, which covers localized text.
    /// This call neither samples nor changes the performance panel's visibility.
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

/// Lay `rows` out down from `placement.top` in reading order, each on its own
/// black box as the game's message line draws text, left-aligned at
/// `placement.left`, and return the last box's bottom edge. Box and bar quads
/// go to `solid`, text to `glyphs`. A row too wide narrows as long
/// translations do in VERA's boxes, and [`fit_row`] shortens it only past
/// the tightest squeeze.
fn layout_rows(
    font: &BitFont,
    rows: &mut [Vec<Part>],
    placement: PanelPlacement,
    solid: &mut Vec<SpriteInstance>,
    glyphs: &mut Vec<SpriteInstance>,
) -> f32 {
    let scale = placement.scale;
    let row_height = (font.cell_height() + 2.0) * scale;
    let left = placement.left.round();
    let mut box_top = placement.top.round();
    let max_width = placement.max_width / scale;
    for row in rows.iter_mut() {
        let fit = font
            .first_fit(|font| row_width(font, row) <= max_width)
            .unwrap_or_else(|tightest| tightest);
        font.fitted(fit, |font| {
            fit_row(font, row, max_width);
            let width = row_width(font, row) * scale + 2.0 * scale;
            solid.push(quad([left, box_top], [width, row_height], [0; 3]));
            let text_top = box_top + scale;
            let mut x = left + scale;
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
        });
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
/// drops whole entries from its end, the least costly areas on the Most CPU
/// row, and only then loses characters behind an ellipsis.
fn fit_row(font: &BitFont, row: &mut [Part], max_width: f32) {
    let width = row_width(font, row);
    if width <= max_width {
        return;
    }
    let Some(Part::Text(text, _)) = row.last_mut() else {
        return;
    };
    let others = width - font.text_width(text) as f32;
    while others + font.text_width(text) as f32 > max_width {
        let Some(entry) = text.rfind(DOT) else {
            break;
        };
        text.truncate(entry);
    }
    if others + font.text_width(text) as f32 <= max_width {
        return;
    }
    crate::render::text_audit::cut_off(text, max_width as u32);
    let mut keep = max_width - others - font.text_width(ELLIPSIS) as f32;
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
    fn simulation_rate_counts_logic_frames_not_draws() {
        let mut rate = SimulationRate::default();
        rate.observe(10, 100, false);
        for now in 101..500 {
            rate.observe(10, now, false);
        }
        rate.observe(11, 600, false);
        assert_eq!(rate.tps, Some(2.0)); // One Logic frame / 500 ms.
        rate.observe(11, 601, true);
        assert_eq!(rate.tps, Some(0.0));
    }

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

    #[test]
    fn a_row_too_wide_drops_whole_entries_before_cutting_a_word() {
        let font = crate::render::bit_font::tests::make_test_font(
            &[(u16::from(b'a'), 4), (0xB7, 2), (u16::from(b'.'), 1)],
            3,
        );
        let two = format!("aa{DOT}aaaa");
        let mut row = vec![Part::Text(format!("{two}{DOT}aaaaaa"), GREEN)];
        fit_row(&font, &mut row, font.text_width(&two) as f32 + 1.0);
        assert_eq!(row[0], Part::Text(two, GREEN));
    }

    #[test]
    fn a_row_a_little_too_wide_narrows_instead_of_losing_its_end() {
        let font = crate::render::bit_font::tests::make_vector_test_font();
        let text = "Most CPU:  Objects 38%";
        let natural = font.text_width(text) as f32;
        let mut rows = vec![vec![Part::Text(text.to_owned(), GREEN)]];
        let placement = PanelPlacement {
            left: 0.0,
            top: 0.0,
            scale: 1.0,
            max_width: natural * 0.9,
        };
        let (mut boxes, mut glyphs) = (Vec::new(), Vec::new());
        layout_rows(&font, &mut rows, placement, &mut boxes, &mut glyphs);
        assert_eq!(rows[0][0], Part::Text(text.to_owned(), GREEN));
        // The box holds the narrowed text and its 1-pixel margins.
        assert!(
            boxes[0].size[0] <= natural * 0.9 + 2.0,
            "{}",
            boxes[0].size[0]
        );
    }

    #[test]
    fn rows_hang_from_the_top_left_corner_above_the_messages() {
        let font = crate::render::bit_font::tests::make_test_font(&[(u16::from(b'a'), 4)], 3);
        let mut rows = vec![
            vec![Part::Text("aa".to_owned(), GREEN)],
            vec![Part::Text("aaaa".to_owned(), GREEN)],
        ];
        // A 400-pixel battlefield at (100, 50), zoomed 2x.
        let placement = PanelPlacement::top_left(100.0, 50.0, 400.0, 2.0);
        let (mut boxes, mut glyphs) = (Vec::new(), Vec::new());
        let bottom = layout_rows(&font, &mut rows, placement, &mut boxes, &mut glyphs);
        // EDGE font pixels in from the top and left, the first row on top.
        let row_height = (font.cell_height() + 2.0) * 2.0;
        for (index, quad) in boxes.iter().enumerate() {
            assert_eq!(quad.position, [108.0, 58.0 + index as f32 * row_height]);
        }
        assert!(boxes[0].size[0] < boxes[1].size[0]);
        assert_eq!(bottom, 58.0 + 2.0 * row_height);
        // The message list starts an EDGE below the last row.
        let message_top = 50.0 + placement.battlefield_rows(bottom) as f32 * 2.0;
        assert_eq!(message_top, bottom + EDGE * 2.0);
    }
}
