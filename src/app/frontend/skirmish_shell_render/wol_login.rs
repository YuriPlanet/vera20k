//! `0x10F`'s left-side children: its widgets through the generic painter,
//! the two labels, and the nickname combo's NewEdit inside the combo's face
//! with the drop list's rows; VERA online's page has its entry chooser, or
//! its sign-in form and the lobby's counts in their place, and its popups
//! over them. The WOL page compositor draws them over its own backdrop and
//! chrome.

use vera20k_gui::NodeKind;

use super::gui_paint::{self, ButtonArt, EditFace, GuiStyle, HostRows};
use super::wol_lobby::{POPUP_DEPTHS, WolLobbyPaint};
use crate::app::frontend::state::FrontendState;
use crate::app::input::ime::ImeState;
use crate::app::match_runtime::state::MatchState;
use crate::app::process_assets::ProcessAssets;
use crate::render::bit_font::BitFont;
use crate::render::hud_paint::{HudPaint, Material};
use crate::render::shell_paint;
use crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas;
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::geom::ShellScale;
use crate::ui::shell::modal::{WolAccountFormLayout, WolEntryChoiceLayout};
use crate::ui::wol_account::{
    FIRST_CAPTION, FORM_CANCEL, FORM_OK, FORM_STATUS, POPUP, WolAccountForm,
};
use crate::ui::wol_login::{
    CAPTIONS_TEXT, CHOICE_BOX, CHOICE_COUNT, CHOICE_HEADING, COUNTS_TEXT, DIALOG, HEADING_TEXT,
    LABEL_KEYS, LABELS, LoginPolicy, NICKNAME, NICKNAME_DROP, PLAY_AS_GUEST, SIGN_IN_AND_PLAY,
    STATUS_TEXT, WolLoginLayout, WolLoginState,
};

/// `0x10F`'s widgets and the statics whose text the page keeps; gamemd's
/// page adds the combo's NewEdit, VERA online's has its form in its place.
pub(crate) fn paint_wol_login_children(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    login: &WolLoginState,
    layout: &WolLoginLayout,
) -> HudPaint {
    let ui = &frontend.shell_controller;
    let (gui, Some(root), Some(atlas)) = (
        ui.gui(),
        ui.root_in(DIALOG),
        frontend.skirmish_shell_chrome.as_ref(),
    ) else {
        return HudPaint::default();
    };
    let scale = layout.page.scale;
    if let Some(choice) = layout.entry_choice() {
        return paint_entry_choice(
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            ui,
            atlas,
            &choice,
            scale,
        );
    }
    let label = |key: &str| process_assets.csf_text(key);
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
            let (key, count) = frontend.login_info.counts().nth(index)?;
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
        return gui_paint::paint_gui_styled(
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            gui,
            root,
            scale,
            &[],
            style,
        );
    }
    // The panel holds every control but the combo's NewEdit.
    let children = gui.tree().children(root).unwrap_or_default();
    let panel = children
        .iter()
        .find(|node| gui.tree().kind(**node) == Ok(NodeKind::Panel));
    let mut paint = panel.map_or_else(HudPaint::default, |panel| {
        gui_paint::paint_gui_styled(
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            gui,
            *panel,
            scale,
            &[],
            style,
        )
    });
    // The combo's NewEdit has no frame of its own: its caret and text paint
    // in the face's text area, its window. ComboDrop's edit-child geometry
    // is untraced; the face's text rect stands in for it.
    if let Some(node) = ui.control(DIALOG, NICKNAME) {
        let face = |id: i32| (id == i32::from(NICKNAME)).then_some(EditFace::ComboText);
        let style = GuiStyle {
            edit_face_of: Some(&face),
            ..style
        };
        gui_paint::paint_gui_into(
            &mut paint,
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            Some(atlas),
            gui,
            node,
            scale,
            &[],
            style,
        );
    }
    paint
}

/// VERA online's entry chooser over the login page: Create Room's Red Alert 2
/// picture of the Tesla Trooper (the mode-2 box picture without that art),
/// then its box's widgets: the welcome and the lobby's player count centred
/// in larger shadowed text, and its two MNBTTN buttons. A button shows
/// MNBTTN's lit frame while the pointer is over it, else while it holds the
/// keyboard focus; the gui's nodes own the press, hover and focus. All of it
/// paints in the popup lanes.
fn paint_entry_choice(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    ui: &DialogController,
    atlas: &SkirmishShellChromeAtlas,
    choice: &WolEntryChoiceLayout,
    scale: ShellScale,
) -> HudPaint {
    let mut paint = HudPaint::default();
    let label = |key: &str| process_assets.csf_text(key);
    let background = atlas
        .ra2_dialog_background
        .or_else(|| atlas.mode_two_dialog_background(None));
    paint.draw(Material::Popup, |out, _| match background {
        Some(background) => super::chrome::push_entry_fitted(
            out,
            background,
            choice.dialog,
            scale,
            POPUP_DEPTHS.background,
        ),
        None => super::chrome::push_solid_rect(
            out,
            atlas,
            choice.dialog,
            super::SHELL_MODAL_PANEL_RGB,
            POPUP_DEPTHS.background,
        ),
    });
    let Some(chooser) = ui.control(DIALOG, CHOICE_BOX) else {
        return paint;
    };
    let gui = ui.gui();
    let id_of = |node| gui.id_of(node).and_then(|id| u16::try_from(id).ok());
    let lit = ui
        .hovered_node_in(DIALOG)
        .and_then(id_of)
        .filter(|id| matches!(*id, PLAY_AS_GUEST | SIGN_IN_AND_PLAY))
        .or_else(|| gui.focused_control().and_then(id_of));
    let lit_of = |id: i32| lit.is_some_and(|lit| i32::from(lit) == id);
    let text_of = |id: i32| match u16::try_from(id).ok()? {
        CHOICE_HEADING => Some(label("VERA:OnlineWelcome")),
        CHOICE_COUNT => {
            let count = frontend
                .login_info
                .players_online()
                .map_or_else(|| "—".to_owned(), |count| count.to_string());
            Some(crate::i18n::fill(
                &label("VERA:PlayersOnline"),
                &[("count", &count)],
            ))
        }
        _ => None,
    };
    let glyph_of = |id: i32| match u16::try_from(id).ok()? {
        CHOICE_HEADING => Some(1.4),
        CHOICE_COUNT => Some(1.25),
        _ => None,
    };
    let style = GuiStyle {
        buttons: ButtonArt::ModalLit,
        text_of: Some(&text_of),
        lit_of: Some(&lit_of),
        glyph_of: Some(&glyph_of),
        shadowed_texts: true,
        ..Default::default()
    };
    let widgets = gui_paint::paint_gui_styled(
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        gui,
        chooser,
        scale,
        &[],
        style,
    );
    paint.draw(Material::Popup, |out, _| {
        out.extend(widgets.lane(Material::Controls).copied())
    });
    paint.popup_texts.extend(widgets.texts);
    paint
}

/// VERA online's popups on Red Alert 2's box art, as the online Create Game
/// box: a caption and edit per field, OK and Cancel, and the status line.
pub(crate) fn paint_wol_account_form(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    form: &WolAccountForm,
    layout: &WolAccountFormLayout,
    scale: ShellScale,
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    let ui = &frontend.shell_controller;
    let (Some(atlas), Some(root)) = (frontend.skirmish_shell_chrome.as_ref(), ui.root_in(POPUP))
    else {
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
            FORM_OK => Some(process_assets.csf_text("GUI:OK")),
            FORM_CANCEL => Some(process_assets.csf_text("GUI:Cancel")),
            FORM_STATUS => Some(form.status.clone()),
            _ => Some(
                process_assets.csf_text(
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
    let widgets = gui_paint::paint_gui_styled(
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        ui.gui(),
        root,
        scale,
        &[],
        style,
    );
    paint
        .controls
        .extend(widgets.lane(Material::Controls).copied());
    paint.texts = widgets.texts;
    paint
}
