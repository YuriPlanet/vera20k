//! `0x10F`'s app adapter: creation from `0x00787010` (after the server list),
//! its widgets on the shell's GUI tree, the pointer and keys into
//! [`WolLoginState`], and its close.

use vera20k_gui::{Key, NodeKind, NodeSpec, Prop, Value};

use super::leaf::{self, Pointer};
use super::service::{self, WolServiceRequest};
use super::{WolPage, WolVisit};
use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::{App, AppState};
use crate::ui::shell::modal::WolAccountFormLayout;
use crate::ui::wol_account::{AccountSuccess, POPUP, WolAccountForm};
use crate::ui::wol_login::*;
use winit::keyboard::KeyCode;

pub(crate) fn login_layout(state: &AppState) -> WolLoginLayout {
    // The page's own step: the legacy WOL page, or VERA online's entry
    // chooser or account page.
    let policy = state
        .frontend
        .wol
        .as_ref()
        .and_then(WolVisit::login)
        .map_or(LoginPolicy::Legacy, |login| login.policy);
    shell_layout::on(state, |w, h, scale| {
        WolLoginLayout::new(w, h, scale, policy)
    })
}

/// The open popup's rects on the login page's shell mapping.
pub(crate) fn account_form_layout(state: &AppState) -> Option<WolAccountFormLayout> {
    let form = state.frontend.wol.as_ref()?.login()?.account.as_ref()?;
    let scale = login_layout(state).page.scale;
    Some(crate::ui::shell::modal::wol_account_form_layout_on(
        POPUP_AREA,
        form.fields.len(),
        scale,
    ))
}

/// After `0x00787010`'s silent steps (`0x007873CC..0x0078741B`): `0x72C600`
/// art, the empty backdrop and `UI__BeginDialog(0x10F)`. The nickname is the
/// process global the flow prefilled.
pub(super) fn open(state: &mut AppState) {
    let remembered = super::live_service::remembered_name(state);
    let last = &state.persistence.options_profile.online_name;
    let frontend = &mut state.frontend;
    let Some(visit) = frontend.wol.as_mut() else {
        return;
    };
    frontend.shell_controller.reset_to(DIALOG);
    let ui = &mut frontend.shell_controller;
    let cx = &mut LoginCx {
        ui,
        globals: &mut frontend.wol_account,
        credentials: &visit.credentials,
    };
    let online = visit.provider == super::WolProvider::Online;
    let login = if online {
        WolLoginState::open_online(cx, remembered.as_deref(), Some(last.as_str()))
    } else {
        WolLoginState::open(cx)
    };
    visit.page = WolPage::Login(Box::new(login));
    if online {
        super::login_info::refresh(state);
    }
    super::leaf::open_art(state, DIALOG);
    let _ = App::ensure_skirmish_shell_chrome(state);
    sync(state);
}

/// The login page and what it works on; what the tree shows from the model
/// follows.
fn with_login<R>(
    state: &mut AppState,
    f: impl FnOnce(&mut WolLoginState, &mut LoginCx<'_>) -> R,
) -> Option<R> {
    let frontend = &mut state.frontend;
    let visit: &mut WolVisit = frontend.wol.as_mut()?;
    let WolPage::Login(login) = &mut visit.page else {
        return None;
    };
    let ui = &mut frontend.shell_controller;
    let cx = &mut LoginCx {
        ui,
        globals: &mut frontend.wol_account,
        credentials: &visit.credentials,
    };
    let result = f(login, cx);
    login.refresh(cx);
    Some(result)
}

/// Puts the top dialog's widgets, the page's or its popup's, on the layout.
fn sync(state: &mut AppState) {
    let layout = login_layout(state);
    let scale = layout.page.scale;
    if let Some(rects) = account_form_layout(state) {
        // Its form built the popup's nodes.
        leaf::sync(
            state,
            POPUP,
            scale,
            || NodeSpec::new(NodeKind::Dialog),
            |id| WolAccountForm::rect_of(&rects, id),
        );
        return;
    }
    if leaf::sync(
        state,
        DIALOG,
        scale,
        || spec(layout.policy),
        |id| layout.rect_of(id),
    )
    .is_none()
    {
        return;
    }
    let ui = &mut state.frontend.shell_controller;
    ui.set_prop(
        DIALOG,
        NICKNAME,
        Prop::HitRect,
        Value::Rect(layout.nickname_hit()),
    );
    ui.set_prop(
        DIALOG,
        NICKNAME_DROP,
        Prop::VisibleRows,
        Value::Int(layout.drop_rows()),
    );
}

/// The widgets back on the layout (edits scrolled to their carets), then
/// what the input asked for.
fn finish(state: &mut AppState, action: Option<WolLoginAction>) {
    sync(state);
    if let Some(action) = action {
        dispatch(state, action);
    }
}

fn dispatch(state: &mut AppState, action: WolLoginAction) {
    match action {
        WolLoginAction::Close(result) => App::leave_shell_dialog(
            state,
            shell_transition::ShellSlideKind::WolLogin,
            move |state| App::commit_wol_login_close(state, result),
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
        WolLoginAction::Send(request) => {
            if let Some(visit) = &mut state.frontend.wol {
                visit.account_attempt = visit.account_attempt.wrapping_add(1);
                visit.requests.push(WolServiceRequest::Account {
                    attempt: visit.account_attempt,
                    request,
                });
            }
        }
    }
}

/// The lobby answered the account popup: a refusal shows in it, a sent code
/// moves it on, a confirmed account or new password signs in, and a deleted
/// account's popup says so.
pub(super) fn account_answered(state: &mut AppState, answer: Result<AccountSuccess, String>) {
    let action = with_login(state, |login, cx| {
        let form = login.account.as_mut().filter(|form| form.busy)?;
        let sign_in = form.answered(cx.ui, answer)?;
        Some(login.signed_up(cx, sign_in))
    });
    finish(state, action.flatten());
}

/// `0x10F`'s teardown has run: Back returns 0 to `WOL_Main` (the welcome
/// again); Login starts the connect over the empty backdrop. The online
/// sign-in is the visit's first page, so its Back leaves WOL.
pub(super) fn commit_close(state: &mut AppState, result: u16) {
    super::leaf::free_art(state, DIALOG);
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

pub(super) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    let shift = state.platform.live_modifiers.shift_key();
    let key = code.and_then(|code| crate::ui::shell::controller::gui_key(code, shift));
    if matches!(key, Some(Key::Enter | Key::Escape)) {
        sync(state);
    }
    let action = with_login(state, |login, cx| match key {
        Some(key @ (Key::Enter | Key::Escape)) => login.dialog_key(cx, key),
        Some(key) => {
            login.edit_key(cx, key);
            None
        }
        None => {
            login.insert_text(cx, text.unwrap_or_default());
            None
        }
    });
    finish(state, action.flatten());
}

pub(super) fn cursor_moved(state: &mut AppState) {
    sync(state);
    leaf::pointer(state, Pointer::Move);
    // VERA online's chooser: the button under the pointer takes the focus.
    with_login(state, |login, cx| login.pointer_moved(cx.ui));
}

pub(super) fn mouse(state: &mut AppState, pressed: bool) {
    sync(state);
    let events = leaf::pointer(state, if pressed { Pointer::Down } else { Pointer::Up });
    let action = with_login(state, |login, cx| login.handle(cx, events));
    finish(state, action.flatten());
}

/// The status help under the pointer (`0x006040B0`). Over the nickname
/// combo's open list it stays the help of the control under the list: the
/// page's hit test takes no notice of the list.
pub(crate) fn status_key(state: &AppState) -> Option<&str> {
    let ui = &state.frontend.shell_controller;
    let list = ui
        .control(DIALOG, NICKNAME_DROP)
        .and_then(|node| ui.gui().combo_drop_down(node));
    let (x, y) = state.match_state.input.cursor_px();
    if !list.is_some_and(|list| list.popup.rect.contains(x, y)) {
        return leaf::status_key(state, DIALOG);
    }
    // The control the page's widgets hit under the list; the nickname's
    // edit and its combo share the combo's help.
    let gui = ui.gui();
    let node = gui.hit_at(ui.root_in(DIALOG)?, x, y, false)?;
    let id = u16::try_from(gui.id_of(node)?).ok()?;
    tooltip_key(if id == NICKNAME_DROP { NICKNAME } else { id })
}
