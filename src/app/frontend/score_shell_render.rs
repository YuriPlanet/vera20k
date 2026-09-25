//! Paint the score dialog `0x108`: the paint-mode-1 dialog paint (the right
//! panel, then the side art at the dialog origin `[0xB0FC1C]`), the proc's
//! shaded rectangle (`0x005CA07F..0x005CA0FB`), and the children: the band
//! bars, the table texts, the heading, the monitor, the status line and
//! Continue.
//!
//! While the entry slide runs the bars and the monitor already show, and the
//! kind-1 texts wait for its end (retail still `g2-s2.png` mid-slide). The
//! teardown slide starts with a full dialog repaint (`0x00622C4F`) whose
//! shaded rectangle follows only once the slide has ended (the proc paints
//! after the common handler returns), and pumps no messages: the art and the
//! column.

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::shell_pass::{
    ShellComposition, TexturedDraw, encode_shell_pass, owner_draw_button_label_rect, resolve_csf,
    software_cursor,
};
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::render::shell_paint::{
    self, CURSOR_DEPTH, PARENT_BACKGROUND_DEPTH, PaintButton, PaintLabel, SHELL_TEXT_RGB_ENABLED,
    STATIC_IMAGE_DEPTH, push_entry_crop, push_entry_native,
};
use crate::render::shell_text::ShellAlign;
use crate::render::shell_text_reveal::PathAReveal;
use crate::ui::score_shell::{CONTINUE_BUTTON, SCORE_PAGE, ScoreAlign, compute_layout};
use crate::ui::shell::geom::{centred_in_window, dialog_origin};

/// The shaded part of the art sits over the art.
const SHADE_DEPTH: f32 = PARENT_BACKGROUND_DEPTH - 0.00001;

/// The shell's text colour `[0xAC18A4]` (`0x0060FA3F`) and the Path-A
/// highlight every kind-1 static trails.
const SHELL_TEXT: [u8; 3] = [255, 255, 0];
const HIGHLIGHT: [u8; 3] = [255, 255, 255];

/// The heading `0x694` the proc sets (`0x005C9BEC..0x005C9C34`).
pub(crate) fn score_title_text(state: &AppState) -> String {
    state
        .frontend
        .score_page
        .as_ref()
        .map(|page| resolve_csf(state, page.model.title_key).into_owned())
        .unwrap_or_default()
}

fn rgb_f32(rgb: [u8; 3]) -> [f32; 3] {
    rgb.map(|channel| f32::from(channel) / 255.0)
}

/// Paint dialog `0x108`. Returns `false` when its chrome or art is missing.
pub(crate) fn render_score_page(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if state.frontend.main_menu_shell_chrome.is_none()
        || state.frontend.score_page.is_none()
        || state.frontend.score_art.is_none()
    {
        return Ok(false);
    }
    let layout = compute_layout(
        state.renderer.gpu.config.width,
        state.renderer.gpu.config.height,
    );
    let screen = layout.page.screen;
    let exit_wave =
        crate::app::frontend::shell_transition::shell_exit_wave(state, ShellSlideKind::Score)
            .cloned();
    let leaving = exit_wave.is_some();
    let wave = exit_wave.or_else(|| state.frontend.shell_first_paint_slide.clone());
    // Children paint until the teardown repaint covers them; the kind-1
    // texts stay hidden until the entry slide's end starts them.
    let children = !leaving;

    let now = std::time::Instant::now();
    let monitor_frame = if children {
        crate::app::frontend::menu_page_render::paint_shell_monitor(state)
    } else {
        None
    };
    let mut labels: Vec<PaintLabel<'static>> = Vec::new();
    if children {
        if let Some(window) = state.frontend.shell_page_title.paint(now) {
            labels.push(PaintLabel {
                text: score_title_text(state).into(),
                rect: layout.page.title,
                align: ShellAlign::H_CENTER,
                rgb: SHELL_TEXT_RGB_ENABLED,
                path_a_reveal: Some(PathAReveal {
                    count: window.count,
                    range: window.range,
                    base_rgb: SHELL_TEXT,
                    highlight_rgb: HIGHLIGHT,
                }),
            });
        }
        let controller = &state.frontend.shell_controller;
        let hovered = (controller.top_id() == Some(SCORE_PAGE.dialog))
            .then(|| controller.hovered())
            .flatten();
        let status_text =
            crate::app::frontend::menu_page_render::status_csf_key(&SCORE_PAGE, hovered)
                .map(|key| resolve_csf(state, key).into_owned())
                .unwrap_or_default();
        labels.extend(
            crate::app::frontend::menu_page_render::paint_shell_status_line(
                state,
                status_text,
                layout.page.status_help,
            ),
        );
        let page = state
            .frontend
            .score_page
            .as_mut()
            .expect("checked before render");
        for text in page.table_mut() {
            let Some(window) = text.paint(now) else {
                continue;
            };
            let base = text.rgb.unwrap_or(SHELL_TEXT);
            labels.push(PaintLabel {
                text: text.text().to_owned().into(),
                rect: text.window,
                align: match text.align {
                    ScoreAlign::Left => ShellAlign::NONE,
                    ScoreAlign::Right => ShellAlign::H_RIGHT,
                },
                rgb: rgb_f32(base),
                path_a_reveal: Some(PathAReveal {
                    count: window.count,
                    range: window.range,
                    base_rgb: base,
                    highlight_rgb: HIGHLIGHT,
                }),
            });
        }
    }

    let chrome = state
        .frontend
        .main_menu_shell_chrome
        .as_ref()
        .expect("checked before render");
    let art = state
        .frontend
        .score_art
        .as_ref()
        .expect("ensured before render");
    let page = state
        .frontend
        .score_page
        .as_ref()
        .expect("checked before render");

    // Paint mode 1 (`0x00621E90`): the right panel, then the dialog's own
    // background at the dialog origin.
    let mut chrome_sprites = shell_paint::paint_chrome(
        chrome,
        layout.page.right_panel,
        Some(layout.page.lower_strip),
        screen.w,
    );
    let origin = dialog_origin(screen.w, screen.h);
    let mut art_sprites = Vec::new();
    if let Some(background) = art.background {
        push_entry_native(
            &mut art_sprites,
            background,
            origin.0,
            origin.1,
            PARENT_BACKGROUND_DEPTH,
        );
    }
    if !leaving
        && let (Some(shaded), Some(rect)) = (
            art.background_shaded,
            layout.table.shaded_rect(page.model.rows.len()),
        )
    {
        push_entry_crop(&mut art_sprites, shaded, origin, rect, SHADE_DEPTH);
    }
    if children {
        // Bands 0 and 1 always carry a bar; player bands only up to the row
        // count (`0x005C9D82..0x005C9DD1`, `0x005C9E4C..0x005C9E7A`).
        let bands = 2 + page.model.rows.len();
        for (window, bar) in layout.table.bands.iter().zip(&art.bars).take(bands) {
            let Some(bar) = *bar else {
                continue;
            };
            let (w, h) = bar.native_size();
            let (x, y) = centred_in_window(*window, w, h);
            push_entry_native(&mut art_sprites, bar, x, y, STATIC_IMAGE_DEPTH);
        }
    }
    chrome_sprites.extend(monitor_frame.and_then(|frame| {
        shell_paint::paint_warning_monitor(chrome, layout.page.warning_monitor, frame)
    }));

    // The slide engine draws the whole column while either slide runs
    // (`0x006071E0`); otherwise Continue paints.
    let controller = &state.frontend.shell_controller;
    let active = controller.top_id() == Some(SCORE_PAGE.dialog);
    let pressed = active.then(|| controller.pressed()).flatten();
    let buttons: Vec<PaintButton> = match wave.as_ref() {
        Some(wave) => {
            chrome_sprites.extend(shell_paint::paint_slide_column(
                chrome,
                layout.page.right_panel,
                &wave.button_draws(),
            ));
            Vec::new()
        }
        None => vec![PaintButton {
            rect: layout.continue_button(),
            pressed: pressed == Some(CONTINUE_BUTTON),
            hovered: false,
            enabled: true,
        }],
    };
    let button_sprites = shell_paint::paint_buttons(
        chrome,
        &buttons,
        crate::app::frontend::menu_page_render::MENU_PAGE_BUTTON_POLICY,
        now,
        None,
    );
    if wave.is_none() {
        labels.push(PaintLabel {
            text: resolve_csf(state, SCORE_PAGE.back.csf_key)
                .into_owned()
                .into(),
            rect: owner_draw_button_label_rect(
                layout.continue_button(),
                pressed == Some(CONTINUE_BUTTON),
            ),
            align: ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0),
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }
    let text = shell_paint::paint_labels(&state.renderer.bit_font, &labels);

    let draws = [
        TexturedDraw {
            texture: &chrome.texture,
            instances: chrome_sprites,
        },
        TexturedDraw {
            texture: &art.texture,
            instances: art_sprites,
        },
        TexturedDraw {
            texture: &chrome.texture,
            instances: button_sprites,
        },
    ];
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Score Shell",
        ShellComposition {
            draws: &draws,
            text: &text,
            cursor: software_cursor(state, CURSOR_DEPTH),
            effects: None,
        },
    );
    Ok(true)
}
