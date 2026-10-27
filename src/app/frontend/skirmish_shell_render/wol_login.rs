//! `0x10F`'s left-side children on the shared control owners: the two
//! labels, the nickname combo (face `0x00617250` with its NewEdit), the
//! `ES_PASSWORD` NewEdit, the Remember checkbox and the combo's drop list.
//! The WOL page compositor draws them over its own backdrop and chrome.

use super::controls::{
    ControlPaint, NewEditPaint, dropdown_scroll_part, paint_control, push_new_edit_caret,
    push_new_edit_instances, push_popup_instances,
};
use super::text::{button_label_color_for_disabled, combo_face_text_color, rect_to_text_rect};
use super::{
    SHELL_CONTROL_TEXT_DEPTH, SHELL_DROPDOWN_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH,
    SHELL_EDIT_CARET_DEPTH, SHELL_EDIT_FRAME_DEPTH, SHELL_LABEL_TEXT_RGB,
};
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw};
use crate::ui::shell::geom::RectPx;
use crate::ui::wol_login::{LoginPolicy, WolLoginLayout, WolLoginState};
use crate::util::time::Instant;

/// Sprites (skirmish atlas) and texts of `0x10F`'s children; `popup` paints
/// after everything else.
#[derive(Default)]
pub(crate) struct WolLoginPaint {
    pub(crate) controls: Vec<SpriteInstance>,
    pub(crate) texts: Vec<ShellTextDraw>,
    pub(crate) popup: Vec<SpriteInstance>,
    pub(crate) popup_texts: Vec<ShellTextDraw>,
}

/// The disabled color of a disabled owner-draw label (`0x00612F5F`).
pub(crate) fn wol_button_label_rgb(enabled: bool) -> [f32; 3] {
    button_label_color_for_disabled(!enabled)
}

pub(crate) fn paint_wol_login_children(
    state: &AppState,
    login: &WolLoginState,
    layout: &WolLoginLayout,
) -> WolLoginPaint {
    let mut paint = WolLoginPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let font = &state.renderer.bit_font;
    let scale = layout.page.scale;
    let online = login.policy == LoginPolicy::Online;
    let now = Instant::now();
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
    let labels = if online {
        &layout.labels[..1]
    } else {
        &layout.labels[..]
    };
    for (key, rect) in labels {
        paint.texts.push(text(
            &if online {
                "Username".to_owned()
            } else {
                label(key)
            },
            *rect,
            ShellAlign::H_RIGHT,
            SHELL_LABEL_TEXT_RGB,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }

    // The nickname combo: its face and arrow, then its NewEdit's text and
    // caret inside the face's text area. ComboDrop's edit-child geometry is
    // untraced; the face's text rect stands in for it.
    if !online {
        paint_control(
            &mut paint.controls,
            &atlas.control_chrome(),
            ControlPaint::Combo {
                rect: layout.nickname_face,
                swatch: None,
                open: login.combo_open,
                disabled: false,
            },
            scale,
        );
    }
    let combo_text = layout.nickname_text_rect();
    let caret_visible = login.caret_visible(now);
    if online {
        push_new_edit_instances(
            &mut paint.controls,
            atlas,
            font,
            &login.nickname,
            NewEditPaint {
                frame: layout.nickname_face,
                text: combo_text,
                background: None,
                frame_depth: SHELL_EDIT_FRAME_DEPTH,
                caret_depth: SHELL_EDIT_CARET_DEPTH,
                caret_visible,
            },
            scale,
        );
    } else if caret_visible {
        // The combo's NewEdit has no frame of its own: only its caret paints
        // over the face.
        push_new_edit_caret(
            &mut paint.controls,
            atlas,
            font,
            &login.nickname,
            combo_text,
            SHELL_EDIT_CARET_DEPTH,
            scale,
        );
    }
    paint.texts.push(text(
        &login.nickname.painted_text(),
        combo_text,
        ShellAlign::V_CENTER,
        combo_face_text_color(false),
        SHELL_CONTROL_TEXT_DEPTH,
    ));

    if !online {
        // 614FAB: the text starts two columns into the inset window.
        let password_text = scale.derive_rect(layout.password, |rect| {
            RectPx::new(rect.x + 2, rect.y, rect.w - 1, rect.h + 1)
        });
        push_new_edit_instances(
            &mut paint.controls,
            atlas,
            font,
            &login.password,
            NewEditPaint {
                frame: scale.derive_rect(layout.password, |rect| {
                    RectPx::new(rect.x - 2, rect.y - 2, rect.w + 5, rect.h + 5)
                }),
                text: password_text,
                background: None,
                frame_depth: SHELL_EDIT_FRAME_DEPTH,
                caret_depth: SHELL_EDIT_CARET_DEPTH,
                caret_visible,
            },
            scale,
        );
        paint.texts.push(text(
            &login.password.painted_text(),
            password_text,
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
            SHELL_CONTROL_TEXT_DEPTH,
        ));

        paint_control(
            &mut paint.controls,
            &atlas.control_chrome(),
            ControlPaint::Checkbox {
                rect: layout.remember,
                checked: login.remember_checked,
            },
            scale,
        );
        paint.texts.push(text(
            &label("GUI:RememberAccount"),
            crate::ui::skirmish_shell::checkbox_text_rect(layout.remember, scale),
            ShellAlign::V_CENTER,
            wol_button_label_rgb(login.remember_enabled),
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    } else {
        // Under the field, starting where its text does.
        let rect = scale.derive_rect(layout.nickname_face, |rect| {
            RectPx::new(rect.x + 3, rect.y + rect.h + 4, rect.w - 6, 16)
        });
        paint.texts.push(text(
            "Use 1-24 letters, digits, _ or -.",
            rect,
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }

    if let Some(popup) = crate::app::shell_wol::combo_popup(login, layout) {
        push_popup_instances(
            &mut paint.popup,
            atlas,
            popup.rect,
            login.combo_hot_row.and_then(|row| popup.row_rect(row)),
            [],
            popup.scrollbar().zip(popup.thumb()),
            login.combo_scroll.pressed_part().map(dropdown_scroll_part),
            scale,
            SHELL_DROPDOWN_DEPTH - 0.00002,
        );
        for index in popup.first..popup.first + popup.visible_rows {
            let (Some(row), Some(entry)) = (popup.row_rect(index), login.rows.get(index)) else {
                continue;
            };
            paint.popup_texts.push(text(
                &entry.nickname,
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
