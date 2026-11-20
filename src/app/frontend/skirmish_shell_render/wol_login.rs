//! `0x10F`'s left-side children: its widgets through the generic painter,
//! the two labels, and the nickname combo's NewEdit inside the combo's face
//! with the drop list's rows; VERA online's page has its welcome, its
//! sign-in form and the lobby's counts in their place, and its popups over
//! them. The WOL page compositor draws them over its own backdrop and
//! chrome.

use vera20k_gui::NodeKind;

use super::controls::push_new_edit_caret;
use super::gui_paint::{self, GuiPaint};
use super::text::{button_label_color_for_disabled, combo_face_text_color, rect_to_text_rect};
use super::wol_lobby::{POPUP_DEPTHS, WolLobbyPaint, label as csf_label, push_edit};
use super::{
    SHELL_CONTROL_TEXT_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH, SHELL_EDIT_CARET_DEPTH,
    SHELL_LABEL_TEXT_RGB,
};
use crate::app::AppState;
use crate::render::shell_paint::{self, ModalButton, PaintLabel};
use crate::render::shell_text::{self, ShellAlign};
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::modal::WolAccountFormLayout;
use crate::ui::wol_account::{FORM_CANCEL, FORM_OK, POPUP, WolAccountForm};
use crate::ui::wol_login::{DIALOG, NICKNAME, NICKNAME_DROP, WolLoginLayout, WolLoginState};

/// VERA online's status line and counts, beside the shell's yellow captions.
const ONLINE_TEXT_RGB: [f32; 3] = [0.80, 0.80, 0.80];

/// The disabled color of a disabled owner-draw label (`0x00612F5F`).
pub(crate) fn wol_button_label_rgb(enabled: bool) -> [f32; 3] {
    button_label_color_for_disabled(!enabled)
}

pub(crate) fn paint_wol_login_children(
    state: &AppState,
    login: &WolLoginState,
    layout: &WolLoginLayout,
) -> GuiPaint {
    let ui = &state.frontend.shell_controller;
    let (gui, Some(root), Some(atlas)) = (
        ui.gui(),
        ui.root_in(DIALOG),
        state.frontend.skirmish_shell_chrome.as_ref(),
    ) else {
        return GuiPaint::default();
    };
    let font = &state.renderer.bit_font;
    let scale = layout.page.scale;
    let text = |value: &str, rect: RectPx, align: ShellAlign, rgb: [f32; 3], depth: f32| {
        let rect = rect_to_text_rect(rect);
        shell_text::draw_in_rect_scaled(
            font,
            value,
            rect,
            rgb,
            align,
            [0.0; 2],
            depth,
            scale.factor(),
        )
    };
    let label = |key: &str| csf_label(state, key);
    if let (Some(online), Some(form)) = (layout.online, login.form.as_ref()) {
        // VERA online: the welcome, the sign-in form and the counts, straight
        // over the art.
        let mut paint = gui_paint::paint_gui_children(state, gui, root, scale);
        let centred = ShellAlign::H_CENTER;
        let heading = label("VERA:OnlineWelcome");
        paint.texts.push(text(
            &heading,
            online.heading,
            centred,
            SHELL_LABEL_TEXT_RGB,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
        for (field, (caption, _)) in form.fields.iter().zip(online.fields) {
            let value = label(field.label);
            paint.texts.push(text(
                &value,
                caption,
                ShellAlign::NONE,
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
    // The panel holds every control but the combo's NewEdit.
    let children = gui.tree().children(root).unwrap_or_default();
    let panel = children
        .iter()
        .find(|node| gui.tree().kind(**node) == Ok(NodeKind::Panel));
    let mut paint = panel.map_or_else(GuiPaint::default, |panel| {
        gui_paint::paint_gui_children(state, gui, *panel, scale)
    });
    for (key, rect) in &layout.labels {
        paint.texts.push(text(
            &label(key),
            *rect,
            ShellAlign::H_RIGHT,
            SHELL_LABEL_TEXT_RGB,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }
    // The combo's NewEdit has no frame of its own: its caret and text paint
    // in the face's text area. ComboDrop's edit-child geometry is untraced;
    // the face's text rect stands in for it.
    if let Some(node) = ui.control(DIALOG, NICKNAME) {
        let edit = gui_paint::new_edit(gui, node);
        let combo_text = layout.nickname_text_rect();
        if ui.caret_visible(node) {
            push_new_edit_caret(
                &mut paint.controls,
                atlas,
                font,
                &edit,
                combo_text,
                SHELL_EDIT_CARET_DEPTH,
                scale,
            );
        }
        let rgb = combo_face_text_color(false);
        paint.texts.push(text(
            &edit.painted_text(),
            combo_text,
            ShellAlign::V_CENTER,
            rgb,
            SHELL_CONTROL_TEXT_DEPTH,
        ));
    }
    // The open list's rows are the page's (the combo has a row count).
    if let Some(drop) = ui
        .control(DIALOG, NICKNAME_DROP)
        .and_then(|node| gui.combo_drop_down(node))
    {
        let popup = drop.popup;
        for index in popup.first..popup.first + popup.visible_rows {
            let (Some(row), Some(entry)) = (popup.row_rect(index), login.rows.get(index)) else {
                continue;
            };
            let at = scale.derive_rect(row, |rect| {
                RectPx::new(rect.x + 3, rect.y, rect.w - 6, rect.h)
            });
            let draw = text(
                &entry.nickname,
                at,
                ShellAlign::V_CENTER,
                SHELL_LABEL_TEXT_RGB,
                SHELL_DROPDOWN_TEXT_DEPTH,
            );
            paint.popup_texts.push(draw);
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
    let ui = &state.frontend.shell_controller;
    // 612B70: pushed while captured and under the pointer.
    let pressed = |id: u16| {
        let node = ui.control(POPUP, id);
        node.is_some() && ui.pressed_node_in(POPUP) == node && ui.hovered_node_in(POPUP) == node
    };
    let ok_enabled = form.ok_enabled(ui);
    let plain = |text: String, rect: RectPx, align: ShellAlign, rgb: [f32; 3]| PaintLabel {
        text: text.into(),
        rect,
        align,
        rgb,
        path_a_reveal: None,
    };
    let button = |id: u16, key: &str, rect: RectPx, rgb: [f32; 3]| {
        let rect =
            crate::app::frontend::shell_pass::owner_draw_button_label_rect(rect, pressed(id));
        plain(
            csf_label(state, key),
            rect,
            ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0),
            rgb,
        )
    };
    let captions = form.fields.iter().zip(&layout.fields);
    let mut labels: Vec<PaintLabel> = captions
        .map(|(field, (caption, _))| {
            plain(
                csf_label(state, field.label),
                *caption,
                ShellAlign::H_RIGHT,
                SHELL_LABEL_TEXT_RGB,
            )
        })
        .collect();
    labels.push(button(
        FORM_OK,
        "GUI:OK",
        layout.ok,
        wol_button_label_rgb(ok_enabled),
    ));
    labels.push(button(
        FORM_CANCEL,
        "GUI:Cancel",
        layout.cancel,
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
            pressed: pressed(FORM_OK),
            enabled: ok_enabled,
        },
        ModalButton {
            rect: layout.cancel,
            pressed: pressed(FORM_CANCEL),
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
    for (index, (_, rect)) in layout.fields.iter().enumerate() {
        if let Some(node) = ui.control(POPUP, WolAccountForm::field_id(index)) {
            let edit = gui_paint::new_edit(ui.gui(), node);
            push_edit(
                &mut paint,
                state,
                &edit,
                *rect,
                ui.caret_visible(node),
                scale,
                POPUP_DEPTHS.text,
            );
        }
    }
    paint
}
