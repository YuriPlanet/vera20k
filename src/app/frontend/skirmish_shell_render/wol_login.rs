//! `0x10F`'s left-side children on the shared control owners: the two
//! labels, the nickname combo (face `0x00617250` with its NewEdit), the
//! `ES_PASSWORD` NewEdit, the Remember checkbox and the combo's drop list;
//! VERA online's page has its welcome, its username NewEdit and the lobby's
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
use crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::list::list_frame_lines;
use crate::ui::shell::modal::WolAccountFormLayout;
use crate::ui::wol_account::{FORM_CANCEL, FORM_OK, WolAccountForm};
use crate::ui::wol_login::{WolLoginLayout, WolLoginState};
use crate::util::time::Instant;

/// VERA online's welcome line and counts, beside the shell's yellow captions.
const ONLINE_TEXT_RGB: [f32; 3] = [0.80, 0.80, 0.80];
/// An info panel sits over the page art, under the page's popups.
const INFO_FILL_DEPTH: f32 = shell_paint::CHROME_DEPTH - 0.00001;
const INFO_FRAME_DEPTH: f32 = shell_paint::CHROME_DEPTH - 0.00002;
/// The panel darkens the art behind it, which still shows through.
const INFO_FILL_ALPHA: f32 = 0.82;

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
    if let Some(online) = layout.online {
        // VERA online: the welcome, the username field and the counts, in
        // their panel.
        push_info_panel(&mut paint.controls, atlas, online.panel);
        let centred = ShellAlign::H_CENTER;
        for (value, rect, align, rgb) in [
            (
                label("VERA:OnlineWelcome"),
                online.heading,
                centred,
                SHELL_LABEL_TEXT_RGB,
            ),
            (
                label("VERA:OnlineWelcomeText"),
                online.text,
                centred,
                ONLINE_TEXT_RGB,
            ),
            (
                label("VERA:Username"),
                online.name_label,
                ShellAlign::NONE,
                SHELL_LABEL_TEXT_RGB,
            ),
        ] {
            paint
                .texts
                .push(text(&value, rect, align, rgb, SHELL_CONTROL_TEXT_DEPTH));
        }
        let name_text = scale.derive_rect(online.name, |rect| {
            RectPx::new(rect.x + 2, rect.y, rect.w - 1, rect.h + 1)
        });
        push_new_edit_instances(
            &mut paint.controls,
            atlas,
            font,
            &login.nickname,
            NewEditPaint {
                frame: scale.derive_rect(online.name, |rect| {
                    RectPx::new(rect.x - 2, rect.y - 2, rect.w + 5, rect.h + 5)
                }),
                text: name_text,
                background: None,
                frame_depth: SHELL_EDIT_FRAME_DEPTH,
                caret_depth: SHELL_EDIT_CARET_DEPTH,
                // A popup over the page holds the caret.
                caret_visible: login.account.is_none() && login.caret_visible(now),
            },
            scale,
        );
        paint.texts.push(text(
            &login.nickname.painted_text(),
            name_text,
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
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

/// A panel's darkened backing and a list's two frame rings around it.
pub(super) fn push_info_panel(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    panel: RectPx,
) {
    let Some(pixel) = atlas.white_pixel else {
        return;
    };
    let mut push = |rect: RectPx, tint: [f32; 3], alpha: f32, depth: f32| {
        out.push(SpriteInstance {
            position: [rect.x as f32, rect.y as f32],
            size: [rect.w as f32, rect.h as f32],
            uv_origin: pixel.uv_origin,
            uv_size: pixel.uv_size,
            depth,
            tint,
            alpha,
            ..Default::default()
        });
    };
    push(
        RectPx::new(panel.x + 1, panel.y + 1, panel.w - 2, panel.h - 2),
        super::SHELL_MODAL_PANEL_RGB,
        INFO_FILL_ALPHA,
        INFO_FILL_DEPTH,
    );
    for (rect, tone) in list_frame_lines(panel) {
        push(
            rect,
            tone.rgb().map(|channel| f32::from(channel) / 255.0),
            1.0,
            INFO_FRAME_DEPTH,
        );
    }
}

/// VERA online's popups on Red Alert 2's box art, as the online Create Game
/// box: a caption and edit per field, Login's Remember password box, OK and
/// Cancel, and the status line.
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
    let mut checkboxes = Vec::new();
    if let (Some(checked), Some(rect)) = (form.remember, layout.remember) {
        paint_control(
            &mut checkboxes,
            &atlas.control_chrome(),
            ControlPaint::Checkbox { rect, checked },
            scale,
        );
        labels.push(plain(
            csf_label(state, "VERA:RememberPassword"),
            crate::ui::skirmish_shell::checkbox_text_rect(rect, scale),
            ShellAlign::V_CENTER,
            SHELL_LABEL_TEXT_RGB,
        ));
    }
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
    // Paint order is draw order: the box first, then its checkbox.
    paint.controls = draw.sprites;
    paint.controls.extend(checkboxes);
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
