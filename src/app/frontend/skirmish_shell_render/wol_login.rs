//! `0x10F`'s left-side children on the shared control owners: the two
//! labels, the nickname combo (face `0x00617250` with its NewEdit), the
//! `ES_PASSWORD` NewEdit, the Remember checkbox and the combo's drop list;
//! VERA online's page has its welcome, its sign-in form and the lobby's
//! counts in their place, and its popups over them. The WOL page compositor
//! draws them over its own backdrop and chrome.

use super::controls::{
    ControlPaint, NewEditPaint, dropdown_scroll_part, paint_control, push_new_edit_caret,
    push_new_edit_instances, push_popup_instances,
};
use super::text::{button_label_color_for_disabled, combo_face_text_color, rect_to_text_rect};
use super::wol_lobby::{POPUP_DEPTHS, WolLobbyPaint, label as csf_label, push_edit};
use super::{
    SHELL_CONTROL_TEXT_DEPTH, SHELL_DROPDOWN_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH,
    SHELL_EDIT_CARET_DEPTH, SHELL_EDIT_FRAME_DEPTH, SHELL_LABEL_TEXT_RGB,
};
use crate::app::AppState;
use crate::render::batch::SpriteInstance;
use crate::render::shell_paint::{self, ModalButton, PaintLabel};
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw};
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::modal::WolAccountFormLayout;
use crate::ui::wol_account::{FORM_CANCEL, FORM_OK, WolAccountForm};
use crate::ui::wol_login::{PLAY_AS_GUEST, SIGN_IN_AND_PLAY, WolLoginLayout, WolLoginState};
use crate::util::time::Instant;

/// VERA online's status line and counts, beside the shell's yellow captions.
const ONLINE_TEXT_RGB: [f32; 3] = [0.80, 0.80, 0.80];

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
    let label = |key: &str| csf_label(state, key);
    if let Some(choice) = layout.entry_choice() {
        let dialog = choice.dialog;
        // Use Create Room's original Red Alert 2 picture of the Tesla Trooper.
        // The regular mode-2 picture covers installs without that RA2 art.
        let background = atlas
            .ra2_dialog_background
            .or_else(|| atlas.mode_two_dialog_background(None));
        if let Some(background) = background {
            super::chrome::push_entry_fitted(
                &mut paint.popup,
                background,
                dialog,
                scale,
                POPUP_DEPTHS.background,
            );
        } else {
            super::chrome::push_solid_rect(
                &mut paint.popup,
                atlas,
                dialog,
                super::SHELL_MODAL_PANEL_RGB,
                POPUP_DEPTHS.background,
            );
        }
        let centre = ShellAlign::H_CENTER | ShellAlign::V_CENTER;
        let count = state
            .frontend
            .login_info
            .players_online()
            .map_or_else(|| "—".to_owned(), |count| count.to_string());
        for (value, rect, size) in [
            (label("VERA:OnlineWelcome"), choice.heading, 1.4),
            (
                crate::i18n::fill(&label("VERA:PlayersOnline"), &[("count", &count)]),
                choice.players_online,
                1.25,
            ),
        ] {
            let glyph_scale = scale.factor() * size;
            let fit = font.fit_line(&value, (rect.w as f32 / glyph_scale).max(1.0) as u32);
            let shadow = scale.derive_rect(rect, |r| r.translate(1, 1));
            font.fitted(fit, |font| {
                for (bounds, rgb, depth) in [
                    (shadow, [0.0; 3], POPUP_DEPTHS.button),
                    (rect, SHELL_LABEL_TEXT_RGB, POPUP_DEPTHS.text),
                ] {
                    paint.popup_texts.push(shell_text::draw_in_rect_scaled(
                        font,
                        &value,
                        rect_to_text_rect(bounds),
                        rgb,
                        centre,
                        [0.0; 2],
                        depth,
                        glyph_scale,
                    ));
                }
            });
        }
        for (id, key, rect) in [
            (PLAY_AS_GUEST, "VERA:PlayAsGuest", choice.guest),
            (SIGN_IN_AND_PLAY, "VERA:SignInAndPlay", choice.account),
        ] {
            let pressed = login.buttons.is_pressed(id);
            let mut frames = super::type3_button_frames(atlas);
            if login.buttons.hovered.unwrap_or(login.focused_control) == id {
                frames.up = atlas.modal_button_mnbttn_frame2.or(frames.up);
            }
            if let Some(face) = frames.select(pressed, true) {
                super::chrome::push_entry(&mut paint.popup, face, rect, POPUP_DEPTHS.button);
            }
            let caption = scale.derive_rect(rect, |r| {
                crate::app::frontend::shell_pass::owner_draw_button_label_rect(r, pressed)
            });
            let value = label(key);
            let face_width = (scale.base_rect(caption).w - 12).max(1) as u32;
            let fit = font.fit_line(&value, face_width);
            paint.popup_texts.push(font.fitted(fit, |_| {
                text(
                    &value,
                    caption,
                    centre,
                    SHELL_LABEL_TEXT_RGB,
                    POPUP_DEPTHS.text,
                )
            }));
        }
        return paint;
    }
    if let (Some(online), Some(form)) = (layout.online, login.form.as_ref()) {
        // The chosen route, its fields and the lobby activity over the login art.
        let centred = ShellAlign::H_CENTER;
        paint.texts.push(text(
            &label("VERA:SignInAndPlay"),
            online.heading,
            centred,
            SHELL_LABEL_TEXT_RGB,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
        // A popup over the page holds the caret.
        let caret_visible = login.account.is_none() && form.caret_visible(now);
        for (field, (caption_rect, edit)) in form.fields.iter().zip(online.fields) {
            paint.texts.push(text(
                &label(field.label),
                caption_rect,
                ShellAlign::NONE,
                SHELL_LABEL_TEXT_RGB,
                SHELL_CONTROL_TEXT_DEPTH,
            ));
            let edit_text = scale.derive_rect(edit, |rect| {
                RectPx::new(rect.x + 2, rect.y, rect.w - 1, rect.h + 1)
            });
            push_new_edit_instances(
                &mut paint.controls,
                atlas,
                font,
                &field.edit,
                NewEditPaint {
                    frame: scale.derive_rect(edit, |rect| {
                        RectPx::new(rect.x - 2, rect.y - 2, rect.w + 5, rect.h + 5)
                    }),
                    text: edit_text,
                    background: None,
                    frame_depth: SHELL_EDIT_FRAME_DEPTH,
                    caret_depth: SHELL_EDIT_CARET_DEPTH,
                    caret_visible: caret_visible && field.edit.focused,
                },
                scale,
            );
            paint.texts.push(text(
                &field.edit.painted_text(),
                edit_text,
                ShellAlign::V_CENTER,
                SHELL_LABEL_TEXT_RGB,
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
        if let Some(checked) = form.remember {
            paint_control(
                &mut paint.controls,
                &atlas.control_chrome(),
                ControlPaint::Checkbox {
                    rect: online.remember,
                    checked,
                },
                scale,
            );
            paint.texts.push(text(
                &label("VERA:RememberPassword"),
                crate::ui::skirmish_shell::checkbox_text_rect(online.remember, scale),
                ShellAlign::V_CENTER,
                SHELL_LABEL_TEXT_RGB,
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
        paint.texts.push(text(
            &form.status,
            online.status,
            centred,
            ONLINE_TEXT_RGB,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
        for ((key, count), rect) in state.frontend.login_info.counts().zip(online.counts) {
            let value = crate::i18n::fill(&label(key), &[("count", &count)]);
            paint.texts.push(text(
                &value,
                rect,
                centred,
                ONLINE_TEXT_RGB,
                SHELL_CONTROL_TEXT_DEPTH,
            ));
        }
        return paint;
    }
    for (key, rect) in &layout.labels {
        paint.texts.push(text(
            &label(key),
            *rect,
            ShellAlign::H_RIGHT,
            SHELL_LABEL_TEXT_RGB,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }

    // The nickname combo: its face and arrow, then its NewEdit's text and
    // caret inside the face's text area. ComboDrop's edit-child geometry is
    // untraced; the face's text rect stands in for it.
    paint_control(
        &mut paint.controls,
        &atlas.control_chrome(),
        ControlPaint::Combo {
            rect: layout.nickname_face,
            swatch: None,
            open: login.popup.control.is_some(),
            disabled: false,
        },
        scale,
    );
    let combo_text = layout.nickname_text_rect();
    let caret_visible = login.caret_visible(now);
    if caret_visible {
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

    {
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
    }

    if let Some(popup) = crate::app::shell_wol::combo_popup(login, layout) {
        push_popup_instances(
            &mut paint.popup,
            atlas,
            popup.rect,
            login.popup.hot_row.and_then(|row| popup.row_rect(row)),
            [],
            popup.scrollbar().zip(popup.thumb()),
            login.popup.scroll.pressed_part().map(dropdown_scroll_part),
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

/// VERA online's popups on Red Alert 2's box art, as the online Create Game
/// box: a caption and edit per field, OK and Cancel, and the status line.
pub(crate) fn paint_wol_account_form(
    state: &AppState,
    form: &WolAccountForm,
    layout: &WolAccountFormLayout,
    scale: ShellScale,
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let plain = |text: String, rect: RectPx, align: ShellAlign, rgb: [f32; 3]| PaintLabel {
        text: text.into(),
        rect,
        align,
        rgb,
        path_a_reveal: None,
    };
    let button_align = ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0);
    let button_label = |id: u16, rect: RectPx| {
        crate::app::frontend::shell_pass::owner_draw_button_label_rect(
            rect,
            form.buttons.is_pressed(id),
        )
    };
    let mut labels: Vec<PaintLabel> = form
        .fields
        .iter()
        .zip(&layout.fields)
        .map(|(field, (caption, _))| {
            plain(
                csf_label(state, field.label),
                *caption,
                ShellAlign::H_RIGHT,
                SHELL_LABEL_TEXT_RGB,
            )
        })
        .collect();
    labels.push(plain(
        csf_label(state, "GUI:OK"),
        button_label(FORM_OK, layout.ok),
        button_align,
        wol_button_label_rgb(form.ok_enabled()),
    ));
    labels.push(plain(
        csf_label(state, "GUI:Cancel"),
        button_label(FORM_CANCEL, layout.cancel),
        button_align,
        SHELL_LABEL_TEXT_RGB,
    ));
    labels.push(plain(
        form.status.clone(),
        layout.status,
        ShellAlign::NONE,
        SHELL_LABEL_TEXT_RGB,
    ));
    let buttons = [
        ModalButton {
            rect: layout.ok,
            pressed: form.buttons.is_pressed(FORM_OK),
            enabled: form.ok_enabled(),
        },
        ModalButton {
            rect: layout.cancel,
            pressed: form.buttons.is_pressed(FORM_CANCEL),
            enabled: true,
        },
    ];
    let background = atlas
        .ra2_dialog_background
        .or_else(|| atlas.mode_two_dialog_background(None));
    let draw = shell_paint::paint_modal_shp(
        &state.renderer.bit_font,
        background,
        super::type3_button_frames(atlas),
        layout.dialog,
        &buttons,
        &labels,
        scale,
        POPUP_DEPTHS,
    );
    paint.controls = draw.sprites;
    paint.texts = draw.text;
    let caret_visible = form.caret_visible(Instant::now());
    for (field, (_, rect)) in form.fields.iter().zip(&layout.fields) {
        push_edit(
            &mut paint,
            state,
            &field.edit,
            *rect,
            caret_visible && field.edit.focused,
            scale,
            POPUP_DEPTHS.text,
        );
    }
    paint
}
