//! OS input-method (IME) text entry for the chat edits.
//!
//! gamemd removes its main window's input context when it creates the window
//! (CreateMainWindow `0x00777D80`: ImmGetContext, then
//! `ImmAssociateContext(hWnd, 0)`), restores it when the in-game chat edit
//! opens (MessageListClass::AddEdit `0x005D4210` calls `0x00778180`) and
//! removes it again on every edit end (`0x007781A0`, from Input `0x005D4510`'s
//! end paths and Start_Scenario `0x00683E6B`). The shell's NewEdits are child
//! windows, which that call leaves alone. A finished character arrives as
//! WM_IME_CHAR: the main window queues it at `0x00B730EC`
//! (Game__MainWindowProc `0x007775C0`) and the chat pre-hook `0x0055E420`
//! feeds each queued unit to Input first; NewEdit `0x00614B30` widens it
//! (`0x007781B0`) and inserts it through its WM_CHAR path. The focused NewEdit
//! paints the open composition (`0x00777EA0`) at its caret (`0x00623880`).
//!
//! VERA20k takes the composition and the finished text from winit's `Ime`
//! events, and asks for an input method only while a chat edit takes keys:
//! the in-game line, the WOL lobby (`0x10F`), the WOL game room and LAN setup
//! (C2/C9, BC/BD) and the LAN lobby (BB). Finished text goes through each
//! screen's own text path, as WM_IME_CHAR does.
//!
//! Player-name edits take no input method: NewEdit `0x00614B30` drops units at
//! or above 0x100 from them (`0x00615226..0x0061527A`, the byte-only filter),
//! so gamemd lets in typed keys as code-page bytes but no widened
//! input-method character such as Chinese.
//!
//! RESIDUAL: native's other NewEdits (login, page, buddy, saved-game
//! descriptions) also take IME input; here they keep plain key text. In
//! battle, native leaves the composition to the system's default IME window
//! (OwnerDraw__ServiceIMEMessage `0x00778030` passes the composition messages
//! to DefWindowProc). winit hides that window, so VERA20k draws the
//! composition after the typed text in the NewEdit composition colour.
//!
//! winit's web backend has no input-method support; in the browser `web::ime`
//! supplies these same events. RESIDUAL: there the candidate window opens at
//! the edit's left end plus the composition, not after the typed text.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::ui::shell::geom::RectPx;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::Ime;

/// The composition colour `[0xAC4618]`, COLORREF `0x9F9F` that
/// ShellDialog__SubclassChildByClass `0x0060F9A0` sets.
pub(crate) const COMPOSITION_RGB: [u8; 3] = [0x9F, 0x9F, 0x00];

/// The chat edit that takes IME input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImeTarget {
    InGameChat,
    WolLobbyChat,
    SetupChat,
    LanLobbyChat,
    /// Android only: the LAN lobby's name box, which otherwise takes plain key
    /// text (`app::shell_lan::ime_name_rect`).
    LanLobbyName,
    /// Android only: the online login page's focused edit, whether the legacy
    /// nickname/password page or the VERA Online sign-in form.
    WolLoginField,
}

/// Process-local input-method state. Presentation-only: never serialized or
/// read by the deterministic simulation.
#[derive(Debug, Default)]
pub(crate) struct ImeState {
    /// The value last handed to `Window::set_ime_allowed`.
    allowed: bool,
    target: Option<ImeTarget>,
    /// The composition string `0x00B73318`, in UTF-16 units.
    composition: Vec<u16>,
    /// The composition cursor `0x00B73568`, in UTF-16 units.
    cursor: usize,
    /// The area last handed to `Window::set_ime_cursor_area`.
    area: Option<RectPx>,
    /// Android: the buffer last read from the input method (`util::android_ime`).
    #[cfg(target_os = "android")]
    android_text: String,
    /// Android: the edit that buffer was seeded from.
    #[cfg(target_os = "android")]
    android_target: Option<ImeTarget>,
    /// Android: frames left to re-assert the seeded text while the platform
    /// still reports the previous edit's.
    #[cfg(target_os = "android")]
    android_reseed: u8,
    /// Android: the platform's buffer has echoed the seed at least once, so a
    /// difference from here on is the player's typing.
    #[cfg(target_os = "android")]
    android_synced: bool,
}

impl ImeState {
    /// The open composition and its cursor, when `target` owns it.
    pub(crate) fn composition(&self, target: ImeTarget) -> Option<(&[u16], usize)> {
        (self.target == Some(target) && !self.composition.is_empty())
            .then_some((self.composition.as_slice(), self.cursor))
    }

    /// While a composition is open, keys go to the input method (Windows
    /// delivers them as VK_PROCESSKEY), not to the edit.
    pub(crate) fn composing(&self) -> bool {
        !self.composition.is_empty()
    }

    /// OwnerDraw__CancelIMEComposition `0x00777E00`.
    fn cancel(&mut self) {
        self.composition.clear();
        self.cursor = 0;
    }
}

/// The chat edit that takes keys now, with its rectangle in window pixels.
/// The dialogs the key dispatch serves first leave none.
fn target(state: &AppState) -> Option<(ImeTarget, RectPx)> {
    if state.main_menu_dialog_open()
        || crate::app::input::coop_score::active(state)
        || crate::app::frontend::preview_zoom::is_open(state)
    {
        return None;
    }
    [
        (
            ImeTarget::InGameChat,
            crate::app::input::messages::ime_edit_rect(state),
        ),
        (
            ImeTarget::LanLobbyChat,
            crate::app::shell_lan::ime_chat_rect(state),
        ),
        (
            ImeTarget::LanLobbyName,
            crate::app::shell_lan::ime_name_rect(state),
        ),
        (
            ImeTarget::WolLoginField,
            crate::app::shell_wol::login::ime_rect(state),
        ),
        (
            ImeTarget::SetupChat,
            crate::app::input::multiplayer_setup::ime_chat_rect(state),
        ),
        (
            ImeTarget::WolLobbyChat,
            crate::app::shell_wol::lobby::ime_chat_rect(state),
        ),
    ]
    .into_iter()
    .find_map(|(target, rect)| rect.map(|rect| (target, rect)))
}

/// Ask for an input method exactly while a chat edit takes keys
/// (`0x00778180` / `0x007781A0`), and keep the system's candidate window at
/// that edit.
pub(crate) fn sync(state: &mut AppState) {
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    for ime in crate::web::ime::take_events() {
        event(state, ime);
    }
    let found = target(state);
    let target = found.map(|(target, _)| target);
    let platform = &mut state.platform;
    let ime = &mut platform.ime;
    if ime.target != target {
        log::info!("IME target {target:?}");
        ime.cancel();
        ime.target = target;
    }
    if ime.allowed != target.is_some() {
        ime.allowed = target.is_some();
        ime.area = None;
        platform.window.set_ime_allowed(ime.allowed);
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        crate::web::ime::set_allowed(ime.allowed);
    }
    if let Some((_, rect)) = found
        && ime.area != Some(rect)
    {        ime.area = Some(rect);
        platform.window.set_ime_cursor_area(
            PhysicalPosition::new(rect.x, rect.y),
            PhysicalSize::new(rect.w.max(1) as u32, rect.h.max(1) as u32),
        );
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        crate::web::ime::set_area(rect.x, rect.y, rect.w.max(1) as u32, rect.h.max(1) as u32);
    }
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    if ime.allowed {
        crate::web::ime::keep_focus();
    }
    #[cfg(target_os = "android")]
    android::sync(state, target);
    #[cfg(target_os = "android")]
    crate::app::shell_lan::take_touch_send(state);
}

/// Android: whether the soft keyboard is up, which keeps the frame loop polling
/// (the input method's commits do not wake winit, winit#4673).
pub(crate) fn keyboard_active(state: &AppState) -> bool {
    state.platform.ime.allowed
}

/// Android: the input method edits its own buffer (`util::android_ime`), so
/// seed it from the focused edit and turn each change into committed text.
#[cfg(target_os = "android")]
mod android {
    use super::*;

    /// The focused edit's text and caret, whichever screen owns it.
    fn seed(state: &AppState, focused: ImeTarget) -> Option<(String, usize)> {
        match focused {
            ImeTarget::LanLobbyChat | ImeTarget::LanLobbyName => {
                crate::app::shell_lan::ime_edit_text(state)
            }
            ImeTarget::WolLoginField => crate::app::shell_wol::login::ime_text(state),
            ImeTarget::InGameChat => crate::app::input::messages::ime_edit_state(state),
            ImeTarget::SetupChat => crate::app::input::multiplayer_setup::ime_text(state),
            _ => None,
        }
    }

    pub(super) fn sync(state: &mut AppState, target: Option<ImeTarget>) {
        let Some(focused) = target else {
            // Hide once, on the way out: re-issuing it every idle frame fights
            // the next edit's show request.
            let was_editing = state.platform.ime.android_target.take().is_some();
            if was_editing {
                crate::util::android_ime::hide();
            }
            let ime = &mut state.platform.ime;
            ime.android_text.clear();
            ime.android_reseed = 0;
            ime.android_synced = false;
            return;
        };
        let Some((text, cursor)) = seed(state, focused) else {
            return;
        };
        // A tap on an edit asks for the keyboard again, with the edit's text:
        // the player may have collapsed the keyboard with its own hide button,
        // which the engine never sees.
        if crate::util::android_ime::take_show_request() {
            crate::util::android_ime::set_state(&text, cursor);
            let ime = &mut state.platform.ime;
            ime.android_text = text;
            ime.android_target = Some(focused);
            ime.android_reseed = 8;
            ime.android_synced = false;
            crate::util::android_ime::show();
            return;
        }
        if state.platform.ime.android_target != Some(focused) {
            crate::util::android_ime::set_state(&text, cursor);
            let ime = &mut state.platform.ime;
            ime.android_text = text;
            ime.android_target = Some(focused);
            ime.android_reseed = 8;
            ime.android_synced = false;
            crate::util::android_ime::show();
            return;
        }
        // The platform keeps the previous edit's text for a while after the
        // seed, and its soft keyboard may ignore the first show request after
        // the previous edit closed. Re-assert both until it echoes the seed;
        // text it still reports afterwards is stale, adopted as the baseline so
        // an old message is never typed into the new edit.
        if !state.platform.ime.android_synced {
            let current = crate::util::android_ime::state();
            if current.text == state.platform.ime.android_text {
                state.platform.ime.android_synced = true;
                return;
            }
            if state.platform.ime.android_reseed > 0 {
                state.platform.ime.android_reseed -= 1;
                crate::util::android_ime::set_state(&state.platform.ime.android_text, cursor);
                crate::util::android_ime::show();
                return;
            }
            state.platform.ime.android_text = current.text;
            state.platform.ime.android_synced = true;
            return;
        }
        let current = crate::util::android_ime::state();
        if current.text == state.platform.ime.android_text {
            return;
        }
        let (removed, inserted) =
            crate::util::android_ime::edit_of(&state.platform.ime.android_text, &current.text);
        state.platform.ime.android_text = current.text;
        for _ in 0..removed {
            backspace(state, focused);
        }
        if !inserted.is_empty() {
            commit(state, &inserted);
        }
    }

    /// One Backspace, through the screen's own key path.
    fn backspace(state: &mut AppState, focused: ImeTarget) {
        match focused {
            ImeTarget::LanLobbyChat | ImeTarget::LanLobbyName => {
                crate::app::shell_lan::key_press(
                    state,
                    Some(winit::keyboard::KeyCode::Backspace),
                    None,
                );
            }
            ImeTarget::WolLoginField => {
                crate::app::shell_wol::login::key_press(
                    state,
                    Some(winit::keyboard::KeyCode::Backspace),
                    None,
                );
            }
            ImeTarget::InGameChat => crate::app::input::chat::backspace(state),
            ImeTarget::SetupChat => crate::app::input::multiplayer_setup::key_press(
                state,
                Some(winit::keyboard::KeyCode::Backspace),
                None,
            ),
            _ => {}
        }
    }
}

/// One winit `Ime` event.
pub(crate) fn event(state: &mut AppState, event: Ime) {
    match event {
        Ime::Preedit(text, cursor) => {
            let ime = &mut state.platform.ime;
            ime.composition = text.encode_utf16().collect();
            // winit reports the cursor as byte offsets, or none when hidden.
            ime.cursor = cursor
                .and_then(|(start, _)| text.get(..start))
                .map_or(ime.composition.len(), |prefix| {
                    prefix.encode_utf16().count()
                });
        }
        Ime::Commit(text) => {
            state.platform.ime.cancel();
            commit(state, &text);
        }
        Ime::Disabled => state.platform.ime.cancel(),
        Ime::Enabled => {}
    }
    state.platform.window.request_redraw();
}

/// WM_IME_CHAR's consumers: the in-game queue and NewEdit's WM_CHAR insertion,
/// here each screen's own text path.
fn commit(state: &mut AppState, text: &str) {
    match target(state).map(|(target, _)| target) {
        Some(ImeTarget::InGameChat) => crate::app::input::chat::commit_text(state, text),
        Some(ImeTarget::SetupChat) => {
            crate::app::input::multiplayer_setup::key_press(state, None, Some(text))
        }
        Some(ImeTarget::LanLobbyChat) => crate::app::shell_lan::key_press(state, None, Some(text)),
        Some(ImeTarget::LanLobbyName) => crate::app::shell_lan::key_press(state, None, Some(text)),
        Some(ImeTarget::WolLoginField) => {
            crate::app::shell_wol::login::key_press(state, None, Some(text))
        }
        Some(ImeTarget::SetupChat) => {
            crate::app::input::multiplayer_setup::key_press(state, None, Some(text))
        }
        Some(ImeTarget::WolLobbyChat) => {
            crate::app::shell_wol::lobby::key_press(state, None, Some(text))
        }
        None => {}
    }
}
