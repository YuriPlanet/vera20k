//! The match's text at the window's own pixels.
//!
//! A zoomed match draws its frame at the window's size divided by the zoom,
//! and the zoom pass scales it up (`render::upscale_pass`), so text drawn
//! into the frame would show the frame's pixels enlarged; an active-game
//! menu's text would also grow with the Sidebar size. The battlefield's
//! screen-fixed text (the timers, the outcome text when its art is missing,
//! messages and chat, tooltips, Phobos's build description) is laid out in
//! the frame as ever, emitted for the window ([`TextRaster`]) with the boxes
//! and frames behind it, and drawn here after the zoom and the sidebar's
//! window copy ([`draw_battlefield`]), first the sidebar column's own text
//! when its copy would resample it (`sidebar_surface::column_text`). An active-game menu's text and its
//! open list come here the same way after the menu's copy
//! (`skirmish_shell_render::in_game_shell`). The software cursor comes last,
//! as gamemd draws the mouse.
//!
//! While the in-match page or the Surrender box is open over the
//! battlefield, the battlefield's text stays in the frame under it, as
//! gamemd draws the page after it, and only the page's text comes here.
//!
//! The screenshot key's copy of the previous frame (`render::screenshot`)
//! takes this frame's draws here, all but the cursor's
//! ([`WindowUi::draw_screenshot_text`]). Outside a zoomed match, at the
//! Normal Sidebar size, the frame is the window and everything stays in it.

use std::cell::RefCell;

use crate::app::AppState;
use crate::render::batch::{BatchRenderer, BatchTexture, CameraUniform};
use crate::render::bit_font::TextRaster;
use crate::render::shell_text::{ScissorRect, TextRect};

/// The window passes' camera and this frame's draws there.
pub(crate) struct WindowUi {
    camera_buffer: wgpu::Buffer,
    camera: wgpu::BindGroup,
    /// The in-match page's text, laid out for the window.
    page_texts: RefCell<Vec<(ScissorRect, Option<(wgpu::Buffer, u32)>)>>,
    /// This frame's draws but the cursor's.
    drawn: RefCell<Vec<WindowDraw>>,
}

/// A draw in the window, kept for the screenshot key's copy.
struct WindowDraw {
    texture: wgpu::BindGroup,
    buffer: wgpu::Buffer,
    count: u32,
    scissor: [u32; 4],
}

impl WindowUi {
    pub(crate) fn new(device: &wgpu::Device, batch: &BatchRenderer) -> Self {
        let (camera_buffer, camera) = batch.create_ui_camera(device);
        Self {
            camera_buffer,
            camera,
            page_texts: RefCell::default(),
            drawn: RefCell::default(),
        }
    }

    /// Forget the previous frame's draws.
    pub(crate) fn begin_frame(&self) {
        self.page_texts.borrow_mut().clear();
        self.drawn.borrow_mut().clear();
    }

    /// The in-match page's text, laid out for the window, to draw after the
    /// zoom ([`draw_battlefield`]).
    pub(crate) fn set_page_texts(&self, texts: Vec<(ScissorRect, Option<(wgpu::Buffer, u32)>)>) {
        *self.page_texts.borrow_mut() = texts;
    }

    /// Whether this frame drew here what a screenshot's copy of the frame
    /// lacks.
    pub(crate) fn drew_text(&self) -> bool {
        !self.drawn.borrow().is_empty()
    }

    /// A pass over the window `view`, after everything drawn there, its
    /// camera at the window's pixels.
    pub(crate) fn begin_pass<'a>(
        &'a self,
        state: &'a AppState,
        encoder: &'a mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) -> WindowPass<'a> {
        let gpu = &state.renderer.gpu;
        let window = (gpu.config.width, gpu.config.height);
        let camera = CameraUniform {
            screen_size: [window.0 as f32, window.1 as f32],
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
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Window UI"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &state.renderer.depth_view,
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
        WindowPass {
            pass,
            ui: self,
            batch: &state.renderer.batch_renderer,
            window,
        }
    }

    /// This frame's draws but the cursor's, over `target`, a screenshot's
    /// window-sized copy of the previous frame: the frame the player saw,
    /// this frame's text on it.
    pub(crate) fn draw_screenshot_text(
        &self,
        state: &AppState,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
    ) {
        let view = target.create_view(&Default::default());
        let mut pass = self.begin_pass(state, encoder, &view);
        for draw in self.drawn.borrow().iter() {
            pass.draw_group(&draw.texture, &draw.buffer, draw.count, draw.scissor);
        }
    }
}

/// A render pass over the window ([`WindowUi::begin_pass`]).
pub(crate) struct WindowPass<'a> {
    pass: wgpu::RenderPass<'a>,
    ui: &'a WindowUi,
    batch: &'a BatchRenderer,
    window: (u32, u32),
}

impl WindowPass<'_> {
    /// `count` sprites of `buffer`, from `texture`, within `scissor` in
    /// window pixels; a screenshot takes them too.
    pub(crate) fn draw(
        &mut self,
        texture: &BatchTexture,
        buffer: &wgpu::Buffer,
        count: u32,
        scissor: ScissorRect,
    ) {
        let Some(scissor) = self.cut(scissor).filter(|_| count > 0) else {
            return;
        };
        self.draw_group(&texture.bind_group, buffer, count, scissor);
        self.ui.drawn.borrow_mut().push(WindowDraw {
            texture: texture.bind_group.clone(),
            buffer: buffer.clone(),
            count,
            scissor,
        });
    }

    /// The software cursor within `scissor`, the frame it points in, which
    /// a screenshot leaves out.
    pub(crate) fn draw_cursor(
        &mut self,
        texture: &BatchTexture,
        buffer: &wgpu::Buffer,
        count: u32,
        scissor: ScissorRect,
    ) {
        if let Some(scissor) = self.cut(scissor) {
            self.draw_group(&texture.bind_group, buffer, count, scissor);
        }
    }

    /// `rect` cut to the window; `None` when nothing of it is left.
    fn cut(&self, rect: ScissorRect) -> Option<[u32; 4]> {
        let (width, height) = self.window;
        let x = rect.x.min(width);
        let y = rect.y.min(height);
        let (w, h) = (rect.w.min(width - x), rect.h.min(height - y));
        (w > 0 && h > 0).then_some([x, y, w, h])
    }

    fn draw_group(
        &mut self,
        texture: &wgpu::BindGroup,
        buffer: &wgpu::Buffer,
        count: u32,
        [x, y, w, h]: [u32; 4],
    ) {
        self.pass.set_scissor_rect(x, y, w, h);
        self.batch.draw_passthrough_group(
            &mut self.pass,
            texture,
            buffer,
            &self.ui.camera,
            0..count,
        );
    }
}

/// The zoom's raster: a frame pixel, in the UI camera's units (its camera
/// included), at the window pixel the zoom pass shows it on. `None` outside
/// a zoomed match.
pub(crate) fn zoom_raster(state: &AppState) -> Option<TextRaster> {
    let zoom = state.match_zoom_pass()?.geometry();
    Some(TextRaster {
        scale: zoom.scale(),
        origin: [zoom.origin.0 as f32, zoom.origin.1 as f32],
        camera: [
            state.match_state.input.camera_x,
            state.match_state.input.camera_y,
        ],
    })
}

/// The raster the battlefield's screen-fixed text takes this frame: the
/// zoom's, unless the in-match page or the Surrender box is open over it.
pub(crate) fn battlefield_raster(state: &AppState) -> Option<TextRaster> {
    use crate::ui::pause_menu::InGameMenuState;
    if matches!(
        state.match_state.match_presentation.in_game_menu,
        InGameMenuState::Page | InGameMenuState::Surrender
    ) {
        return None;
    }
    zoom_raster(state)
}

/// The raster for a dialog drawn over the battlefield at frame pixels with
/// its camera at the origin: the in-match page, the Surrender box, and the
/// software cursor over an active-game menu.
pub(crate) fn dialog_raster(state: &AppState) -> Option<TextRaster> {
    zoom_raster(state).map(|raster| TextRaster {
        camera: [0.0; 2],
        ..raster
    })
}

/// The raster for an active-game menu, whose pixels are the Sidebar size's
/// (`sidebar`, copied into the frame at that size) or the frame's: a menu
/// pixel at the window pixel it shows on. `None` outside a match, where
/// menus paint at the window's size, or where that is the pixel itself.
pub(crate) fn shell_raster(state: &AppState, sidebar: bool) -> Option<TextRaster> {
    if state.frontend.screen != crate::ui::game_screen::GameScreen::InGame {
        return None;
    }
    let ((left, top), scale) = if sidebar {
        let frame = state.sidebar_frame();
        (frame.layout_origin(), frame.scale())
    } else {
        ((0.0, 0.0), 1.0)
    };
    let (zoom, origin) = state
        .tactical_zoom()
        .map_or((1.0, (0, 0)), |zoom| (zoom.scale(), zoom.origin));
    let raster = TextRaster {
        scale: scale * zoom,
        origin: [origin.0 as f32 + left * zoom, origin.1 as f32 + top * zoom],
        camera: [0.0; 2],
    };
    (raster != TextRaster::NATIVE).then_some(raster)
}

/// `(x, y, width, height)` in `raster`'s pixels, in the window's.
fn window_rect(raster: TextRaster, (x, y, width, height): (u32, u32, u32, u32)) -> ScissorRect {
    ScissorRect::of(
        TextRect {
            x: x as i32,
            y: y as i32,
            w: width,
            h: height,
        },
        raster,
    )
}

/// The battlefield's window layers in the frame's order, the in-match
/// page's text, then the software cursor: after the zoom pass and the
/// sidebar's window copy put the frame in the window `view` (`app::frame`).
pub(crate) fn draw_battlefield(
    state: &AppState,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    software_cursor: Option<crate::app::presentation::ui_overlays::SoftwareCursorDraw>,
) {
    let Some(zoom) = zoom_raster(state) else {
        return;
    };
    let renderer = &state.renderer;
    let pool = &renderer.instance_pool;
    let white = state
        .match_state
        .match_presentation
        .selection_overlay
        .as_ref()
        .map(|overlay| overlay.white_texture());
    let atlas = renderer.bit_font.atlas();
    let frame = frame_rect(state);
    let layer = |pass: &mut WindowPass<'_>, texture: Option<&BatchTexture>, key, scissor| {
        if let (Some(texture), Some((buffer, count))) = (texture, pool.get(key)) {
            pass.draw(texture, buffer, count, scissor);
        }
    };
    let mut pass = renderer.window_ui.begin_pass(state, encoder, view);
    if battlefield_raster(state).is_some() {
        use crate::app::presentation::sidebar_surface::{self, ColumnText};
        // The sidebar column's text, where its copy would resample it, over
        // the window copy within the column; the timers keep to the
        // tactical view, so drawing them after it changes nothing.
        if let ColumnText::Window(_) = sidebar_surface::column_text(state) {
            let column = window_rect(zoom, state.sidebar_frame().column());
            for key in sidebar_surface::TEXT_LAYERS {
                layer(&mut pass, Some(atlas), key, column);
            }
        }
        // The timers keep the tactical view's clip.
        let tactical = window_rect(zoom, crate::app::input::camera::tactical_viewport_px(state));
        layer(&mut pass, white, "super_timer_fill", tactical);
        layer(&mut pass, Some(atlas), "super_timer_text", tactical);
        for (texture, key) in [
            (Some(atlas), "outcome_text"),
            (white, "message_fill"),
            (Some(atlas), "message_text"),
            (white, "tooltip_fill"),
            (white, "tooltip_border"),
            (Some(atlas), "tooltip_text"),
            (white, "build_text_fill"),
            (Some(atlas), "build_text_text"),
        ] {
            layer(&mut pass, texture, key, frame);
        }
    }
    for (scissor, buffer) in renderer.window_ui.page_texts.borrow().iter() {
        if let Some((buffer, count)) = buffer {
            pass.draw(atlas, buffer, *count, *scissor);
        }
    }
    if let (Some(texture), Some((buffer, count))) = (
        software_cursor.and_then(|draw| {
            crate::app::presentation::ui_overlays::software_cursor_texture(state, draw)
        }),
        pool.get("software_cursor"),
    ) {
        pass.draw_cursor(texture, buffer, count, frame);
    }
}

/// The frame's rect in the window, where a zoomed match shows it; the
/// window itself without a zoom.
pub(crate) fn frame_rect(state: &AppState) -> ScissorRect {
    window_rect(
        dialog_raster(state).unwrap_or(TextRaster::NATIVE),
        (0, 0, state.render_width(), state.render_height()),
    )
}
