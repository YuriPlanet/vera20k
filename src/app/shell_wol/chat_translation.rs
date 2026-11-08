//! The online lobby's Chat Translation page
//! ([`crate::ui::wol_chat_translation`]). Like WOL Options `0x2BC` it runs over
//! the hidden lobby, whose service sinks keep writing to it. The settings
//! belong to the options profile: the page edits a copy, and OK saves it.

use super::WolPage;
use crate::app::frontend::{shell_layout, shell_transition::ShellExitThen};
use crate::app::{App, AppState};
use crate::ui::chat_translation::ChatTranslationSettings;
use crate::ui::main_menu_dialogs::options::LauncherCue;
use crate::ui::shell::combobox::ComboPress;
use crate::ui::wol_chat_translation::{
    ChatTranslationAction, DIALOG, LANGUAGE, PAGE, WolChatTranslationLayout, WolChatTranslationPage,
};
use crate::util::time::Instant;

pub(crate) fn layout(state: &AppState) -> WolChatTranslationLayout {
    let (width, height) = shell_layout::shell_screen_size(state);
    if shell_layout::fills_window(state) {
        WolChatTranslationLayout::responsive(width as i32, height as i32)
    } else {
        WolChatTranslationLayout::new(width as i32, height as i32)
    }
}

pub(crate) fn page(state: &AppState) -> Option<&WolChatTranslationPage> {
    match &state.frontend.wol.as_ref()?.page {
        WolPage::ChatTranslation(page) => Some(page),
        _ => None,
    }
}

fn page_mut(state: &mut AppState) -> Option<&mut WolChatTranslationPage> {
    match &mut state.frontend.wol.as_mut()?.page {
        WolPage::ChatTranslation(page) => Some(page),
        _ => None,
    }
}

pub(super) fn page_open(state: &AppState) -> bool {
    page(state).is_some()
}

/// The lobby has slid out over the empty backdrop.
pub(crate) fn commit_open(state: &mut AppState) {
    let settings = state.persistence.options_profile.chat_translation;
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    if let Some(lobby) = visit.lobby_mut() {
        lobby.shown = false;
        lobby.ui.buttons = Default::default();
    }
    visit.page = WolPage::ChatTranslation(Box::new(WolChatTranslationPage::open(settings)));
    super::options::open_art(state);
    state.frontend.shell_controller.reset_to(DIALOG, false);
    state.platform.window.request_redraw();
}

/// The page has slid out: the lobby shows again and OK's settings save.
pub(crate) fn commit_close(state: &mut AppState, staged: Option<ChatTranslationSettings>) {
    state.frontend.wol_options_art = None;
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.page = WolPage::Lobby;
    if let Some(lobby) = visit.lobby_mut() {
        lobby.shown = true;
        lobby.ui.refocus_chat();
    }
    state
        .frontend
        .shell_controller
        .reset_to(super::WOL_LOBBY_DIALOG, false);
    if let Some(staged) = staged {
        let profile = &mut state.persistence.options_profile;
        profile.chat_translation = staged;
        if let Some(config) = state.platform.game_config.as_ref()
            && let Err(error) = profile.commit_chat_translation(config.paths.settings_dir())
        {
            log::warn!("Could not save the chat translation settings: {error}");
        }
    }
    state.platform.window.request_redraw();
}

pub(super) fn cursor_moved(state: &mut AppState) {
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let over = layout.control_at(x, y);
    let Some(page) = page_mut(state) else {
        return;
    };
    let popup = page.popup_geometry(&layout);
    // The open list covers the controls under it.
    let over = over.filter(|_| !popup.is_some_and(|popup| popup.rect.contains(x, y)));
    page.hovered = over;
    page.buttons.hovered = over.filter(|id| PAGE.button(*id).is_some());
    if let Some(popup) = popup {
        page.popup.pointer_moved(popup, x, y);
    }
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
}

pub(super) fn mouse(state: &mut AppState, pressed: bool) {
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let over = layout.control_at(x, y);
    let scale = layout.page.scale;
    let Some(page) = page_mut(state) else {
        return;
    };
    let mut action = None;
    let mut cue = None;
    let mut button_sound = false;
    if !pressed {
        page.popup.cancel_pointer();
        let button = over.filter(|id| PAGE.button(*id).is_some());
        if let Some(control) = page.buttons.release(button) {
            action = page.activate(control);
        }
    } else if let Some(popup) = page.popup_geometry(&layout) {
        let press = page.popup.press_list(popup, x, y, Instant::now());
        if let ComboPress::Pick(row) = press {
            page.select_row(row);
        }
        cue = press.cue().map(LauncherCue::from);
    } else {
        match over {
            Some(control) if PAGE.button(control).is_some() => {
                page.buttons.press(Some(control));
                button_sound = true;
            }
            Some(LANGUAGE) => {
                let selected = Some(page.selected_row());
                let press = page
                    .popup
                    .press_face(LANGUAGE, layout.language, scale, selected, x, y);
                cue = press.cue().map(LauncherCue::from);
            }
            // A checkbox admits only its icon.
            Some(control)
                if layout.checkbox_rect(control).is_some_and(|rect| {
                    crate::ui::skirmish_shell::checkbox_icon_rect(rect, scale).contains(x, y)
                }) =>
            {
                if page.toggle(control) {
                    cue = Some(LauncherCue::Checkbox);
                }
            }
            _ => page.buttons.press(None),
        }
    }
    if button_sound {
        App::play_main_menu_button_sound(state);
    }
    if let Some(cue) = cue {
        App::play_launcher_options_cue(state, cue);
    }
    match action {
        Some(ChatTranslationAction::Ok(staged)) => {
            App::leave_shell_dialog(state, ShellExitThen::WolChatTranslationClose(Some(staged)))
        }
        Some(ChatTranslationAction::Cancel) => {
            App::leave_shell_dialog(state, ShellExitThen::WolChatTranslationClose(None))
        }
        None => {}
    }
    state.frontend.shell_status_line.repaint();
    state.platform.window.request_redraw();
}

/// Esc closes the open language list.
pub(super) fn close_list(state: &mut AppState) -> bool {
    let Some(page) = page_mut(state) else {
        return false;
    };
    let open = page.popup.control.is_some();
    page.popup.close();
    open
}

/// The status help under the pointer.
pub(crate) fn status_key(state: &AppState) -> Option<&'static str> {
    let page = page(state)?;
    page.tooltip_key(page.hovered?)
}
