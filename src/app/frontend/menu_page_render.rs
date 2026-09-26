//! Render glue for the RA2TS-bearing right-panel menu pages: Single Player
//! `0x100` and Movies & Credits `0x101` (`ui::shell::menu_page`).

use std::time::Instant;

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::main_menu_shell_render::{Ra2tsDialogOwner, shell_reveal_path_a};
use crate::app::frontend::shell_pass::{
    ShellComposition, TexturedDraw, encode_shell_pass, owner_draw_button_label_rect, resolve_csf,
    software_cursor,
};
use crate::render::batch::SpriteInstance;
use crate::render::shell_paint::{
    self, ArtFit, ButtonPolicy, CURSOR_DEPTH, MOVIE_DEPTH, PaintButton, PaintLabel,
    SHELL_TEXT_RGB_DISABLED, SHELL_TEXT_RGB_ENABLED,
};
use crate::render::shell_text::ShellAlign;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::menu_page::{MenuPageLayout, MenuPageSpec, compute_layout};
use crate::ui::shell::static_reveal::Kind1RevealWindow;

/// Menu pages paint the native 156x42 SDBTNANM frame at the control origin.
/// Mouse hover updates static 0x695 but does not select frame 3. Press selects
/// frame 4 without moving the art; a runtime-disabled button keeps its art and
/// shows its caption in the disabled colour (owner-draw type 1, `0x006135F3`).
pub(crate) const MENU_PAGE_BUTTON_POLICY: ButtonPolicy = ButtonPolicy {
    art_fit: ArtFit::Native,
    hover_flash: false,
    art_sink_y: 0.0,
};
const MENU_PAGE_BUTTON_ALIGN: ShellAlign =
    ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0);

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

/// Owner-draw button list for the steady paint pass. A disabled control can
/// never paint pressed.
fn paint_buttons(layout: &MenuPageLayout, input: PageInput, disabled: &[u16]) -> Vec<PaintButton> {
    layout
        .buttons
        .iter()
        .map(|button| {
            let enabled = !disabled.contains(&button.id);
            PaintButton {
                rect: button.rect,
                pressed: enabled && input.pressed == Some(button.id),
                hovered: enabled && input.hovered == Some(button.id),
                enabled,
            }
        })
        .collect()
}

/// Status help for the hovered control. Hover is enable-unfiltered, so a
/// disabled button still writes its help text.
pub(crate) fn status_csf_key(spec: &MenuPageSpec, hovered: Option<u16>) -> Option<&'static str> {
    hovered
        .and_then(|id| spec.button(id))
        .map(|button| button.tooltip_key)
}

/// Button captions (none while a slide draws the frames) and the heading;
/// the status line comes from [`paint_shell_status_line`].
fn paint_labels<'a>(
    state: &'a AppState,
    view: &MenuPageView<'_>,
    layout: &MenuPageLayout,
    input: PageInput,
    title_window: Option<Kind1RevealWindow>,
    captions: bool,
) -> Vec<PaintLabel<'a>> {
    let mut out = Vec::with_capacity(layout.buttons.len() + 2);
    for button in layout.buttons.iter().filter(|_| captions) {
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
    if let Some(window) = title_window {
        out.push(PaintLabel {
            text: resolve_csf(state, view.spec.title_key),
            rect: layout.title,
            align: ShellAlign::H_CENTER,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: Some(shell_reveal_path_a(window)),
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

/// Advance the showing family dialog's `0x71C` for this recomposition; the
/// frame loop commits it after present. No timer reaches the static while the
/// dialog's first-paint slide runs.
pub(crate) fn paint_shell_monitor(state: &mut AppState) -> Option<usize> {
    let frames = state
        .frontend
        .main_menu_shell_chrome
        .as_ref()
        .map_or(0, |chrome| chrome.warning_monitor_frames.len());
    let timers = !crate::app::frontend::shell_transition::shell_slide_running(state);
    state
        .frontend
        .shell_monitor
        .paint(Instant::now(), frames, timers)
}

/// Status line `0x695` for this recomposition: deliver the hover help text
/// (`0x4B2`, empty off every control) and return the label to draw, `None`
/// while the static is hidden or blank. Kind-1 paint passes left alignment
/// only (`0x00615A91`), so the text is top-left in the window.
pub(crate) fn paint_shell_status_line(
    state: &mut AppState,
    text: String,
    window: RectPx,
) -> Option<PaintLabel<'static>> {
    let now = Instant::now();
    let status = &mut state.frontend.shell_status_line;
    status.set_text(&text, now);
    let reveal = status.paint(now)?;
    (!text.is_empty()).then(|| PaintLabel {
        text: text.into(),
        rect: window,
        align: ShellAlign::NONE,
        rgb: SHELL_TEXT_RGB_ENABLED,
        path_a_reveal: Some(shell_reveal_path_a(reveal)),
    })
}

/// The slide kind of a menu page.
fn page_slide_kind(spec: &MenuPageSpec) -> crate::app::frontend::shell_transition::ShellSlideKind {
    use crate::app::frontend::shell_transition::ShellSlideKind;
    if spec.dialog == crate::ui::single_player_shell::SINGLE_PLAYER_PAGE.dialog {
        ShellSlideKind::SinglePlayer
    } else {
        ShellSlideKind::MoviesAndCredits
    }
}

/// Heading text of the menu page a first-paint slide belongs to.
pub(crate) fn active_page_title_text(
    state: &AppState,
    kind: crate::app::frontend::shell_transition::ShellSlideKind,
) -> String {
    use crate::app::frontend::shell_transition::ShellSlideKind;
    let key = match kind {
        ShellSlideKind::SinglePlayer => {
            crate::ui::single_player_shell::SINGLE_PLAYER_PAGE.title_key
        }
        ShellSlideKind::MoviesAndCredits => {
            crate::ui::movies_credits_shell::MOVIES_CREDITS_PAGE.title_key
        }
        ShellSlideKind::MovieList => crate::ui::movies_credits_shell::MOVIE_LIST_PAGE.title_key,
        ShellSlideKind::Campaign => crate::ui::campaign_shell::CAMPAIGN_PAGE.title_key,
        ShellSlideKind::LoadSavedGame => {
            crate::ui::shell::saved_games::LOAD_SAVED_GAME_PAGE.title_key
        }
        ShellSlideKind::WolWelcome => crate::ui::wol_shell::WOL_WELCOME_PAGE.title_key,
        ShellSlideKind::Keyboard => crate::ui::shell::keyboard::KEYBOARD_PAGE.title_key,
        ShellSlideKind::ChooseMap => crate::ui::skirmish_shell::CHOOSE_MAP_TITLE_KEY,
        ShellSlideKind::RandomMap => crate::ui::skirmish_shell::RANDOM_MAP_TITLE_KEY,
        // The Options heading is the dialog's own resolved label.
        ShellSlideKind::Options => {
            return state
                .frontend
                .options_dialog
                .as_ref()
                .map(|dialog| dialog.shell_title_text().to_owned())
                .unwrap_or_default();
        }
        // These dialogs set their headings themselves.
        ShellSlideKind::MainMenu | ShellSlideKind::Skirmish | ShellSlideKind::Score => {
            return String::new();
        }
    };
    resolve_csf(state, key).into_owned()
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
        // Play_Movie and Show_Credits run after the page is destroyed.
        if state.frontend.fullscreen_movie.is_some() || state.frontend.credits_roll.is_some() {
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

    // The page's slide, if one runs: its teardown slide-out or its entry
    // slide. Either way the buttons animate through their SDBTNANM ramp
    // frames; off-slide they paint steady-state.
    let exit_wave =
        crate::app::frontend::shell_transition::shell_exit_wave(state, page_slide_kind(view.spec))
            .cloned();
    let leaving = exit_wave.is_some();
    let wave = exit_wave.or_else(|| state.frontend.shell_first_paint_slide.clone());
    // While either slide runs the RA2TS static shows no movie and gets no
    // timer (`0x006071E0`; the teardown also stops it with 0x4E2), so the
    // shell background shows and the movie clock starts after the slide.
    let sliding = wave.is_some();
    if sliding {
        state.frontend.main_menu_movie_last_step = Instant::now();
    } else if let Some(movie) = state.frontend.main_menu_movie.as_mut() {
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

    let layout = compute_layout(
        view.spec,
        state.renderer.gpu.config.width,
        state.renderer.gpu.config.height,
    );
    let input = page_input(state, view.spec);
    // The teardown slide starts with a full dialog repaint (`0x00622C4F`)
    // and pumps no messages until it ends: the statics stay blank and the
    // monitor window shows the right panel's own art.
    let monitor_frame = if leaving {
        None
    } else {
        paint_shell_monitor(state)
    };
    let (title_window, status_label) = if leaving {
        (None, None)
    } else {
        let title_window = state.frontend.shell_page_title.paint(Instant::now());
        let status_text = status_csf_key(view.spec, input.hovered)
            .map(|key| resolve_csf(state, key).into_owned())
            .unwrap_or_default();
        let status_label = paint_shell_status_line(state, status_text, layout.status_help);
        (title_window, status_label)
    };
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

    // Menu pages have no parent background under the movie; while a slide
    // runs the RA2TS area shows the shell background instead.
    let backdrop = if sliding {
        crate::app::frontend::main_menu_shell_render::shell_parent_background_instances(
            chrome,
            layout.screen.w,
            layout.screen.h,
        )
    } else {
        Vec::new()
    };
    let mut chrome_instances = shell_paint::paint_chrome(
        chrome,
        layout.right_panel,
        Some(layout.lower_strip),
        layout.screen.w,
    );
    chrome_instances.extend(monitor_frame.and_then(|frame| {
        shell_paint::paint_warning_monitor(chrome, layout.warning_monitor, frame)
    }));
    // While a slide runs the engine draws the whole tile column in place of
    // the buttons (`0x006071E0`).
    let buttons = match wave.as_ref() {
        Some(wave) => {
            chrome_instances.extend(shell_paint::paint_slide_column(
                chrome,
                layout.right_panel,
                &wave.button_draws(),
            ));
            Vec::new()
        }
        None => paint_buttons(&layout, input, view.disabled),
    };
    let button_instances = shell_paint::paint_buttons(
        chrome,
        &buttons,
        MENU_PAGE_BUTTON_POLICY,
        Instant::now(),
        None,
    );
    let mut labels = paint_labels(state, &view, &layout, input, title_window, !sliding);
    labels.extend(status_label);
    let text = shell_paint::paint_labels(&state.renderer.bit_font, &labels);
    let draws = [
        TexturedDraw {
            texture: movie_texture,
            instances: if sliding {
                Vec::new()
            } else {
                vec![movie_instance(&layout)]
            },
        },
        TexturedDraw {
            texture: &chrome.texture,
            instances: backdrop,
        },
        TexturedDraw {
            texture: &chrome.texture,
            instances: chrome_instances,
        },
        TexturedDraw {
            texture: &chrome.texture,
            instances: button_instances,
        },
    ];
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Menu Page Shell",
        ShellComposition {
            draws: &draws,
            text: &text,
            cursor: software_cursor(state, CURSOR_DEPTH),
            effects: None,
        },
    );

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
    }

    #[test]
    fn status_help_includes_disabled_load() {
        assert_eq!(status_csf_key(&SINGLE_PLAYER_PAGE, None), None);
        assert_eq!(
            status_csf_key(&SINGLE_PLAYER_PAGE, Some(0x0689)),
            Some("STT:SingleButtonLoadSavedGame")
        );
    }

    #[test]
    fn disabled_page_button_never_paints_pressed_or_hovered() {
        let layout = crate::ui::single_player_shell::compute_layout(800, 600);
        let input = PageInput {
            pressed: Some(0x0689),
            hovered: Some(0x0689),
        };
        let buttons = paint_buttons(&layout, input, &[0x0689]);
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
        let fallback_returns: Vec<_> = renderer
            .match_indices("return Ok(MenuPageRenderResult::Fallback)")
            .map(|(index, _)| index)
            .collect();
        let pass = renderer
            .find("encode_shell_pass(")
            .expect("shared RGB565 composition pass");
        assert_eq!(fallback_returns.len(), 2);
        assert!(fallback_returns.iter().all(|&index| index < pass));

        // The shared pass composes everything, the cursor last, before the
        // RGB565 presentation.
        let shared = include_str!("shell_pass.rs");
        let encoder = &shared[shared
            .find("pub(crate) fn encode_shell_pass")
            .expect("shared encoder")..];
        let source_view = encoder
            .find("shell_surface_presenter.source_render_view()")
            .expect("RGB565 presenter source view");
        let render_pass = encoder
            .find("encoder.begin_render_pass")
            .expect("complete shell render pass");
        let cursor = encoder.find("if let (Some((texture, _))").expect("cursor");
        let pass_end = encoder.find("drop(pass);").expect("render pass end");
        let present = encoder.find("presenter.encode_present(").expect("present");
        assert!(source_view < render_pass);
        assert!(render_pass < cursor);
        assert!(cursor < pass_end);
        assert!(pass_end < present);

        // Steady dispatch: the page is presented to the swapchain texture,
        // then the egui overlay (save/load panel) draws on the view.
        let app_source = include_str!("../frame.rs");
        let dispatch = &app_source[app_source
            .find("ActiveMenuPage::from_state(state)")
            .expect("menu page steady dispatch")..];
        let shell_call = dispatch
            .find("render_active_menu_page")
            .expect("menu page renderer call");
        let overlay = dispatch
            .find("state.renderer.egui.end_frame_and_render")
            .expect("post-shell egui overlay");
        assert!(dispatch[shell_call..overlay].contains("&output.texture"));
        assert!(dispatch[overlay..].contains("&view"));
    }
}
