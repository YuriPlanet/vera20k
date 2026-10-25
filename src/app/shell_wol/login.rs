//! `0x10F`'s app adapter: creation from `0x00787010` (after the server list),
//! pointer and key routing into [`WolLoginState`], and its close.

use std::time::Instant;

use super::service::{self, WolServiceRequest};
use super::{WolPage, WolVisit};
use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::{App, AppState};
use crate::ui::main_menu_dialogs::options::LauncherCue;
use crate::ui::shell::combobox::ComboPopup;
use crate::ui::skirmish_shell::{COMBO_DROPDOWN_ROW_H, COMBO_FACE_H};
use crate::ui::wol_login::*;
use winit::keyboard::KeyCode;

pub(crate) fn login_layout(state: &AppState) -> WolLoginLayout {
    let (width, height) = shell_layout::shell_screen_size(state);
    let layout = if shell_layout::fills_window(state) {
        WolLoginLayout::responsive(width as i32, height as i32)
    } else {
        WolLoginLayout::new(width as i32, height as i32)
    };
    let policy = if state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.provider == super::WolProvider::Online)
    {
        LoginPolicy::Online
    } else {
        LoginPolicy::Legacy
    };
    layout.with_policy(policy)
}

/// `0x0072C600`: LoginScreen.shp through LoginScreen.PAL, only at 800 wide.
pub(crate) fn open_login_art(state: &mut AppState) -> bool {
    if state.frontend.wol_login_art.is_some() {
        return true;
    }
    let Some(assets) = state.process_assets.manager() else {
        return false;
    };
    state.frontend.wol_login_art = crate::render::main_menu_shell_chrome::build_wol_page_art(
        &state.renderer.gpu,
        &state.renderer.batch_renderer,
        assets,
        shell_layout::art_key_screen_width(state),
        ("LoginScreen.shp", "LoginScreen.PAL"),
        &[],
    );
    state.frontend.wol_login_art.is_some()
}

/// The nickname combo's drop list. ComboDrop's internals are untraced: the
/// list fills the template height below the face.
pub(crate) fn combo_popup(login: &WolLoginState, layout: &WolLoginLayout) -> Option<ComboPopup> {
    if login.policy == LoginPolicy::Online || !login.combo_open {
        return None;
    }
    let base = layout.page.scale.base_rect(layout.nickname_combo);
    let cap = ((base.h - COMBO_FACE_H - 1) / COMBO_DROPDOWN_ROW_H).max(1) as usize;
    Some(ComboPopup::new(
        layout.nickname_face,
        layout.page.scale,
        login.rows.len(),
        cap,
        login.combo_top,
    ))
}

/// After `0x00787010`'s silent steps (`0x007873CC..0x0078741B`): `0x72C600`
/// art, the empty backdrop and `UI__BeginDialog(0x10F)`. The nickname is the
/// process global the flow prefilled.
pub(super) fn open(state: &mut AppState) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    let login = if visit.provider == super::WolProvider::Online {
        WolLoginState::open_online(&mut state.frontend.wol_account)
    } else {
        WolLoginState::open(&mut state.frontend.wol_account, &visit.credentials)
    };
    visit.page = WolPage::Login(Box::new(login));
    open_login_art(state);
    let _ = App::ensure_skirmish_shell_chrome(state);
    state.frontend.shell_controller.reset_to(DIALOG, true);
    state.platform.window.request_redraw();
}

/// The login page and the visit's credential store, with the account globals.
fn with_login<R>(
    state: &mut AppState,
    f: impl FnOnce(&mut WolLoginState, &mut WolAccountGlobals, &[WolCredential]) -> R,
) -> Option<R> {
    let frontend = &mut state.frontend;
    let visit: &mut WolVisit = frontend.wol.as_mut()?;
    let WolPage::Login(login) = &mut visit.page else {
        return None;
    };
    Some(f(login, &mut frontend.wol_account, &visit.credentials))
}

fn dispatch(state: &mut AppState, action: WolLoginAction) {
    match action {
        WolLoginAction::Close(result) => App::leave_shell_dialog(
            state,
            shell_transition::ShellExitThen::WolLoginClose(result),
        ),
        WolLoginAction::Forget { slot, nickname } => {
            // 0x0079A6F0..0x0079A7B6: a lowered match clears AutoLogin, then
            // LastValid.
            if let Some(file) = super::persona::info_file(state) {
                let name = nickname.to_ascii_lowercase();
                if file.auto_login().to_ascii_lowercase() == name {
                    file.set_auto_login("");
                }
                if file.last_valid().to_ascii_lowercase() == name {
                    file.set_last_valid("");
                }
            }
            if let Some(visit) = state.frontend.wol.as_mut() {
                visit.credentials.retain(|stored| stored.slot != slot);
            }
            service::request(state, WolServiceRequest::ForgetCredential { slot });
        }
        // 0x00773E90 needs the registry URL `Manage` and an HTMLPath; a
        // retail install has neither and it returns.
        WolLoginAction::Manage => {}
    }
}

/// `0x10F`'s teardown has run: Back returns 0 to `WOL_Main` (the welcome
/// again); Login starts the connect over the empty backdrop.
pub(super) fn commit_close(state: &mut AppState, result: u16) {
    state.frontend.wol_login_art = None;
    if result != LOGIN {
        App::reopen_wol_welcome(state);
        return;
    }
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.page = WolPage::Backdrop { wait: None };
    }
    service::begin_connect(state);
}

pub(super) fn update_geometry(state: &mut AppState) {
    let layout = login_layout(state);
    let font = &state.renderer.bit_font;
    let Some(WolPage::Login(login)) = state.frontend.wol.as_mut().map(|visit| &mut visit.page)
    else {
        return;
    };
    let scale = layout.page.scale;
    let text = layout.nickname_text_rect();
    login
        .nickname
        .scroll_to_caret((scale.base_rect(text).w - 3).max(0) as u32, |value| {
            font.text_width(value)
        });
    if login.policy == LoginPolicy::Legacy {
        login.password.scroll_to_caret(
            (scale.base_rect(layout.password).w - 3).max(0) as u32,
            |value| font.text_width(value),
        );
    }
}

pub(super) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    let reverse = state.platform.live_modifiers.shift_key();
    let key = match code {
        Some(KeyCode::Backspace) => Some(WolEditKey::Backspace),
        Some(KeyCode::Delete) => Some(WolEditKey::Delete),
        Some(KeyCode::ArrowLeft) => Some(WolEditKey::Left),
        Some(KeyCode::ArrowRight) => Some(WolEditKey::Right),
        Some(KeyCode::Home) => Some(WolEditKey::Home),
        Some(KeyCode::End) => Some(WolEditKey::End),
        Some(KeyCode::Tab) => Some(WolEditKey::Tab { reverse }),
        Some(KeyCode::Enter | KeyCode::NumpadEnter) => Some(WolEditKey::Enter),
        // The NewEdits consume Escape; the proc has no IDCANCEL case.
        Some(KeyCode::Escape) => return,
        _ => None,
    };
    let action = with_login(state, |login, globals, credentials| match key {
        Some(key) => login.edit_key(key, globals, credentials),
        None => {
            login.insert_text(text.unwrap_or_default(), globals, credentials);
            None
        }
    })
    .flatten();
    update_geometry(state);
    if let Some(action) = action {
        dispatch(state, action);
    }
    state.platform.window.request_redraw();
}

pub(super) fn cursor_moved(state: &mut AppState) {
    let layout = login_layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    with_login(state, |login, _, _| {
        login.buttons.hovered = layout.control_at(x, y);
        if let Some(popup) = combo_popup(login, &layout) {
            if let Some(row) = popup.row_at(x, y) {
                login.combo_hot_row = Some(row);
            }
            login
                .combo_scroll
                .pointer_moved(popup.scroll_geometry(), &mut login.combo_top, x, y);
        }
    });
    state.platform.window.request_redraw();
}

enum Press {
    Nothing,
    Cue(LauncherCue),
    Button,
}

pub(super) fn mouse(state: &mut AppState, pressed: bool) {
    let layout = login_layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let over = layout.control_at(x, y);
    let scale = layout.page.scale;
    if !pressed {
        let action = with_login(state, |login, globals, _| {
            login.combo_scroll.cancel();
            login
                .buttons
                .release(over)
                .and_then(|control| login.activate(control, globals))
        })
        .flatten();
        update_geometry(state);
        if let Some(action) = action {
            dispatch(state, action);
        }
        state.platform.window.request_redraw();
        return;
    }
    let press = with_login(state, |login, globals, credentials| {
        if let Some(popup) = combo_popup(login, &layout) {
            if let Some(part) = popup.scroll_part_at(x, y) {
                login.combo_scroll.press(
                    part,
                    popup.scroll_geometry(),
                    &mut login.combo_top,
                    y,
                    Instant::now(),
                );
                return Press::Nothing;
            }
            match popup.row_at(x, y).filter(|row| *row < login.rows.len()) {
                Some(row) => login.select_row(row, globals, credentials),
                None => login.combo_open = false,
            }
            return Press::Cue(LauncherCue::ComboClose);
        }
        match over {
            Some(NICKNAME) => {
                if login.policy == LoginPolicy::Legacy
                    && crate::ui::skirmish_shell::combo_arrow_hit_rect(layout.nickname_face, scale)
                        .contains(x, y)
                {
                    login.combo_open = true;
                    login.combo_top = 0;
                    login.combo_hot_row = login
                        .rows
                        .iter()
                        .position(|row| row.nickname.eq_ignore_ascii_case(&globals.nickname));
                    return Press::Cue(LauncherCue::ComboOpen);
                }
                login.focus(NICKNAME);
                Press::Nothing
            }
            Some(PASSWORD) => {
                login.focus(PASSWORD);
                Press::Nothing
            }
            Some(REMEMBER) => {
                // OwnerDraw_CheckBox 0x006163A0 admits only its 18x18 icon
                // and toggles on the press; a disabled one takes nothing.
                if login.remember_enabled
                    && crate::ui::skirmish_shell::checkbox_icon_rect(layout.remember, scale)
                        .contains(x, y)
                {
                    login.toggle_remember(globals);
                    return Press::Cue(LauncherCue::Checkbox);
                }
                Press::Nothing
            }
            Some(control) if login.button_enabled(control) => {
                login.buttons.press(Some(control));
                Press::Button
            }
            _ => {
                login.buttons.press(None);
                Press::Nothing
            }
        }
    });
    match press {
        Some(Press::Cue(cue)) => App::play_launcher_options_cue(state, cue),
        // Owner-draw buttons play GUIMainButtonSound on the press.
        Some(Press::Button) => App::play_main_menu_button_sound(state),
        _ => {}
    }
    state.platform.window.request_redraw();
}

/// Shared scrollbar repeats of the open drop list.
pub(crate) fn poll_scroll_repeat(state: &mut AppState) -> Option<Instant> {
    let layout = login_layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let WolPage::Login(login) = &mut state.frontend.wol.as_mut()?.page else {
        return None;
    };
    let popup = combo_popup(login, &layout)?;
    if login.combo_scroll.poll(
        popup.scroll_geometry(),
        &mut login.combo_top,
        x,
        y,
        Instant::now(),
    ) {
        state.platform.window.request_redraw();
    }
    login.combo_scroll.repeat_at()
}
