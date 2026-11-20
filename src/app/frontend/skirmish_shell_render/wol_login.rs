//! `0x10F`'s left-side children: its widgets through the generic painter,
//! the two labels, and the nickname combo's NewEdit inside the combo's face
//! with the drop list's rows; VERA online's page has its welcome, its
//! sign-in form and the lobby's counts in their place, and its popups over
//! them. The WOL page compositor draws them over its own backdrop and
//! chrome.

use vera20k_gui::NodeKind;

use super::controls::push_new_edit_caret;
use super::gui_paint::{self, GuiPaint, GuiStyle, HostRows};
use super::text::{button_label_color_for_disabled, combo_face_text_color, rect_to_text_rect};
use super::wol_lobby::{POPUP_DEPTHS, WolLobbyPaint};
use super::{SHELL_CONTROL_TEXT_DEPTH, SHELL_EDIT_CARET_DEPTH};
use crate::app::AppState;
use crate::render::shell_paint;
use crate::render::shell_text::{self, ShellAlign};
use crate::ui::shell::geom::ShellScale;
use crate::ui::shell::modal::WolAccountFormLayout;
use crate::ui::wol_account::{
    FIRST_CAPTION, FORM_CANCEL, FORM_OK, FORM_STATUS, POPUP, WolAccountForm,
};
use crate::ui::wol_login::{
    CAPTIONS_TEXT, COUNTS_TEXT, DIALOG, HEADING_TEXT, LABELS, NICKNAME, NICKNAME_DROP, STATUS_TEXT,
    WolLoginLayout, WolLoginState,
};

/// The disabled color of a disabled owner-draw label (`0x00612F5F`).
pub(crate) fn wol_button_label_rgb(enabled: bool) -> [f32; 3] {
    button_label_color_for_disabled(!enabled)
}

/// `0x10F`'s widgets and the statics whose text the page keeps; gamemd's
/// page adds the combo's NewEdit, VERA online's has its form in its place.
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
    let scale = layout.page.scale;
    let label = |key: &str| state.csf_text(key);
    let text_of = |id: i32| {
        let id = u16::try_from(id).ok()?;
        let at = |ids: [u16; 2]| ids.iter().position(|other| *other == id);
        if let Some(index) = at(LABELS) {
            return Some(label(layout.labels[index].0));
        }
        let form = login.form.as_ref()?;
        if let Some(index) = at(CAPTIONS_TEXT) {
            return Some(label(form.fields.get(index)?.label));
        }
        if let Some(index) = at(COUNTS_TEXT) {
            let (key, count) = state.frontend.login_info.counts().nth(index)?;
            return Some(crate::i18n::fill(&label(key), &[("count", &count)]));
        }
        match id {
            HEADING_TEXT => Some(label("VERA:OnlineWelcome")),
            STATUS_TEXT => Some(form.status.clone()),
            _ => None,
        }
    };
    let rows_of = |id| {
        (id == i32::from(NICKNAME_DROP)).then(|| {
            login
                .rows
                .iter()
                .map(|row| (row.nickname.clone(), u32::MAX))
                .collect::<HostRows>()
        })
    };
    let style = GuiStyle {
        text_of: Some(&text_of),
        rows_of: Some(&rows_of),
        ..Default::default()
    };
    if layout.online.is_some() && login.form.is_some() {
        // VERA online: the welcome, the sign-in form and the counts, straight
        // over the art.
        return gui_paint::paint_gui_styled(state, gui, root, scale, &[], style);
    }
    // The panel holds every control but the combo's NewEdit.
    let children = gui.tree().children(root).unwrap_or_default();
    let panel = children
        .iter()
        .find(|node| gui.tree().kind(**node) == Ok(NodeKind::Panel));
    let mut paint = panel.map_or_else(GuiPaint::default, |panel| {
        gui_paint::paint_gui_styled(state, gui, *panel, scale, &[], style)
    });
    // The combo's NewEdit has no frame of its own: its caret and text paint
    // in the face's text area. ComboDrop's edit-child geometry is untraced;
    // the face's text rect stands in for it.
    if let Some(node) = ui.control(DIALOG, NICKNAME) {
        let edit = gui_paint::new_edit(gui, node);
        let combo_text = layout.nickname_text_rect();
        let font = &state.renderer.bit_font;
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
        paint.texts.push(shell_text::draw_in_rect_scaled(
            font,
            &edit.painted_text(),
            rect_to_text_rect(combo_text),
            combo_face_text_color(false),
            ShellAlign::V_CENTER,
            [0.0; 2],
            SHELL_CONTROL_TEXT_DEPTH,
            scale.factor(),
        ));
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
    let ui = &state.frontend.shell_controller;
    let (Some(atlas), Some(root)) = (
        state.frontend.skirmish_shell_chrome.as_ref(),
        ui.root_in(POPUP),
    ) else {
        return paint;
    };
    let style = GuiStyle::popup(atlas);
    let background = atlas
        .ra2_dialog_background
        .or_else(|| atlas.mode_two_dialog_background(None));
    paint.controls = shell_paint::paint_modal_sprites(
        background,
        Default::default(),
        layout.dialog,
        &[],
        scale,
        POPUP_DEPTHS,
    );
    let text_of = |id: i32| {
        let id = u16::try_from(id).ok()?;
        match id {
            FORM_OK => Some(state.csf_text("GUI:OK")),
            FORM_CANCEL => Some(state.csf_text("GUI:Cancel")),
            FORM_STATUS => Some(form.status.clone()),
            _ => Some(
                state.csf_text(
                    form.fields
                        .get(usize::from(id.checked_sub(FIRST_CAPTION)?))?
                        .label,
                ),
            ),
        }
    };
    let ok_enabled = form.ok_enabled(ui);
    let enabled_of = |id| (id == i32::from(FORM_OK)).then_some(ok_enabled);
    let style = GuiStyle {
        text_of: Some(&text_of),
        enabled_of: Some(&enabled_of),
        ..style
    };
    let widgets = gui_paint::paint_gui_styled(state, ui.gui(), root, scale, &[], style);
    paint.controls.extend(widgets.controls);
    paint.texts = widgets.texts;
    paint
}
