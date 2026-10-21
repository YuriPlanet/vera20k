//! Asset-independent utility cards for load errors, generic loading/results,
//! and missing modal artwork. These are VERA presentation, not retail parity.
//!
//! The card is drawn from three sources only: a one-pixel fill texture for its
//! frame and rules, a baked radial-gradient texture for the buttons, and
//! [`crate::render::status_font`]'s in-repo face for its type. Nothing here
//! reads a retail asset, so the card renders correctly when the game
//! installation is missing or incomplete — the situation it exists for.
//!
//! Look: black field, #FFFF00 type, and buttons carrying the Red Alert 2 read —
//! a #CE0000 core fading to black at the rim, edged in black and #FF0000.

use std::borrow::Cow;

use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::app::{App, AppState, GameScreen};
use crate::render::batch::{BatchRenderer, BatchTexture, DepthAxis, SpriteInstance};
use crate::render::gpu::GpuContext;
use crate::render::status_font::StrokeFace;
use crate::ui::pause_menu::{InGameMenuState, ModalOutcome};
use crate::ui::shell::geom::RectPx;

/// Identity line: the bottom-right status screen and the dashboard's header
/// share this one stamp. Not a retail build string.
pub(crate) const BUILD_STAMP: &str = concat!("VERA20K ", env!("CARGO_PKG_VERSION"));

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
                    && !App::campaign_score_shell_active(state)
                    && !crate::app::input::coop_score::active(state) =>
            {
                Some(Self::BACK_TO_MENU)
            }
            GameScreen::InGame
                if state.match_state.match_presentation.in_game_menu.is_open()
                    && !state.match_state.match_presentation.in_game_menu.draws_over_battlefield()
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

    fn actions(self) -> Vec<Action> {
        self.0.into_iter().flatten().collect()
    }
}

// --- palette -------------------------------------------------------------
/// The Red Alert 2 button read: a #CE0000 core fading to black, edged in
/// #FF0000 between black hairlines, under #FFFF00 type.
const FRAME_RED: [f32; 3] = [1.0, 0.0, 0.0];
const FRAME_BLACK: [f32; 3] = [0.0, 0.0, 0.0];
/// The short rule under the heading, one step down from the frame so the two
/// red lines do not read as a single thick band.
const RULE_DIM_RED: [f32; 3] = [0.62, 0.0, 0.0];
/// Button core colour, before the radial falloff darkens it toward the rim.
const FIELD_RED: [u8; 3] = [0xCE, 0x00, 0x00];
const TEXT_YELLOW: [f32; 3] = [1.0, 1.0, 0.0];
/// The build stamp is furniture; it stays dimmer than the message.
const TEXT_DIM: [f32; 3] = [0.62, 0.62, 0.0];

/// Depth of the card's solid shapes and of the type above them.
const GLOW_DEPTH: f32 = 0.3;
const SHAPE_DEPTH: f32 = 0.2;
const TYPE_DEPTH: f32 = 0.1;

/// Radial gradient resolution. It is a smooth ramp, so a small texture
/// stretched over the window is indistinguishable from a per-pixel one.
const GRADIENT_TEXELS: u32 = 256;

/// The card's design grid for one window size; every pixel value is a rounded
/// multiple of the height scale so the frame stays crisp.
struct CardLayout {
    scale: f32,
    width: i32,
    height: i32,
    margin: i32,
    pad: i32,
    title_cap: f32,
    body_cap: f32,
    small_cap: f32,
    button_w: i32,
    button_h: i32,
    button_y: i32,
}

impl CardLayout {
    fn new(width: u32, height: u32) -> Self {
        let width = width.min(i32::MAX as u32) as i32;
        let height = height.min(i32::MAX as u32) as i32;
        // One design unit is 1/480 of the window height: the retail front-end
        // grid, so the card keeps its proportions at every size.
        let scale = (height as f32 / 480.0).max(1.0);
        let px = |value: f32| (value * scale).round() as i32;
        let margin = px(12.0).max(2);
        let button_h = px(26.0).max(12);
        Self {
            scale,
            width,
            height,
            margin,
            pad: margin + px(26.0),
            // Sized from the 640x480 grid: at 800x600 these land on a 22 px
            // heading and 12 px body, which stays readable without crowding.
            title_cap: (18.0 * scale).round().max(9.0),
            body_cap: (10.0 * scale).round().max(7.0),
            small_cap: (9.0 * scale).round().max(6.0),
            button_w: px(118.0).max(48),
            button_h,
            button_y: height - margin - px(22.0) - button_h,
        }
    }

    /// Button rects in click order, centred along the card's foot.
    fn button_rects(&self, count: usize) -> Vec<RectPx> {
        if count == 0 {
            return Vec::new();
        }
        let gap = (12.0 * self.scale).round() as i32;
        let total = self.button_w * count as i32 + gap * (count as i32 - 1);
        let x = (self.width - total).max(0) / 2;
        (0..count)
            .map(|index| {
                RectPx::new(
                    x + index as i32 * (self.button_w + gap),
                    self.button_y,
                    self.button_w,
                    self.button_h,
                )
            })
            .collect()
    }
}

/// Build the button face: a radial ramp from #CE0000 at the centre to black at
/// the rim, which is what gives the Red Alert 2 button its inward glow.
pub(crate) fn build_button_glow(gpu: &GpuContext, batch: &BatchRenderer) -> BatchTexture {
    let size = GRADIENT_TEXELS;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let half = size as f32 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 + 0.5 - half) / half;
            let dy = (y as f32 + 0.5 - half) / half;
            let radius = (dx * dx + dy * dy).sqrt().min(1.0);
            // Bright core, black rim: smooth falloff, so no banding shows.
            let glow = (1.0 - radius).powf(1.6);
            let index = ((y * size + x) * 4) as usize;
            rgba[index] = (FIELD_RED[0] as f32 * glow).round() as u8;
            rgba[index + 1] = (FIELD_RED[1] as f32 * glow).round() as u8;
            rgba[index + 2] = (FIELD_RED[2] as f32 * glow).round() as u8;
            rgba[index + 3] = 255;
        }
    }
    batch.create_texture(gpu, &rgba, size, size)
}

pub(crate) fn handle_event(
    state: &mut AppState,
    event: &WindowEvent,
    event_loop: &ActiveEventLoop,
) -> bool {
    let Some(buttons) = Buttons::for_state(state) else {
        return false;
    };
    let actions = buttons.actions();
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
            let layout = CardLayout::new(
                state.renderer.gpu.config.width,
                state.renderer.gpu.config.height,
            );
            let (x, y) = state.window_cursor_position();
            if let Some((_, action)) = layout
                .button_rects(actions.len())
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
                    PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                        actions.first().copied()
                    }
                    PhysicalKey::Code(KeyCode::Escape) => actions.iter().copied().find(|action| {
                        matches!(action, Action::Resume | Action::Quit | Action::BackToMenu)
                    }),
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

/// Push one solid rectangle.
fn push_rect(out: &mut Vec<SpriteInstance>, rect: RectPx, color: [f32; 3]) {
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    out.push(SpriteInstance {
        position: [rect.x as f32, rect.y as f32],
        size: [rect.w as f32, rect.h as f32],
        uv_origin: [0.0; 2],
        uv_size: [1.0; 2],
        depth: SHAPE_DEPTH,
        tint: color,
        alpha: 1.0,
        ..Default::default()
    });
}

/// Push a rectangular outline of `thickness` inside `rect`.
fn push_frame(out: &mut Vec<SpriteInstance>, rect: RectPx, thickness: i32, color: [f32; 3]) {
    let thickness = thickness.max(1);
    push_rect(out, RectPx::new(rect.x, rect.y, rect.w, thickness), color);
    push_rect(
        out,
        RectPx::new(rect.x, rect.y + rect.h - thickness, rect.w, thickness),
        color,
    );
    push_rect(out, RectPx::new(rect.x, rect.y, thickness, rect.h), color);
    push_rect(
        out,
        RectPx::new(rect.x + rect.w - thickness, rect.y, thickness, rect.h),
        color,
    );
}

/// One baked face plus the instances that use it.
struct TypeRun<'a> {
    face: &'a StrokeFace,
    instances: Vec<SpriteInstance>,
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
    // The in-repo stroke face is a skeleton: its capitals are unambiguous where
    // its lowercase forms are not, so the card sets every string in caps. That
    // also matches the Red Alert 2 front end's own casing.
    let title = title.to_uppercase();
    let detail = detail.to_uppercase();
    let layout = CardLayout::new(
        state.renderer.gpu.config.width,
        state.renderer.gpu.config.height,
    );
    let actions = buttons.actions();
    let labels: Vec<String> = actions
        .iter()
        .map(|action| action.label(state).to_uppercase())
        .collect();
    let (cursor_x, cursor_y) = state.window_cursor_position();
    let button_rects = layout.button_rects(actions.len());
    let thickness = (layout.scale.round() as i32).max(1);

    let mut shapes: Vec<SpriteInstance> = Vec::new();
    // Buttons are the only ornamented element: the field behind them is plain
    // black until the card gets a background of its own.
    let mut glows: Vec<SpriteInstance> = Vec::new();
    for rect in &button_rects {
        let hovered = rect.contains(cursor_x as i32, cursor_y as i32);
        glows.push(SpriteInstance {
            position: [rect.x as f32, rect.y as f32],
            size: [rect.w as f32, rect.h as f32],
            uv_origin: [0.0; 2],
            uv_size: [1.0; 2],
            depth: GLOW_DEPTH,
            tint: if hovered {
                [1.35, 1.35, 1.35]
            } else {
                [1.0; 3]
            },
            alpha: 1.0,
            ..Default::default()
        });
        // Black edging outside a #FF0000 line, and the same inside it.
        push_frame(&mut shapes, *rect, thickness, FRAME_BLACK);
        push_frame(
            &mut shapes,
            RectPx::new(
                rect.x + thickness,
                rect.y + thickness,
                rect.w - thickness * 2,
                rect.h - thickness * 2,
            ),
            thickness,
            FRAME_RED,
        );
        push_frame(
            &mut shapes,
            RectPx::new(
                rect.x + thickness * 2,
                rect.y + thickness * 2,
                rect.w - thickness * 4,
                rect.h - thickness * 4,
            ),
            thickness,
            FRAME_BLACK,
        );
    }

    let renderer = &mut state.renderer;
    let gpu = &renderer.gpu;
    let batch = &renderer.batch_renderer;
    let font = &mut renderer.status_font;
    let inner_w = (layout.width - layout.pad * 2) as f32;

    // Line advance is a property of the size: 45% of the cap height between
    // baselines reads the same at every scale.
    let gap_for = |cap: f32| (cap * 0.5).round();
    let title_face = font.face(
        gpu,
        batch,
        layout.title_cap,
        1.55,
        gap_for(layout.title_cap),
    );
    let body_face = font.face(gpu, batch, layout.body_cap, 1.15, gap_for(layout.body_cap));
    let small_face = font.face(
        gpu,
        batch,
        layout.small_cap,
        1.05,
        gap_for(layout.small_cap),
    );
    let stamp_cap = (layout.small_cap * 0.9).max(7.0);
    let stamp_face = font.face(gpu, batch, stamp_cap, 1.0, gap_for(stamp_cap));

    let mut runs: Vec<TypeRun<'_>> = Vec::new();

    {
        let face = font.get(title_face);
        let mut instances = Vec::new();
        let baseline = layout.pad as f32 + face.cap_px() + 2.0 * layout.scale;
        face.push_text(
            &mut instances,
            &title,
            layout.pad as f32,
            baseline,
            TEXT_YELLOW,
            TYPE_DEPTH,
        );
        runs.push(TypeRun { face, instances });

        // A #FF0000 rule separates the heading from the message, with a short
        // black rule beneath it.
        let rule_y = (baseline + (10.0 * layout.scale).round()) as i32;
        push_rect(
            &mut shapes,
            RectPx::new(
                layout.pad,
                rule_y,
                (layout.width - layout.pad * 2).max(1),
                thickness,
            ),
            FRAME_RED,
        );
        let sub_rule_y = rule_y + (8.0 * layout.scale).round() as i32;
        push_rect(
            &mut shapes,
            RectPx::new(
                layout.pad,
                sub_rule_y,
                ((layout.width - layout.pad * 2) as f32 * 0.34).round() as i32,
                thickness,
            ),
            RULE_DIM_RED,
        );

        // Body paragraphs: the first states the failure, the rest the advice.
        let body = font.get(body_face);
        let small = font.get(small_face);
        let footer = (layout.button_y as f32 - 14.0 * layout.scale).max(0.0);
        let area_top = sub_rule_y as f32 + (20.0 * layout.scale).round();

        // A numbered step hangs its continuation lines under the text rather
        // than under the number, which is what keeps a wrapped list readable.
        fn list_indent(face: &StrokeFace, paragraph: &str) -> f32 {
            let mut chars = paragraph.char_indices();
            let Some((_, first)) = chars.next() else {
                return 0.0;
            };
            if !first.is_ascii_digit() {
                return 0.0;
            }
            let Some((dot, second)) = chars.next() else {
                return 0.0;
            };
            if second != '.' {
                return 0.0;
            }
            let _ = dot;
            face.text_width("1. ")
        }

        // Lay the paragraphs out first, then centre the block between the rules
        // and the button row: a one-line message otherwise leaves a dead band.
        struct Planned {
            first: bool,
            indent: f32,
            lines: Vec<String>,
        }
        let mut planned: Vec<Planned> = Vec::new();
        let mut block_height = 0.0f32;
        let mut first = true;
        for paragraph in detail.split('\n') {
            if paragraph.trim().is_empty() {
                planned.push(Planned {
                    first,
                    indent: 0.0,
                    lines: Vec::new(),
                });
                block_height += (body.cap_px() * 0.85).round().max(3.0);
                first = false;
                continue;
            }
            let face = if first { body } else { small };
            let indent = list_indent(face, paragraph);
            let lines = face.wrap(paragraph, inner_w - indent);
            if !first {
                block_height += (face.cap_px() * 0.45).round().max(2.0);
            }
            block_height += lines.len() as f32 * face.line_px();
            planned.push(Planned {
                first,
                indent,
                lines,
            });
            first = false;
        }
        let slack = (footer - area_top - block_height).max(0.0);
        let mut boundary = area_top + slack * 0.45;

        let mut body_instances = Vec::new();
        let mut small_instances = Vec::new();
        for paragraph in planned {
            let face = if paragraph.first { body } else { small };
            if paragraph.lines.is_empty() {
                boundary += (body.cap_px() * 0.85).round().max(3.0);
                continue;
            }
            if !paragraph.first {
                boundary += (face.cap_px() * 0.45).round().max(2.0);
            }
            for (index, line) in paragraph.lines.iter().enumerate() {
                let baseline = boundary + face.cap_px();
                if baseline > footer {
                    break;
                }
                let targets = if paragraph.first {
                    &mut body_instances
                } else {
                    &mut small_instances
                };
                let x = layout.pad as f32 + if index == 0 { 0.0 } else { paragraph.indent };
                face.push_text(targets, line, x, baseline, TEXT_YELLOW, TYPE_DEPTH);
                boundary += face.line_px();
            }
        }
        runs.push(TypeRun {
            face: body,
            instances: body_instances,
        });
        runs.push(TypeRun {
            face: small,
            instances: small_instances,
        });

        // Button labels, centred in their panels.
        // The label uses the body size: a heading-sized label overruns the
        // button it sits in at every window size.
        let label_face = font.get(body_face);
        let mut label_instances = Vec::new();
        for (rect, label) in button_rects.iter().zip(&labels) {
            let width = label_face.text_width(label);
            label_face.push_text(
                &mut label_instances,
                label,
                rect.x as f32 + (rect.w as f32 - width) / 2.0,
                rect.y as f32 + (rect.h as f32 + label_face.cap_px()) / 2.0,
                TEXT_YELLOW,
                TYPE_DEPTH,
            );
        }
        runs.push(TypeRun {
            face: label_face,
            instances: label_instances,
        });

        // Build stamp at the bottom right, clear of the frame.
        let stamp = font.get(stamp_face);
        let mut stamp_instances = Vec::new();
        let width = stamp.text_width(BUILD_STAMP);
        stamp.push_text(
            &mut stamp_instances,
            BUILD_STAMP,
            layout.width as f32 - layout.pad as f32 - width,
            layout.height as f32 - layout.margin as f32 - (12.0 * layout.scale),
            TEXT_DIM,
            TYPE_DEPTH,
        );
        runs.push(TypeRun {
            face: stamp,
            instances: stamp_instances,
        });
    }

    let fill: &BatchTexture = &renderer.status_screen_fill;
    let glow: &BatchTexture = &renderer.status_card_glow;
    batch.update_camera(
        gpu,
        layout.width as f32,
        layout.height as f32,
        0.0,
        0.0,
        1.0,
        DepthAxis::NONE,
    );
    let glow_buffer = batch.create_instance_buffer(gpu, &glows);
    let shape_buffer = batch.create_instance_buffer(gpu, &shapes);
    let run_buffers: Vec<Option<(wgpu::Buffer, u32)>> = runs
        .iter()
        .map(|run| batch.create_instance_buffer(gpu, &run.instances))
        .collect();

    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Utility status card"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &renderer.depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    if let Some((buffer, count)) = &glow_buffer {
        batch.draw_with_buffer_passthrough(&mut pass, glow, buffer, *count);
    }
    if let Some((buffer, count)) = &shape_buffer {
        batch.draw_with_buffer_passthrough(&mut pass, fill, buffer, *count);
    }
    for (run, buffer) in runs.iter().zip(&run_buffers) {
        if let Some((buffer, count)) = buffer {
            batch.draw_with_buffer_passthrough(&mut pass, run.face.texture(), buffer, *count);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_are_centred_and_never_overlap() {
        let layout = CardLayout::new(1280, 720);
        let one = layout.button_rects(1);
        assert_eq!(one.len(), 1);
        let centre = one[0].x + one[0].w / 2;
        assert!((centre - layout.width / 2).abs() <= 1, "{centre}");

        let two = layout.button_rects(2);
        assert_eq!(two.len(), 2);
        assert!(two[0].x + two[0].w <= two[1].x);
        let span = two[1].x + two[1].w - two[0].x;
        assert!((two[0].x + span / 2 - layout.width / 2).abs() <= 1);
    }

    #[test]
    fn card_scales_with_the_window_height() {
        let small = CardLayout::new(640, 480);
        let large = CardLayout::new(1280, 960);
        assert!(large.title_cap > small.title_cap);
        assert!(large.pad > small.pad);
        // The 640x480 grid is the design size: no upscaling below it.
        assert_eq!(small.title_cap, 18.0);
        assert_eq!(small.body_cap, 10.0);
    }

    #[test]
    fn buttons_sit_clear_of_the_frame() {
        let layout = CardLayout::new(1920, 1080);
        for rect in layout.button_rects(2) {
            assert!(rect.y + rect.h <= layout.height - layout.margin);
            assert!(rect.x >= layout.margin);
        }
    }

    #[test]
    fn button_labels_fit_inside_their_panels() {
        // The label uses the body face; a heading-sized label would overrun the
        // panel at every size, which is what this guards against.
        for (width, height) in [(640u32, 480u32), (800, 600), (1920, 1080), (2560, 1440)] {
            let layout = CardLayout::new(width, height);
            let rect = layout.button_rects(1)[0];
            assert!(layout.body_cap < rect.h as f32, "{width}x{height}");
            // A six-letter label at body size must leave room inside the panel.
            let budget = rect.w as f32 - 4.0 * layout.scale;
            assert!(
                layout.body_cap * 0.6 * 6.0 <= budget,
                "{width}x{height}: label {} vs panel {budget}",
                layout.body_cap * 0.6 * 6.0
            );
        }
    }
}
