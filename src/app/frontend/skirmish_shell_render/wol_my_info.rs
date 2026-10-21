//! `0x114`'s left-side children on the shared control owners: the captions,
//! the five owner-draw combos (the colour one with swatches), the two
//! checkboxes, the heading, statistics and update line, and the open drop
//! list. The WOL page compositor draws them, and the group box, over its
//! own backdrop and chrome.

use super::controls::{
    ControlPaint, dropdown_scroll_part, house_color_tint, paint_control, push_popup_instances,
};
use super::text::{combo_face_text_color, rect_to_text_rect};
use super::wol_login::{WolLoginPaint, wol_button_label_rgb};
use super::{
    SHELL_CONTROL_TEXT_DEPTH, SHELL_DROPDOWN_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB,
};
use crate::app::AppState;
use crate::render::shell_text::{self, ShellAlign};
use crate::ui::shell::geom::RectPx;
use crate::ui::wol_my_info::*;

pub(crate) fn paint_wol_my_info_children(
    state: &AppState,
    page: &WolMyInfoPage,
    layout: &WolMyInfoLayout,
) -> WolLoginPaint {
    let mut paint = WolLoginPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let font = &state.renderer.bit_font;
    let scale = layout.page.scale;
    let text = |value: &str, rect: RectPx, align: ShellAlign, rgb: [f32; 3], depth: f32| {
        shell_text::draw_in_rect_scaled(
            font,
            value,
            rect_to_text_rect(rect),
            rgb,
            align,
            [0.0; 2],
            depth,
            scale.factor(),
        )
    };
    let label = |key: &str| {
        state
            .process_assets
            .csf
            .as_ref()
            .map_or_else(|| key.to_owned(), |csf| csf.text(key).into_owned())
    };
    for ((key, _), rect) in LABELS.iter().zip(layout.labels) {
        paint.texts.push(text(
            &label(key),
            rect,
            ShellAlign::H_RIGHT,
            SHELL_LABEL_TEXT_RGB,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }
    for (id, face) in COMBOS.iter().copied().zip(layout.faces) {
        let Some(combo) = page.combo(id) else {
            continue;
        };
        let disabled = !combo.enabled;
        let shown = combo.selected.and_then(|row| combo.rows.get(row));
        paint_control(
            &mut paint.controls,
            &atlas.control_chrome(),
            ControlPaint::Combo {
                rect: face,
                swatch: shown
                    .and_then(|row| row.swatch)
                    .map(|index| house_color_tint(&[], index)),
                open: page.popup.control == Some(id),
                disabled,
            },
            scale,
        );
        if let Some(row) = shown.filter(|row| row.swatch.is_none()) {
            paint.texts.push(text(
                &row.text,
                crate::ui::skirmish_shell::combo_text_rect(face, scale),
                ShellAlign::V_CENTER,
                combo_face_text_color(disabled),
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
    }
    for (control, rect, checked, key) in [
        (
            AUTO_LOGIN,
            layout.auto_login,
            page.auto_login,
            AUTO_LOGIN_KEY,
        ),
        (
            MATCH_SAME_RES,
            layout.match_same_res,
            page.match_same_res,
            MATCH_SAME_RES_KEY,
        ),
    ] {
        paint_control(
            &mut paint.controls,
            &atlas.control_chrome(),
            ControlPaint::Checkbox { rect, checked },
            scale,
        );
        paint.texts.push(text(
            &label(key),
            crate::ui::skirmish_shell::checkbox_text_rect(rect, scale),
            ShellAlign::V_CENTER,
            wol_button_label_rgb(page.enabled(control)),
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }
    let value = |id: u16| {
        page.texts
            .iter()
            .find(|(control, _)| *control == id)
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| label(UNKNOWN_STATS_KEY))
    };
    for (id, rect, align) in [
        (HEADING, layout.heading, ShellAlign::H_CENTER),
        (UPDATE, layout.update, ShellAlign::H_CENTER),
    ]
    .into_iter()
    .chain(
        STATS
            .iter()
            .copied()
            .zip(layout.stats)
            .map(|(id, rect)| (id, rect, ShellAlign::NONE)),
    ) {
        let value = value(id);
        if value.is_empty() {
            continue;
        }
        paint.texts.push(text(
            &value,
            rect,
            align,
            SHELL_LABEL_TEXT_RGB,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }

    if let Some(popup) = page.popup_geometry(layout)
        && let Some(combo) = page.popup.control.and_then(|id| page.combo(id))
    {
        let swatches = (popup.first..popup.first + popup.visible_rows).filter_map(|index| {
            Some((
                scale.derive_rect(popup.row_rect(index)?, |row| {
                    RectPx::new(row.x + 2, row.y + 2, row.w - 4, 19)
                }),
                house_color_tint(&[], combo.rows.get(index)?.swatch?),
            ))
        });
        push_popup_instances(
            &mut paint.popup,
            atlas,
            popup.rect,
            page.popup.hot_row.and_then(|row| popup.row_rect(row)),
            swatches,
            popup.scrollbar().zip(popup.thumb()),
            page.popup.scroll.pressed_part().map(dropdown_scroll_part),
            scale,
            SHELL_DROPDOWN_DEPTH - 0.00002,
        );
        for index in popup.first..popup.first + popup.visible_rows {
            let (Some(row), Some(entry)) = (popup.row_rect(index), combo.rows.get(index)) else {
                continue;
            };
            if entry.swatch.is_some() {
                continue;
            }
            paint.popup_texts.push(text(
                &entry.text,
                scale.derive_rect(row, |rect| {
                    RectPx::new(rect.x + 3, rect.y, rect.w - 6, rect.h)
                }),
                ShellAlign::V_CENTER,
                SHELL_LABEL_TEXT_RGB,
                SHELL_DROPDOWN_TEXT_DEPTH,
            ));
        }
    }
    paint
}
