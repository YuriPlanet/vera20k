//! `0x10F`'s app adapter: creation from `0x00787010` (after the server list),
//! pointer and key routing into [`WolLoginState`], and its close.

use crate::util::time::Instant;

use super::service::{self, WolServiceRequest};
use super::{WolPage, WolVisit};
use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::{App, AppState};
use crate::ui::main_menu_dialogs::options::LauncherCue;
use crate::ui::shell::combobox::{ComboPopup, ComboPress};
use crate::ui::shell::modal::WolAccountFormLayout;
use crate::ui::skirmish_shell::{COMBO_DROPDOWN_ROW_H, COMBO_FACE_H};
use crate::ui::wol_account::{
    AccountDone, AccountFormAction, FORM_CANCEL, FORM_REMEMBER, WolAccountForm,
};
use crate::ui::wol_login::*;
use winit::keyboard::KeyCode;

pub(crate) fn login_layout(state: &AppState) -> WolLoginLayout {
    let (width, height) = shell_layout::shell_screen_size(state);
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
    if shell_layout::fills_window(state) {
        WolLoginLayout::responsive(width as i32, height as i32, policy)
    } else {
        WolLoginLayout::new(width as i32, height as i32, policy)
    }
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

/// The open popup's rects on the login page's shell mapping.
pub(crate) fn account_form_layout(state: &AppState) -> Option<WolAccountFormLayout> {
    let form = state.frontend.wol.as_ref()?.login()?.account.as_ref()?;
    Some(crate::ui::shell::modal::wol_account_form_layout_on(
        POPUP_AREA,
        form.fields.len(),
        form.remember.is_some(),
        login_layout(state).page.scale,
    ))
}

/// The nickname combo's drop list. ComboDrop's internals are untraced: the
/// list fills the template height below the face.
pub(crate) fn combo_popup(login: &WolLoginState, layout: &WolLoginLayout) -> Option<ComboPopup> {
    if login.policy == LoginPolicy::Online || login.popup.control.is_none() {
        return None;
    }
    let base = layout.page.scale.base_rect(layout.nickname_combo);
    let cap = ((base.h - COMBO_FACE_H - 1) / COMBO_DROPDOWN_ROW_H).max(1) as usize;
    Some(ComboPopup::new(
        layout.nickname_face,
        layout.page.scale,
        login.rows.len(),
        cap,
        login.popup.top_index,
    ))
}

/// After `0x00787010`'s silent steps (`0x007873CC..0x0078741B`): `0x72C600`
/// art, the empty backdrop and `UI__BeginDialog(0x10F)`. The nickname is the
/// process global the flow prefilled.
pub(super) fn open(state: &mut AppState) {
    let remembered = super::live_service::remembered_name(state);
    let last = &state.persistence.options_profile.online_name;
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    let login = if visit.provider == super::WolProvider::Online {
        WolLoginState::open_online(
            &mut state.frontend.wol_account,
            remembered.as_deref(),
            Some(last.as_str()),
        )
    } else {
        WolLoginState::open(&mut state.frontend.wol_account, &visit.credentials)
    };
    visit.page = WolPage::Login(Box::new(login));
    if visit.provider == super::WolProvider::Online {
        super::login_info::refresh(state);
    }
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
        WolLoginAction::ForgetSignIn => service::request(state, WolServiceRequest::ForgetSignIn),
    }
}

fn form_action(state: &mut AppState, action: AccountFormAction) {
    match action {
        AccountFormAction::Send(request) => {
            service::request(state, WolServiceRequest::Account(request));
        }
        AccountFormAction::SignIn { sign_in, remember } => {
            if let Some(action) = with_login(state, |login, globals, _| {
                login.sign_in(sign_in, remember, globals)
            }) {
                dispatch(state, action);
            }
        }
        AccountFormAction::ForgetSignIn => {
            with_login(state, |login, _, _| login.forget_remembered());
            dispatch(state, WolLoginAction::ForgetSignIn);
        }
        AccountFormAction::Close => {
            with_login(state, |login, _, _| login.account = None);
        }
    }
}

/// The lobby answered the account popup: a refusal shows in it, a sent code
/// moves it on, a confirmed account or new password signs in, and a deleted
/// account's popup says so.
pub(super) fn account_answered(state: &mut AppState, answer: Result<(), String>) {
    let action = with_login(state, |login, globals, _| {
        let form = login.account.as_mut().filter(|form| form.busy)?;
        match form.answered(answer)? {
            AccountDone::SignIn(sign_in) => Some(login.signed_up(sign_in, globals)),
            AccountDone::Deleted { name } => {
                login.account_deleted(&name);
                None
            }
        }
    })
    .flatten();
    update_geometry(state);
    if let Some(action) = action {
        dispatch(state, action);
    }
    state.platform.window.request_redraw();
}

/// `0x10F`'s teardown has run: Back returns 0 to `WOL_Main` (the welcome
/// again); Login starts the connect over the empty backdrop. The online
/// sign-in is the visit's first page, so its Back leaves WOL.
pub(super) fn commit_close(state: &mut AppState, result: u16) {
    state.frontend.wol_login_art = None;
    if result != LOGIN {
        if state
            .frontend
            .wol
            .as_ref()
            .is_some_and(|visit| visit.modern_ui())
        {
            App::return_from_wol(state);
        } else {
            App::reopen_wol_welcome(state);
        }
        return;
    }
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.page = WolPage::Backdrop { wait: None };
    }
    service::begin_connect(state);
}

pub(super) fn update_geometry(state: &mut AppState) {
    let layout = login_layout(state);
    let form_rects = account_form_layout(state);
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
    login.password.scroll_to_caret(
        (scale.base_rect(layout.password).w - 3).max(0) as u32,
        |value| font.text_width(value),
    );
    if let Some(form) = login.account.as_mut()
        && let Some(&(_, edit)) = form_rects
            .as_ref()
            .and_then(|rects| rects.fields.get(form.focused))
        && let Some(focused) = form.focused_edit_mut()
    {
        focused.scroll_to_caret((scale.base_rect(edit).w - 3).max(0) as u32, |value| {
            font.text_width(value)
        });
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
        _ => None,
    };
    let escape = code == Some(KeyCode::Escape);
    // An open account popup takes every key; Escape is its Cancel.
    let in_form = with_login(state, |login, _, _| {
        let form = login.account.as_mut()?;
        Some(match key {
            _ if escape => form.activate(FORM_CANCEL),
            Some(key) => form.edit_key(key),
            None => {
                form.insert_text(text.unwrap_or_default());
                None
            }
        })
    })
    .flatten();
    if let Some(action) = in_form {
        update_geometry(state);
        if let Some(action) = action {
            form_action(state, action);
        }
        state.platform.window.request_redraw();
        return;
    }
    // The NewEdits consume Escape; the proc has no IDCANCEL case.
    if escape {
        return;
    }
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
    let form_rects = account_form_layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    with_login(state, |login, _, _| {
        if let (Some(form), Some(rects)) = (login.account.as_mut(), &form_rects) {
            form.buttons.hovered = WolAccountForm::control_at(rects, x, y);
            return;
        }
        login.buttons.hovered = layout.control_at(x, y);
        if let Some(popup) = combo_popup(login, &layout) {
            login.popup.pointer_moved(popup, x, y);
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
    if let Some(rects) = account_form_layout(state) {
        account_mouse(state, &rects, pressed, x, y);
        return;
    }
    let over = layout.control_at(x, y);
    let scale = layout.page.scale;
    if !pressed {
        let action = with_login(state, |login, globals, _| {
            login.popup.cancel_pointer();
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
            let press = login.popup.press_list(popup, x, y, Instant::now());
            if let ComboPress::Pick(row) = press {
                login.select_row(row, globals, credentials);
            }
            return press
                .cue()
                .map_or(Press::Nothing, |cue| Press::Cue(cue.into()));
        }
        match over {
            Some(NICKNAME) => {
                // The editable combo's edit child takes every press outside
                // the arrow zone, so only an opening press reaches the combo.
                if login.policy == LoginPolicy::Legacy {
                    let selected = login
                        .rows
                        .iter()
                        .position(|row| row.nickname.eq_ignore_ascii_case(&globals.nickname));
                    let press =
                        login
                            .popup
                            .press_face((), layout.nickname_face, scale, selected, x, y);
                    if press == ComboPress::Open {
                        return Press::Cue(LauncherCue::ComboOpen);
                    }
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

/// The popup over the page takes the pointer. Remember password toggles on
/// a press on its box, as the Create Game box's checkboxes.
fn account_mouse(
    state: &mut AppState,
    rects: &WolAccountFormLayout,
    pressed: bool,
    x: i32,
    y: i32,
) {
    let over = WolAccountForm::control_at(rects, x, y);
    let scale = login_layout(state).page.scale;
    let on_box = rects.remember.is_some_and(|rect| {
        crate::ui::skirmish_shell::checkbox_icon_rect(rect, scale).contains(x, y)
    });
    let (press, action) = with_login(state, |login, _, _| {
        let form = login.account.as_mut()?;
        Some(if !pressed {
            let action = form
                .buttons
                .release(over)
                .and_then(|control| form.activate(control));
            (Press::Nothing, action)
        } else if over == Some(FORM_REMEMBER) {
            form.buttons.press(None);
            if on_box {
                (
                    Press::Cue(LauncherCue::Checkbox),
                    form.activate(FORM_REMEMBER),
                )
            } else {
                (Press::Nothing, None)
            }
        } else if form.press(over) {
            (Press::Button, None)
        } else {
            (Press::Nothing, None)
        })
    })
    .flatten()
    .unwrap_or((Press::Nothing, None));
    match press {
        Press::Cue(cue) => App::play_launcher_options_cue(state, cue),
        // Owner-draw buttons play GUIMainButtonSound on the press.
        Press::Button => App::play_main_menu_button_sound(state),
        Press::Nothing => {}
    }
    if let Some(action) = action {
        form_action(state, action);
    }
    update_geometry(state);
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
    if login.popup.scroll.poll(
        popup.scroll_geometry(),
        &mut login.popup.top_index,
        x,
        y,
        Instant::now(),
    ) {
        state.platform.window.request_redraw();
    }
    login.popup.scroll.repeat_at()
}
