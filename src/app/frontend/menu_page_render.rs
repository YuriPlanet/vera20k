//! Render glue for the RA2TS-bearing right-panel menu pages: Single Player
//! `0x100` and Movies & Credits `0x101` (`ui::shell::menu_page`).

use std::time::Instant;

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::main_menu_shell_render::Ra2tsDialogOwner;
use crate::app::frontend::shell_transition::{ButtonGroup, ShellFrameWave};
use crate::render::batch::SpriteInstance;
use crate::render::shell_paint::{
    self, ArtFit, ButtonPolicy, CURSOR_DEPTH, MOVIE_DEPTH, PaintButton, PaintLabel,
    SHELL_TEXT_RGB_DISABLED, SHELL_TEXT_RGB_ENABLED,
};
use crate::render::shell_text::ShellAlign;
use crate::render::shell_transition_pass::ShellRenderTarget;
use crate::ui::main_menu_shell::RectPx;
use crate::ui::shell::menu_page::{MenuPageLayout, MenuPageSpec, compute_layout};

/// Menu pages paint the native 156x42 SDBTNANM frame at the control origin.
/// Mouse hover updates static 0x695 but does not select frame 3. Press selects
/// frame 4 without moving the art; a runtime-disabled button remains dimmed.
const MENU_PAGE_BUTTON_POLICY: ButtonPolicy = ButtonPolicy {
    art_fit: ArtFit::Native,
    hover_flash: false,
    art_sink_y: 0.0,
    disabled_dim: true,
};
const MENU_PAGE_BUTTON_ALIGN: ShellAlign =
    ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0);
const MENU_PAGE_STATUS_ALIGN: ShellAlign = ShellAlign::V_CENTER;

pub(crate) enum MenuPageRenderResult {
    Rendered,
    Fallback,
}

/// One page to paint: its resource table, the dialog owning the RA2TS static
/// `0x71A`, and the controls its dialog proc has runtime-disabled.
pub(crate) struct MenuPageView<'a> {
    pub spec: &'static MenuPageSpec,
    pub movie_owner: Ra2tsDialogOwner,
    pub disabled: &'a [u16],
}

/// Press/hover for the page, read from the shared controller only while the
/// page is its top dialog.
#[derive(Debug, Clone, Copy, Default)]
struct PageInput {
    pressed: Option<u16>,
    hovered: Option<u16>,
}

fn page_input(state: &AppState, spec: &MenuPageSpec) -> PageInput {
    let controller = &state.frontend.shell_controller;
    if controller.top_id() != Some(spec.dialog) {
        return PageInput::default();
    }
    PageInput {
        pressed: controller.pressed(),
        hovered: controller.hovered(),
    }
}

fn resolve_csf<'a>(state: &'a AppState, key: &'static str) -> std::borrow::Cow<'a, str> {
    state
        .process_assets
        .csf
        .as_ref()
        .map(|csf| csf.text(key))
        .unwrap_or(std::borrow::Cow::Borrowed(key))
}

/// Owner-draw button list for the paint pass. A disabled control can never
/// paint pressed; during a first-paint slide every button rides Group A's ramp.
fn paint_buttons(
    layout: &MenuPageLayout,
    input: PageInput,
    disabled: &[u16],
    wave: Option<&ShellFrameWave>,
) -> Vec<PaintButton> {
    layout
        .buttons
        .iter()
        .enumerate()
        .map(|(slot, button)| {
            let enabled = !disabled.contains(&button.id);
            let wave_frame = wave.map(|w| w.sdbtnanm_frame(slot as u32, ButtonGroup::A));
            PaintButton {
                rect: button.rect,
                pressed: enabled && input.pressed == Some(button.id),
                hovered: enabled && input.hovered == Some(button.id),
                enabled,
                wave_frame,
            }
        })
        .collect()
}

/// Native owner-draw label clip: unpressed `(x, y+1, w-2, h-1)`, pressed
/// `(x+2, y+5, w-4, h-5)`.
fn owner_draw_button_label_rect(rect: RectPx, pressed: bool) -> RectPx {
    let (dx, dy) = if pressed { (2, 5) } else { (0, 1) };
    RectPx::new(
        rect.x + dx,
        rect.y + dy,
        (rect.w - 2 - dx).max(0),
        (rect.h - dy).max(0),
    )
}

/// Status help for the hovered control. Hover is enable-unfiltered, so a
/// disabled button still writes its help text.
fn status_csf_key(spec: &MenuPageSpec, hovered: Option<u16>) -> Option<&'static str> {
    hovered
        .and_then(|id| spec.button(id))
        .map(|button| button.tooltip_key)
}

/// Button captions, heading, and the immediate 0x695 hover-help static.
fn paint_labels<'a>(
    state: &'a AppState,
    view: &MenuPageView<'_>,
    layout: &MenuPageLayout,
    input: PageInput,
) -> Vec<PaintLabel<'a>> {
    let mut out = Vec::with_capacity(layout.buttons.len() + 2);
    for button in &layout.buttons {
        let Some(spec_button) = view.spec.button(button.id) else {
            continue;
        };
        let enabled = !view.disabled.contains(&button.id);
        let pressed = enabled && input.pressed == Some(button.id);
        out.push(PaintLabel {
            text: resolve_csf(state, spec_button.csf_key),
            rect: owner_draw_button_label_rect(button.rect, pressed),
            align: MENU_PAGE_BUTTON_ALIGN,
            rgb: if enabled {
                SHELL_TEXT_RGB_ENABLED
            } else {
                SHELL_TEXT_RGB_DISABLED
            },
            path_a_reveal: None,
        });
    }
    out.push(PaintLabel {
        text: resolve_csf(state, view.spec.title_key),
        rect: layout.title,
        align: ShellAlign::H_CENTER,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: None,
    });
    if let Some(key) = status_csf_key(view.spec, input.hovered) {
        out.push(PaintLabel {
            text: resolve_csf(state, key),
            rect: layout.status_help,
            align: MENU_PAGE_STATUS_ALIGN,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }
    out
}

fn movie_instance(layout: &MenuPageLayout) -> SpriteInstance {
    SpriteInstance {
        position: [layout.movie.x as f32, layout.movie.y as f32],
        size: [layout.movie.w as f32, layout.movie.h as f32],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        depth: MOVIE_DEPTH,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    }
}

/// Software-cursor sprite in screen space (camera at the origin): the raw
/// pointer minus the default cursor hotspot. `None` without a software cursor.
fn shell_cursor_instance(state: &AppState) -> Option<SpriteInstance> {
    let cursor = state
        .match_state
        .match_presentation
        .software_cursor
        .as_ref()?;
    let sequence = cursor.get(crate::app::types::CursorId::Default)?;
    let frame = crate::app::input::cursor::current_software_cursor_frame(sequence)?;
    Some(SpriteInstance {
        position: [
            state.match_state.input.cursor_x - sequence.hotspot[0],
            state.match_state.input.cursor_y - sequence.hotspot[1],
        ],
        size: [frame.width, frame.height],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        depth: CURSOR_DEPTH,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    })
}

/// Which menu page owns the `MainMenu` screen, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveMenuPage {
    SinglePlayer,
    MoviesAndCredits,
}

impl ActiveMenuPage {
    pub(crate) fn from_state(state: &AppState) -> Option<Self> {
        if state.frontend.screen != crate::ui::game_screen::GameScreen::MainMenu {
            return None;
        }
        let route = state.frontend.shell_route;
        if route.single_player() {
            Some(Self::SinglePlayer)
        } else if route.movies_and_credits() {
            Some(Self::MoviesAndCredits)
        } else {
            None
        }
    }

    pub(crate) fn spec(self) -> &'static MenuPageSpec {
        match self {
            Self::SinglePlayer => &crate::ui::single_player_shell::SINGLE_PLAYER_PAGE,
            Self::MoviesAndCredits => &crate::ui::movies_credits_shell::MOVIES_CREDITS_PAGE,
        }
    }

    fn movie_owner(self) -> Ra2tsDialogOwner {
        match self {
            Self::SinglePlayer => Ra2tsDialogOwner::SinglePlayer0x100,
            Self::MoviesAndCredits => Ra2tsDialogOwner::MoviesAndCredits0x101,
        }
    }
}

/// Controls the page's dialog proc has runtime-disabled: Single Player
/// disables Load Saved Game on `0x497` when no loadable save exists.
const LOAD_SAVED_GAME_DISABLED: &[u16] = &[0x0689];

/// Paint whichever menu page the route selects.
pub(crate) fn render_active_menu_page(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<MenuPageRenderResult> {
    let Some(page) = ActiveMenuPage::from_state(state) else {
        return Ok(MenuPageRenderResult::Fallback);
    };
    let disabled: &[u16] = match page {
        ActiveMenuPage::SinglePlayer
            if !state
                .frontend
                .single_player_shell_state
                .load_saved_game_enabled =>
        {
            LOAD_SAVED_GAME_DISABLED
        }
        _ => &[],
    };
    render_menu_page(
        state,
        encoder,
        destination,
        MenuPageView {
            spec: page.spec(),
            movie_owner: page.movie_owner(),
            disabled,
        },
    )
}

pub(crate) fn render_menu_page(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
    view: MenuPageView<'_>,
) -> Result<MenuPageRenderResult> {
    crate::app::frontend::main_menu_shell_render::ensure_movie_for_current_layout(
        state,
        view.movie_owner,
    )?;
    if state.frontend.main_menu_shell_failed || state.frontend.main_menu_shell_chrome.is_none() {
        state.frontend.main_menu_shell_failed = true;
        return Ok(MenuPageRenderResult::Fallback);
    }

    if let Some(movie) = state.frontend.main_menu_movie.as_mut() {
        let now = Instant::now();
        let elapsed = now
            .duration_since(state.frontend.main_menu_movie_last_step)
            .as_secs_f64();
        state.frontend.main_menu_movie_last_step = now;
        if let Err(err) = movie.step(&state.renderer.gpu, elapsed) {
            log::warn!(
                "Failed to step RA2TS movie for dialog 0x{:X}: {err:#}",
                view.spec.dialog.0
            );
            state.frontend.main_menu_shell_failed = true;
            return Ok(MenuPageRenderResult::Fallback);
        }
    }

    let color = state.renderer.shell_surface_presenter.source_render_view();
    let depth = state.renderer.depth_view.clone();
    let target = ShellRenderTarget {
        color: &color,
        depth: &depth,
    };
    let layout = compute_layout(
        view.spec,
        state.renderer.gpu.config.width,
        state.renderer.gpu.config.height,
    );
    let input = page_input(state, view.spec);
    // While a first-paint slide is live the buttons animate through their
    // SDBTNANM ramp frames; off-slide this is None and they paint steady-state.
    let wave = state.frontend.shell_first_paint_slide.clone();
    let chrome = state
        .frontend
        .main_menu_shell_chrome
        .as_ref()
        .expect("checked before render");
    let movie_texture = state
        .frontend
        .main_menu_movie
        .as_ref()
        .map(|movie| movie.batch_texture())
        .expect("movie loaded before render");

    // Menu pages have NO parent background; the movie is submitted first.
    let movie_instances = vec![movie_instance(&layout)];
    let chrome_instances = shell_paint::paint_chrome(
        chrome,
        layout.right_panel,
        Some(layout.lower_strip),
        layout.screen.w,
    );
    let buttons = paint_buttons(&layout, input, view.disabled, wave.as_ref());
    let button_instances = shell_paint::paint_buttons(
        chrome,
        &buttons,
        MENU_PAGE_BUTTON_POLICY,
        Instant::now(),
        None,
    );
    let labels = paint_labels(state, &view, &layout, input);
    let text_draws = shell_paint::paint_labels(&state.renderer.bit_font, &labels);

    state.renderer.batch_renderer.update_camera(
        &state.renderer.gpu,
        state.renderer.gpu.config.width as f32,
        state.renderer.gpu.config.height as f32,
        0.0,
        0.0,
        1.0,
        crate::render::batch::DepthAxis::NONE,
    );
    let batch = &state.renderer.batch_renderer;
    let gpu = &state.renderer.gpu;
    let movie_buffer = batch.create_instance_buffer(gpu, &movie_instances);
    let chrome_buffer = batch.create_instance_buffer(gpu, &chrome_instances);
    let button_buffer = batch.create_instance_buffer(gpu, &button_instances);
    let text_buffers: Vec<_> = text_draws
        .iter()
        .map(|draw| batch.create_instance_buffer(gpu, &draw.instances))
        .collect();
    let cursor_instances: Vec<SpriteInstance> = shell_cursor_instance(state).into_iter().collect();
    let cursor_buffer = batch.create_instance_buffer(gpu, &cursor_instances);
    // Default-cursor frame-0 texture, borrowed for the duration of the pass.
    let cursor_texture = state
        .match_state
        .match_presentation
        .software_cursor
        .as_ref()
        .and_then(|cursor| cursor.get(crate::app::types::CursorId::Default))
        .and_then(|sequence| sequence.frames.first())
        .map(|frame| &frame.texture);

    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Menu Page Shell"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target.color,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(crate::app::types::CLEAR_COLOR),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: target.depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    if let Some((buffer, count)) = movie_buffer.as_ref() {
        batch.draw_with_buffer_passthrough(&mut pass, movie_texture, buffer, *count);
    }
    if let Some((buffer, count)) = chrome_buffer.as_ref() {
        batch.draw_with_buffer_passthrough(&mut pass, &chrome.texture, buffer, *count);
    }
    if let Some((buffer, count)) = button_buffer.as_ref() {
        batch.draw_with_buffer_passthrough(&mut pass, &chrome.texture, buffer, *count);
    }
    for (draw, buffer) in text_draws.iter().zip(text_buffers.iter()) {
        let Some((buffer, count)) = buffer.as_ref() else {
            continue;
        };
        pass.set_scissor_rect(
            draw.scissor.x,
            draw.scissor.y,
            draw.scissor.w,
            draw.scissor.h,
        );
        batch.draw_with_buffer_passthrough(
            &mut pass,
            state.renderer.bit_font.atlas(),
            buffer,
            *count,
        );
    }
    pass.set_scissor_rect(0, 0, gpu.config.width, gpu.config.height);
    // Software cursor draws last, on top of all chrome/controls.
    if let (Some((buffer, count)), Some(texture)) = (cursor_buffer.as_ref(), cursor_texture) {
        batch.draw_with_buffer_passthrough(&mut pass, texture, buffer, *count);
    }
    drop(pass);
    state
        .renderer
        .shell_surface_presenter
        .encode_present(encoder, destination);

    Ok(MenuPageRenderResult::Rendered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::single_player_shell::SINGLE_PLAYER_PAGE;

    #[test]
    fn menu_page_policy_uses_native_art_without_mouse_hover_flash() {
        assert!(matches!(MENU_PAGE_BUTTON_POLICY.art_fit, ArtFit::Native));
        assert!(!MENU_PAGE_BUTTON_POLICY.hover_flash);
        assert_eq!(MENU_PAGE_BUTTON_POLICY.art_sink_y, 0.0);
        assert!(MENU_PAGE_BUTTON_POLICY.disabled_dim);
    }

    #[test]
    fn owner_draw_label_clip_matches_native_up_and_pressed_rects() {
        let button = RectPx::new(644, 199, 156, 42);
        assert_eq!(
            owner_draw_button_label_rect(button, false),
            RectPx::new(644, 200, 154, 41)
        );
        assert_eq!(
            owner_draw_button_label_rect(button, true),
            RectPx::new(646, 204, 152, 37)
        );
    }

    #[test]
    fn status_help_is_immediate_and_includes_disabled_load() {
        assert_eq!(status_csf_key(&SINGLE_PLAYER_PAGE, None), None);
        assert_eq!(
            status_csf_key(&SINGLE_PLAYER_PAGE, Some(0x0689)),
            Some("STT:SingleButtonLoadSavedGame")
        );
        assert!(MENU_PAGE_STATUS_ALIGN.contains(ShellAlign::V_CENTER));
        assert!(!MENU_PAGE_STATUS_ALIGN.contains(ShellAlign::H_CENTER));
    }

    #[test]
    fn disabled_page_button_never_paints_pressed_or_hovered() {
        let layout = crate::ui::single_player_shell::compute_layout(800, 600);
        let input = PageInput {
            pressed: Some(0x0689),
            hovered: Some(0x0689),
        };
        let buttons = paint_buttons(&layout, input, &[0x0689], None);
        let load = buttons
            .iter()
            .zip(&layout.buttons)
            .find(|(_, laid)| laid.id == 0x0689)
            .map(|(button, _)| button)
            .expect("load button");
        assert!(!load.enabled && !load.pressed && !load.hovered);
    }

    #[test]
    fn gsi_13_26_menu_page_steady_frame_uses_rgb565_presenter_after_full_composition() {
        let source = include_str!("menu_page_render.rs");
        let production = source
            .split_once("#[cfg(test)]")
            .expect("test module follows production renderer")
            .0;
        let renderer = &production[production
            .find("pub(crate) fn render_menu_page")
            .expect("production renderer")..];

        assert!(renderer.contains("destination: &wgpu::Texture"));
        assert!(!renderer.contains("target: &wgpu::TextureView"));
        let source_view = renderer
            .find("shell_surface_presenter.source_render_view()")
            .expect("RGB565 presenter source view");
        let fallback_returns: Vec<_> = renderer
            .match_indices("return Ok(MenuPageRenderResult::Fallback)")
            .map(|(index, _)| index)
            .collect();
        let render_pass = renderer
            .find("encoder.begin_render_pass")
            .expect("complete shell render pass");
        let cursor = renderer
            .find("Software cursor draws last")
            .expect("software cursor submission");
        let pass_end = renderer.find("drop(pass);").expect("render pass end");
        let present = renderer
            .find(".encode_present(encoder, destination);")
            .expect("RGB565 encode/present");

        assert_eq!(fallback_returns.len(), 2);
        assert!(fallback_returns.iter().all(|&index| index < source_view));
        assert!(source_view < render_pass);
        assert!(render_pass < cursor);
        assert!(cursor < pass_end);
        assert!(pass_end < present);
    }
}
