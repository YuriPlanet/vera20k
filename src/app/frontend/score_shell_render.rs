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
//! column. The panel, monitor, column, Continue, heading and status line are
//! the family compositor's ([`encode_family_page`]).

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::main_menu_shell_render::shell_reveal_path_a;
use crate::app::frontend::menu_page_render::{
    FamilyFrame, PageButton, encode_family_page, page_panel_state,
};
use crate::app::frontend::shell_pass::TexturedDraw;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::render::shell_paint::{
    PARENT_BACKGROUND_DEPTH, PaintButton, PaintLabel, SHELL_TEXT_RGB_ENABLED, STATIC_IMAGE_DEPTH,
    push_entry_crop, push_entry_native, push_entry_scaled, push_entry_window_crop,
};
use crate::render::shell_text::ShellAlign;
use crate::render::shell_text_reveal::PathAReveal;
use crate::ui::score_shell::{CONTINUE_BUTTON, NEXT_PAGE, PREVIOUS_PAGE, SCORE_PAGE, ScoreAlign};
use crate::ui::shell::geom::{centred_in_window, dialog_origin};
use crate::ui::shell::menu_page;

/// The shaded part of the art sits over the art.
const SHADE_DEPTH: f32 = PARENT_BACKGROUND_DEPTH - 0.00001;

/// The Path-A highlight every kind-1 static trails.
const HIGHLIGHT: [u8; 3] = [255, 255, 255];

/// The heading `0x694` the proc sets (`0x005C9BEC..0x005C9C34`).
pub(crate) fn score_title_text(state: &AppState) -> String {
    state
        .frontend
        .score_page
        .as_ref()
        .map(|page| state.csf_text(page.model.title_key))
        .unwrap_or_default()
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
    let layout = crate::app::frontend::shell_layout::score_layout(state);
    let (screen, scale, dialog) = (layout.page.screen, layout.page.scale, SCORE_PAGE.dialog);
    // Only Continue has status help; the paging pair has none.
    let mut panel = page_panel_state(
        state,
        ShellSlideKind::Score,
        true,
        |state| {
            let controller = &state.frontend.shell_controller;
            (controller.top_id() == Some(dialog))
                .then(|| menu_page::hovered_help(controller, dialog))
                .flatten()
        },
        layout.page.status_help,
    );
    // Children paint until the teardown repaint covers them; the kind-1
    // texts stay hidden until the entry slide's end starts them.
    let leaving = panel.leaving;
    let mut labels: Vec<PaintLabel<'static>> = Vec::new();
    if let Some(window) = panel.title.take() {
        labels.push(PaintLabel {
            text: score_title_text(state).into(),
            rect: layout.page.title,
            align: ShellAlign::H_CENTER,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: Some(shell_reveal_path_a(window)),
        });
    }
    let now = crate::util::time::Instant::now();
    let page = state
        .frontend
        .score_page
        .as_mut()
        .expect("checked before render");
    for text in page.table_mut().iter_mut().filter(|_| !leaving) {
        let Some(window) = text.paint(now) else {
            continue;
        };
        let base = text.rgb.unwrap_or([255, 255, 0]);
        labels.push(PaintLabel {
            text: text.text().to_owned().into(),
            // The statics carry the template windows `0x0060B950` keeps;
            // on a stretched page they map onto the window with the table.
            rect: scale.scale_rect(text.window),
            align: match text.align {
                ScoreAlign::Left => ShellAlign::NONE,
                ScoreAlign::Right => ShellAlign::H_RIGHT,
            },
            rgb: base.map(|channel| f32::from(channel) / 255.0),
            path_a_reveal: Some(PathAReveal {
                count: window.count,
                range: window.range,
                base_rgb: base,
                highlight_rgb: HIGHLIGHT,
            }),
        });
    }

    let page = state
        .frontend
        .score_page
        .as_ref()
        .expect("checked before render");
    let art = state
        .frontend
        .score_art
        .as_ref()
        .expect("checked before render");
    // Paint mode 1 (`0x00621E90`): the right panel, then the dialog's own
    // background at the dialog origin.
    let origin = dialog_origin(screen.w, screen.h);
    let mut art_sprites = Vec::new();
    if let Some(background) = art.background {
        if scale.is_retail() {
            push_entry_native(
                &mut art_sprites,
                background,
                origin.0,
                origin.1,
                PARENT_BACKGROUND_DEPTH,
            );
        } else {
            push_entry_scaled(
                &mut art_sprites,
                background,
                0,
                0,
                scale,
                PARENT_BACKGROUND_DEPTH,
            );
        }
    }
    if !leaving {
        let rows = page.visible_row_count();
        match (art.background_shaded, layout.table.shaded_rect(rows, scale)) {
            (Some(shaded), Some(rect)) if scale.is_retail() => {
                push_entry_crop(&mut art_sprites, shaded, origin, rect, SHADE_DEPTH)
            }
            (Some(shaded), Some(rect)) => {
                let window = scale.dialog_rect();
                push_entry_window_crop(&mut art_sprites, shaded, window, rect, SHADE_DEPTH)
            }
            _ => {}
        }
        // Bands 0 and 1 always carry a bar; player bands only up to the row
        // count (`0x005C9D82..0x005C9DD1`, `0x005C9E4C..0x005C9E7A`). The bar
        // image is centred by its authored window and stretched with it.
        let bands = 2 + rows;
        for (window, bar) in layout.table.bands.iter().zip(&art.bars).take(bands) {
            let Some(bar) = *bar else {
                continue;
            };
            let (w, h) = bar.native_size();
            let (x, y) = centred_in_window(scale.base_rect(*window), w, h);
            push_entry_scaled(&mut art_sprites, bar, x, y, scale, STATIC_IMAGE_DEPTH);
        }
    }

    // The slide engine draws the whole column while either slide runs
    // (`0x006071E0`); otherwise Continue and VERA's paging pair paint.
    let controller = &state.frontend.shell_controller;
    let pressed = (controller.top_id() == Some(dialog))
        .then(|| menu_page::pressed_id(controller, dialog))
        .flatten();
    let paged = page.page_count() > 1;
    let ids: &[u16] = if paged {
        &[CONTINUE_BUTTON, PREVIOUS_PAGE, NEXT_PAGE]
    } else {
        &[CONTINUE_BUTTON]
    };
    let buttons = ids
        .iter()
        .map(|&id| PageButton {
            id,
            paint: PaintButton {
                rect: layout
                    .control_rect(id, paged)
                    .expect("a laid-out score button"),
                pressed: pressed == Some(id),
                hovered: false,
                enabled: true,
            },
        })
        .collect();
    if panel.column.is_none() && paged {
        for (id, caption) in [(PREVIOUS_PAGE, "VERA:Previous"), (NEXT_PAGE, "VERA:Next")] {
            labels.push(PaintLabel {
                text: crate::i18n::tr(caption).into(),
                rect: crate::app::frontend::shell_pass::owner_draw_button_label_rect(
                    layout.paging_button(id).expect("paging button"),
                    pressed == Some(id),
                ),
                align: ShellAlign::BUTTON_CAPTION,
                rgb: SHELL_TEXT_RGB_ENABLED,
                path_a_reveal: None,
            });
        }
        let (index, count) = (page.page_index() + 1, page.page_count());
        labels.push(PaintLabel {
            text: crate::i18n::tr_fill("VERA:PageOf", &[("page", &index), ("pages", &count)])
                .into(),
            rect: layout.page_label(),
            align: ShellAlign::H_CENTER,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: None,
        });
    }

    // The shell pass draws in array order with depth off, so paint mode 1's
    // panel goes under the background. A country card (`[EndGameScreens]
    // Score*Art=`) is opaque over the whole dialog and would hide the panel,
    // Continue and the lower strip, so it goes under the chrome instead.
    let art_draw = TexturedDraw {
        texture: &art.texture,
        instances: art_sprites,
    };
    let mut frame = FamilyFrame::new("Score Shell", &SCORE_PAGE, &layout.page, panel, buttons);
    frame.labels = labels;
    if page.model.background.is_some() {
        frame.under.push(art_draw);
    } else {
        frame.over_panel_art.push(art_draw);
    }
    encode_family_page(state, encoder, destination, frame);
    Ok(true)
}
