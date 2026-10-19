//! D7 Network Options: native family composition and shared owner-draw controls.
//! Dialog owner 00560480; the UI owner supplies the original resource geometry.

use super::chrome::*;
use super::controls::{
    ControlPaint, NewEditPaint, paint_control, push_group_or_plain_frame, push_new_edit_instances,
};
use super::in_game_shell::{
    self, InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay,
};
use super::text::{button_text_rect, localized_label, rect_to_text_rect};
use super::{
    SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH, SHELL_DROPDOWN_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH,
    SHELL_EDIT_CARET_DEPTH, SHELL_EDIT_FRAME_DEPTH, SHELL_LABEL_TEXT_RGB,
};
use crate::app::AppState;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw};
use crate::render::shell_text_reveal::PathAReveal;
use crate::ui::network_options::{OK, PAGE};
use crate::ui::shell::geom::{RectPx, ShellScale};
use std::time::Instant;

pub(crate) fn render_network_options(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    crate::app::input::network_options::update_edit_scroll(state);
    let now = Instant::now();
    let layout = crate::app::input::network_options::layout(state);
    let scale = layout.page.scale;
    let width = state.render_width();
    let height = state.render_height();
    let exit = crate::app::frontend::shell_transition::shell_exit_wave(
        state,
        ShellSlideKind::NetworkOptions,
    )
    .cloned();
    let leaving = exit.is_some();
    let wave = exit.or_else(|| state.frontend.shell_first_paint_slide.clone());
    let sliding = wave.is_some();
    let title = (!sliding)
        .then(|| state.frontend.shell_page_title.paint(now))
        .flatten();
    let monitor_frames = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .map_or(0, |atlas| atlas.launcher_warning_frames.len());
    let monitor = if leaving {
        None
    } else {
        state
            .frontend
            .shell_monitor
            .paint(now, monitor_frames, !sliding)
    };
    let status_label = if sliding {
        None
    } else {
        let help = state
            .frontend
            .network_options
            .as_ref()
            .and_then(|dialog| dialog.buttons.hovered)
            .and_then(crate::ui::network_options::tooltip_key)
            .map(|key| localized_label(state, key, ""))
            .unwrap_or_default();
        crate::app::frontend::menu_page_render::paint_shell_status_line(
            state,
            help,
            layout.page.status_help,
        )
    };
    let dialog = state.frontend.network_options.as_ref().expect("active D7");
    let atlas = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("D7 control art");
    let font = &state.renderer.bit_font;
    let mut art =
        super::launcher_options::launcher_background_instances(atlas, width, height, scale);
    if let Some(frame) = monitor.and_then(|frame| atlas.launcher_warning_frames.get(frame)) {
        push_flag_entry_clipped_centered(
            &mut art,
            *frame,
            layout.page.warning_monitor,
            scale,
            SHELL_CONTROL_DEPTH,
        );
    }
    if let Some(wave) = &wave {
        push_slide_column(
            &mut art,
            atlas,
            &crate::ui::skirmish_shell::compute_layout_on(scale, width, height),
            &wave.button_draws(),
            SHELL_CONTROL_DEPTH,
        );
    }
    let mut controls = Vec::new();
    let mut texts = Vec::new();
    let mut overlay = ShellFrameOverlay::default();
    let text = |value: &str, rect: RectPx, align: ShellAlign, depth: f32| -> ShellTextDraw {
        shell_text::draw_in_rect_scaled(
            font,
            value,
            rect_to_text_rect(rect),
            SHELL_LABEL_TEXT_RGB,
            align,
            [0.0; 2],
            depth,
            scale.factor(),
        )
    };
    // 622C4F teardown paints the backing while children stay hidden. Entry
    // paints ordinary controls but holds the family title and column labels.
    if !leaving {
        for (key, rect) in layout.labels {
            texts.push(text(
                &localized_label(state, key, key),
                rect,
                ShellAlign::NONE,
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
        for (key, rect) in layout.groups {
            let caption = localized_label(state, key, key);
            let top = (font.cell_height() * scale.factor()).round() as i32 / 2;
            push_group_or_plain_frame(
                &mut controls,
                atlas,
                RectPx::new(rect.x, rect.y + top, rect.w, rect.h - top),
                Some((font.text_width(&caption) as f32 * scale.factor()).round() as i32),
                false,
            );
            texts.push(text(
                &caption,
                scale.derive_rect(rect, |rect| {
                    RectPx::new(rect.x + 10, rect.y, rect.w - 10, font.cell_height() as i32)
                }),
                ShellAlign::NONE,
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
        for (rect, edit) in [
            (layout.socket, &dialog.socket),
            (layout.destination, &dialog.destination),
            (layout.port, &dialog.port),
        ] {
            // 614FAB: the text starts two columns into the runtime window;
            // that window already includes the NewEdit initialization inset.
            let field = scale.derive_rect(rect, |rect| {
                RectPx::new(rect.x + 2, rect.y, rect.w - 1, rect.h + 1)
            });
            push_new_edit_instances(
                &mut controls,
                atlas,
                font,
                edit,
                NewEditPaint {
                    frame: scale.derive_rect(rect, |rect| {
                        RectPx::new(rect.x - 2, rect.y - 2, rect.w + 5, rect.h + 5)
                    }),
                    text: field,
                    background: None,
                    frame_depth: SHELL_EDIT_FRAME_DEPTH,
                    caret_depth: SHELL_EDIT_CARET_DEPTH,
                    caret_visible: dialog.caret_visible(now),
                },
                scale,
            );
            texts.push(text(
                &String::from_utf16_lossy(
                    &edit.units[edit.first_visible_unit.min(edit.units.len())..],
                ),
                field,
                ShellAlign::V_CENTER,
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
        paint_control(
            &mut controls,
            &atlas.control_chrome(),
            ControlPaint::Combo {
                rect: layout.card,
                swatch: None,
                open: dialog.combo_open,
                disabled: false,
            },
            scale,
        );
        let no_card = localized_label(state, "TXT_NO_BOUND_NET_CARD", "No network card bound");
        let card_label = |index: usize| {
            dialog
                .cards
                .get(index)
                .map(String::as_str)
                .unwrap_or(&no_card)
        };
        texts.push(text(
            card_label(dialog.selected_card),
            crate::ui::skirmish_shell::combo_text_rect(layout.card, scale),
            ShellAlign::V_CENTER,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
        paint_control(
            &mut controls,
            &atlas.control_chrome(),
            ControlPaint::Checkbox {
                rect: layout.send_delay,
                checked: dialog.send_delay,
            },
            scale,
        );
        texts.push(text(
            &localized_label(state, "GUI:FirewallSendDelay", "Send Delay"),
            crate::ui::skirmish_shell::checkbox_text_rect(layout.send_delay, scale),
            ShellAlign::V_CENTER,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
        if let Some(reveal) = title {
            texts.push(shell_text::draw_in_rect_path_a_scaled(
                font,
                &localized_label(state, PAGE.title_key, "Network Options"),
                rect_to_text_rect(layout.page.title),
                ShellAlign::H_CENTER,
                [0.0; 2],
                SHELL_CONTROL_TEXT_DEPTH,
                PathAReveal {
                    count: reveal.count,
                    range: reveal.range,
                    base_rgb: [255, 255, 0],
                    highlight_rgb: [255; 3],
                },
                scale.factor(),
            ));
        }
        if !sliding {
            for button in &layout.page.buttons {
                let pressed = dialog.buttons.is_pressed(button.id);
                push_right_panel_button_shp(
                    &mut art,
                    atlas,
                    button.rect,
                    pressed,
                    false,
                    SHELL_CONTROL_DEPTH,
                );
                if let Some(spec) = PAGE.button(button.id) {
                    texts.push(shell_text::draw_in_rect_scaled(
                        font,
                        &localized_label(state, spec.csf_key, spec.csf_key),
                        button_text_rect(button.rect, pressed, scale),
                        SHELL_LABEL_TEXT_RGB,
                        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                        [0.0; 2],
                        SHELL_CONTROL_TEXT_DEPTH,
                        scale.factor(),
                    ));
                }
            }
        }
        if let Some(popup) = dialog.card_popup(&layout) {
            super::controls::push_popup_instances(
                &mut overlay.controls,
                atlas,
                popup.rect,
                dialog.card_hot_row.and_then(|index| popup.row_rect(index)),
                [],
                popup.scrollbar().zip(popup.thumb()),
                dialog
                    .card_scroll
                    .pressed_part()
                    .map(super::controls::dropdown_scroll_part),
                scale,
                SHELL_DROPDOWN_DEPTH - 0.00002,
            );
            for index in popup.first..popup.first + popup.visible_rows {
                let Some(row) = popup.row_rect(index) else {
                    continue;
                };
                overlay.texts.push(text(
                    card_label(index),
                    scale.derive_rect(row, |rect| {
                        RectPx::new(rect.x + 3, rect.y, rect.w - 6, rect.h)
                    }),
                    ShellAlign::V_CENTER,
                    SHELL_DROPDOWN_TEXT_DEPTH,
                ));
            }
        }
        texts.extend(crate::render::shell_paint::paint_labels_scaled(
            font,
            &status_label.into_iter().collect::<Vec<_>>(),
            scale.factor(),
        ));
        if let Some(message) = dialog.pending_message() {
            let modal = crate::ui::shell::modal::body_ok_layout(width as i32, height as i32);
            let pressed = dialog.buttons.is_pressed(OK);
            super::modals::push_validation_modal_instances(
                &mut overlay.controls,
                atlas,
                &modal,
                pressed,
            );
            // The retained modal uses unscaled resource geometry, including
            // at responsive shell sizes; input uses the same body_ok_layout.
            overlay.texts.push(shell_text::draw_in_rect(
                font,
                &localized_label(state, message.csf_key(), message.csf_key()),
                rect_to_text_rect(modal.body),
                SHELL_LABEL_TEXT_RGB,
                super::text::validation_modal_body_text_align(),
                [0.0; 2],
                SHELL_DROPDOWN_TEXT_DEPTH - 0.00012,
            ));
            overlay.texts.push(shell_text::draw_in_rect(
                font,
                &localized_label(state, "TXT_OK", "OK"),
                button_text_rect(modal.ok, pressed, ShellScale::RETAIL),
                SHELL_LABEL_TEXT_RGB,
                ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                [0.0; 2],
                SHELL_DROPDOWN_TEXT_DEPTH - 0.00013,
            ));
        }
    }
    in_game_shell::render_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art,
            controls,
            texts,
            label: "Network Options D7",
        },
        ShellFrameArt::Launcher,
        overlay,
        ShellFrameCursor::Visible,
    )?;
    state.platform.window.request_redraw();
    Ok(())
}
