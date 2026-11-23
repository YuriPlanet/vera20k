//! `0x10F`'s left-side children: its widgets through the generic painter,
//! the two labels, and the nickname combo's NewEdit inside the combo's face
//! with the drop list's rows; VERA online's page has its entry chooser, or
//! its sign-in form and the lobby's counts in their place, and its popups
//! over them. The WOL page compositor draws them over its own backdrop and
//! chrome.

use vera20k_gui::NodeKind;

use super::controls::push_new_edit_caret;
use super::gui_paint::{self, GuiPaint, GuiStyle, HostRows};
use super::text::{combo_face_text_color, rect_to_text_rect};
use super::wol_lobby::{POPUP_DEPTHS, WolLobbyPaint};
use super::{SHELL_CONTROL_TEXT_DEPTH, SHELL_EDIT_CARET_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::render::shell_paint;
use crate::render::shell_text;
use crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::geom::{ShellAlign, ShellScale};
use crate::ui::shell::modal::{WolAccountFormLayout, WolEntryChoiceLayout};
use crate::ui::wol_account::{
    FIRST_CAPTION, FORM_CANCEL, FORM_OK, FORM_STATUS, POPUP, WolAccountForm,
};
use crate::ui::wol_login::{
    CAPTIONS_TEXT, COUNTS_TEXT, DIALOG, HEADING_TEXT, LABEL_KEYS, LABELS, LoginPolicy, NICKNAME,
    NICKNAME_DROP, PLAY_AS_GUEST, SIGN_IN_AND_PLAY, STATUS_TEXT, WolLoginLayout, WolLoginState,
};

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
    if let Some(choice) = layout.entry_choice() {
        return paint_entry_choice(state, ui, atlas, &choice, scale);
    }
    let label = |key: &str| state.csf_text(key);
    let text_of = |id: i32| {
        let id = u16::try_from(id).ok()?;
        let at = |ids: [u16; 2]| ids.iter().position(|other| *other == id);
        if let Some(index) = at(LABELS) {
            return Some(label(LABEL_KEYS[index]));
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
            // The chooser welcomes the player; the account form names its route.
            HEADING_TEXT => Some(label("VERA:SignInAndPlay")),
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
    if layout.policy == LoginPolicy::Account && login.form.is_some() {
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
        let edit = gui.edit_state(node).unwrap_or_default();
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

/// VERA online's entry chooser over the login page: Create Room's Red Alert 2
/// picture of the Tesla Trooper (the mode-2 box picture without that art),
/// the welcome and the lobby's player count centred on it in larger shadowed
/// text, and its two MNBTTN buttons. A button shows MNBTTN's lit frame while
/// the pointer is over it, else while it holds the keyboard focus; the gui's
/// nodes own the press, hover and focus.
fn paint_entry_choice(
    state: &AppState,
    ui: &DialogController,
    atlas: &SkirmishShellChromeAtlas,
    choice: &WolEntryChoiceLayout,
    scale: ShellScale,
) -> GuiPaint {
    let mut paint = GuiPaint::default();
    let font = &state.renderer.bit_font;
    let label = |key: &str| state.csf_text(key);
    let background = atlas
        .ra2_dialog_background
        .or_else(|| atlas.mode_two_dialog_background(None));
    match background {
        Some(background) => super::chrome::push_entry_fitted(
            &mut paint.popup,
            background,
            choice.dialog,
            scale,
            POPUP_DEPTHS.background,
        ),
        None => super::chrome::push_solid_rect(
            &mut paint.popup,
            atlas,
            choice.dialog,
            super::SHELL_MODAL_PANEL_RGB,
            POPUP_DEPTHS.background,
        ),
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
    let gui = ui.gui();
    let root = ui.root_in(DIALOG);
    let held = root.and_then(|root| crate::ui::shell::page::held_in(gui, root));
    let id_of = |node| gui.id_of(node).and_then(|id| u16::try_from(id).ok());
    let lit = ui
        .hovered_node_in(DIALOG)
        .and_then(id_of)
        .filter(|id| matches!(*id, PLAY_AS_GUEST | SIGN_IN_AND_PLAY))
        .or_else(|| gui.focused_control().and_then(id_of));
    for (id, key, rect) in [
        (PLAY_AS_GUEST, "VERA:PlayAsGuest", choice.guest),
        (SIGN_IN_AND_PLAY, "VERA:SignInAndPlay", choice.account),
    ] {
        let pressed = held.is_some() && held == ui.control(DIALOG, id);
        let mut frames = super::type3_button_frames(atlas);
        if lit == Some(id) {
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
        paint.popup_texts.push(font.fitted(fit, |font| {
            shell_text::draw_in_rect_scaled(
                font,
                &value,
                rect_to_text_rect(caption),
                SHELL_LABEL_TEXT_RGB,
                centre,
                [0.0; 2],
                POPUP_DEPTHS.text,
                scale.factor(),
            )
        }));
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
    let style = GuiStyle::popup();
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
