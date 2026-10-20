//! Asset-independent utility cards for load errors, generic loading/results,
//! and missing modal artwork. These are VERA presentation, not retail parity.
//! The existing BitFont (including its built-in face) and batch renderer own
//! drawing; actions commit from window events before acquiring a frame.

use std::borrow::Cow;

use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::app::{App, AppState, GameScreen};
use crate::render::batch::{DepthAxis, SpriteInstance};
use crate::ui::pause_menu::{InGameMenuState, ModalOutcome};
use crate::ui::shell::geom::RectPx;

#[derive(Clone, Copy)]
enum Action {
    Quit,
    BackToMenu,
    Resume,
    Abort,
    Leave,
}

impl Action {
    fn label(self, state: &AppState) -> Cow<'_, str> {
        let (key, fallback) = match self {
            Self::Quit => (None, "Quit"),
            Self::BackToMenu => (None, "Back To Menu"),
            Self::Resume => (Some("GUI:ResumeMission"), "Resume Mission"),
            Self::Abort => (Some("GUI:AbortMission"), "Abort Mission"),
            Self::Leave => (Some("GUI:Leave"), "Quit"),
        };
        key.and_then(|key| state.process_assets.csf.as_ref()?.get(key))
            .map_or(Cow::Borrowed(fallback), Cow::Borrowed)
    }

    fn commit(self, state: &mut AppState, event_loop: &ActiveEventLoop) {
        match self {
            // A startup failure must not write defaults over the retail profile.
            Self::Quit => event_loop.exit(),
            Self::BackToMenu => App::leave_mission_result_screen(state),
            Self::Resume => App::enter_in_game_menu_state(state, InGameMenuState::Closed),
            Self::Abort => App::enter_in_game_menu_state(state, InGameMenuState::AbortConfirm),
            Self::Leave => App::apply_in_game_modal_outcome(
                state,
                ModalOutcome::LeaveMatch { destruct: false },
            ),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Buttons([Option<Action>; 2]);

impl Buttons {
    pub(crate) const NONE: Self = Self([None, None]);
    pub(crate) const QUIT: Self = Self([Some(Action::Quit), None]);
    pub(crate) const BACK_TO_MENU: Self = Self([Some(Action::BackToMenu), None]);
    pub(crate) const MISSING_MENU: Self = Self([Some(Action::Resume), Some(Action::Abort)]);
    pub(crate) const ABORT_CONFIRM: Self = Self([Some(Action::Leave), Some(Action::Resume)]);

    fn for_state(state: &AppState) -> Option<Self> {
        match state.frontend.screen {
            GameScreen::MainMenu if state.frontend.main_menu_shell_error.is_some() => Some(Self::QUIT),
            GameScreen::MissionResult { .. }
                if !App::score_shell_active(state)
                    && !App::campaign_score_shell_active(state) =>
            {
                Some(Self::BACK_TO_MENU)
            }
            GameScreen::InGame
                if state.match_state.match_presentation.in_game_menu.is_open()
                    && !crate::app::frontend::skirmish_shell_render::native_in_game_shell_active(state) =>
            {
                Some(if state.match_state.match_presentation.in_game_menu == InGameMenuState::AbortConfirm {
                    Self::ABORT_CONFIRM
                } else {
                    Self::MISSING_MENU
                })
            }
            _ => None,
        }
    }
}

/// One layout in physical window pixels, used by both drawing and hit testing.
fn button_rects(width: u32, height: u32, count: usize) -> Vec<RectPx> {
    if count == 0 {
        return Vec::new();
    }
    let gap = 16;
    let width = width.min(i32::MAX as u32) as i32;
    let height = height.min(i32::MAX as u32) as i32;
    let button_width = ((width - 32 - gap * (count as i32 - 1)) / count as i32).clamp(1, 240);
    let total_width = button_width * count as i32 + gap * (count as i32 - 1);
    let x = (width - total_width).max(0) / 2;
    (0..count)
        .map(|i| {
            RectPx::new(
                x + i as i32 * (button_width + gap),
                (height - 64).max(0),
                button_width,
                40.min(height),
            )
        })
        .collect()
}

pub(crate) fn handle_event(
    state: &mut AppState,
    event: &WindowEvent,
    event_loop: &ActiveEventLoop,
) -> bool {
    let Some(buttons) = Buttons::for_state(state) else {
        return false;
    };
    match event {
        WindowEvent::CursorMoved { position, .. } => {
            // Match input keeps source pixels even while a card covers a
            // zoomed match; the card maps back through `window_cursor_position`.
            state.set_cursor_from_window(position.x as f32, position.y as f32);
            state.platform.window.request_redraw();
            true
        }
        WindowEvent::MouseInput {
            state: ElementState::Released,
            button: MouseButton::Left,
            ..
        } => {
            let actions: Vec<_> = buttons.0.into_iter().flatten().collect();
            let rects = button_rects(
                state.renderer.gpu.config.width,
                state.renderer.gpu.config.height,
                actions.len(),
            );
            let (x, y) = state.window_cursor_position();
            if let Some((_, action)) = rects
                .into_iter()
                .zip(actions)
                .find(|(rect, _)| rect.contains(x as i32, y as i32))
            {
                action.commit(state, event_loop);
            }
            state.platform.window.request_redraw();
            true
        }
        WindowEvent::KeyboardInput { event, .. } => {
            if event.state.is_pressed() && !event.repeat {
                let action = match event.physical_key {
                    PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => buttons.0[0],
                    PhysicalKey::Code(KeyCode::Escape) => {
                        buttons.0.into_iter().flatten().find(|action| {
                            matches!(action, Action::Resume | Action::Quit | Action::BackToMenu)
                        })
                    }
                    _ => None,
                };
                if let Some(action) = action {
                    action.commit(state, event_loop);
                }
                state.platform.window.request_redraw();
            }
            true
        }
        WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. } => true,
        // Focus, resize and close retain the platform lifecycle handler.
        _ => false,
    }
}

pub(crate) fn render(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    title: &str,
    detail: &str,
    buttons: Buttons,
) {
    state.frontend.status_screen_drawn = true;
    let gpu = &state.renderer.gpu;
    let batch = &state.renderer.batch_renderer;
    let font = &state.renderer.bit_font;
    let width = gpu.config.width;
    let height = gpu.config.height;
    let pad = (width / 20).clamp(8, 32);
    let content_width = width.saturating_sub(pad * 2).clamp(1, 960);
    let left = width.saturating_sub(content_width) as f32 / 2.0;
    let top = (height / 10).clamp(12, 64) as f32;
    let text_scale = (16.0 / font.glyph_height()).max(1.0);
    let actions: Vec<_> = buttons.0.into_iter().flatten().collect();
    let rects = button_rects(width, height, actions.len());
    let body_top = top + 48.0;
    let body_bottom = rects
        .first()
        .map_or(height.saturating_sub(pad), |rect| rect.y.max(0) as u32)
        .saturating_sub(16);
    let mut text = font.build_text(title, left, top, text_scale * 1.4, 0.0, [0.9; 3], [0.0; 2]);
    let layout = font.wrap_layout(detail, (content_width as f32 / text_scale).max(1.0) as u32);
    for (index, line) in layout.lines.iter().enumerate() {
        let y = body_top + index as f32 * font.cell_height() * text_scale;
        if y + font.glyph_height() * text_scale > body_bottom as f32 {
            break;
        }
        text.extend(font.build_text(
            &detail[line.start_byte..line.end_byte],
            left,
            y,
            text_scale,
            0.0,
            [0.8; 3],
            [0.0; 2],
        ));
    }
    let (cursor_x, cursor_y) = state.window_cursor_position();
    let mut fills = Vec::with_capacity(rects.len());
    let mut button_text = Vec::with_capacity(rects.len());
    for (rect, action) in rects.iter().zip(actions) {
        let hovered = rect.contains(cursor_x as i32, cursor_y as i32);
        fills.push(SpriteInstance {
            position: [rect.x as f32, rect.y as f32],
            size: [rect.w as f32, rect.h as f32],
            uv_origin: [0.0; 2],
            uv_size: [1.0; 2],
            depth: 0.1,
            tint: if hovered {
                [0.18, 0.28, 0.36]
            } else {
                [0.1, 0.16, 0.22]
            },
            alpha: 1.0,
            ..Default::default()
        });
        let label = action.label(state);
        let x = rect.x as f32
            + ((rect.w as f32 - font.text_width(&label) as f32 * text_scale) / 2.0).max(4.0);
        let y = rect.y as f32 + (rect.h as f32 - font.glyph_height() * text_scale) / 2.0;
        button_text.push(font.build_text(&label, x, y, text_scale, 0.0, [0.95; 3], [0.0; 2]));
    }
    batch.update_camera(
        gpu,
        width as f32,
        height as f32,
        0.0,
        0.0,
        1.0,
        DepthAxis::NONE,
    );
    let fills = batch.create_instance_buffer(gpu, &fills);
    let text = batch.create_instance_buffer(gpu, &text);
    let button_buffers: Vec<_> = button_text
        .iter()
        .map(|instances| batch.create_instance_buffer(gpu, instances))
        .collect();
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Utility status card"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: 0.02,
                    g: 0.03,
                    b: 0.04,
                    a: 1.0,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &state.renderer.depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    if let Some((buffer, count)) = &fills {
        batch.draw_with_buffer_passthrough(
            &mut pass,
            &state.renderer.status_screen_fill,
            buffer,
            *count,
        );
    }
    if let Some((buffer, count)) = &text {
        batch.draw_with_buffer_passthrough(&mut pass, font.atlas(), buffer, *count);
    }
    for (rect, buffer) in rects.iter().zip(&button_buffers) {
        let x = (rect.x.max(0) as u32).min(width);
        let y = (rect.y.max(0) as u32).min(height);
        let w = (rect.w.max(0) as u32).min(width - x);
        let h = (rect.h.max(0) as u32).min(height - y);
        if w > 0 && h > 0 {
            pass.set_scissor_rect(x, y, w, h);
            if let Some((buffer, count)) = buffer {
                batch.draw_with_buffer_passthrough(&mut pass, font.atlas(), buffer, *count);
            }
        }
    }
}
