//! BC6 (`0xBC6`) window input adapter. The dialog produces intents;
//! `app::cooperative_round` and `MatchSession` decide their effects.

use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::{App, AppState};
use crate::ui::coop_score_shell::*;
use crate::ui::shell::page::PageEvent;
use crate::util::time::Instant;
use winit::event::{KeyEvent, MouseButton};
use winit::keyboard::{KeyCode, PhysicalKey};

pub(crate) fn layout(state: &AppState) -> CoopScoreLayout {
    let (w, h) = shell_layout::shell_screen_size(state);
    CoopScoreLayout::new(w as i32, h as i32, shell_layout::fills_window(state))
}

/// BC6 owns the result screen while its page is open.
pub(crate) fn active(state: &AppState) -> bool {
    matches!(
        state.frontend.screen,
        crate::app::GameScreen::MissionResult { .. }
    ) && state.frontend.coop_score_page.is_some()
}

/// BC6's page is the active dialog, its nodes on `layout`: the chat edit
/// holds the focus throughout (BC6 never moves it), the transcript's list
/// follows new lines, and NewEdit keeps the chat's caret in view.
fn sync(state: &mut AppState, layout: &CoopScoreLayout) {
    let enabled = [CONTINUE, QUIT].map(|id| (id, enabled(state, id)));
    let rows = state
        .frontend
        .coop_score_page
        .as_ref()
        .map_or(0, |page| page.ui.chat.lines.len());
    let font = &state.renderer.bit_font;
    let controller = &mut state.frontend.shell_controller;
    controller.ensure_active(COOP_SCORE_DIALOG);
    controller.set_scale_in(COOP_SCORE_DIALOG, layout.page.scale);
    let rect_of = |id| layout.control_rect(id);
    crate::ui::shell::menu_page::sync_nodes(
        controller,
        COOP_SCORE_DIALOG,
        nodes,
        rect_of,
        &enabled,
    );
    if controller.focused_control(COOP_SCORE_DIALOG).is_none() {
        controller.focus_control(COOP_SCORE_DIALOG, Some(CHAT));
    }
    if let Some(log) = controller.control(COOP_SCORE_DIALOG, CHAT_LOG) {
        let _ = controller.gui_mut().log_rows(log, rows, 0);
    }
    if let (Some(chat), Some(_)) = (
        controller.control(COOP_SCORE_DIALOG, CHAT),
        layout.control_rect(CHAT),
    ) {
        let _ = controller
            .gui_mut()
            .keep_caret_in_view(chat, &|text| font.text_width(text));
    }
}

pub(crate) fn update_geometry(state: &mut AppState) {
    if active(state) {
        let layout = layout(state);
        sync(state, &layout);
    }
}

/// Whether control `id` takes input now (`0x0046FB50`, `0x0046DE20`).
fn enabled(state: &AppState, id: u16) -> bool {
    let (Some(view), Some(page)) = (
        crate::app::cooperative_round::view(state),
        state.frontend.coop_score_page.as_ref(),
    ) else {
        return false;
    };
    if !page.model.can_continue && hidden_on_final_mission(id) {
        return false;
    }
    match id {
        CONTINUE => view.continue_enabled,
        QUIT => view.quit_enabled,
        CHAT | CHAT_LOG => true,
        _ => false,
    }
}

pub(crate) fn key(state: &mut AppState, event: &KeyEvent) {
    if !event.state.is_pressed() || shell_transition::blocks_shell_input(state) {
        return;
    }
    let code = if let PhysicalKey::Code(code) = event.physical_key {
        Some(code)
    } else {
        None
    };
    key_press(state, code, event.text.as_deref());
}

pub(crate) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    if state.frontend.coop_score_page.is_none() {
        return;
    }
    let layout = layout(state);
    sync(state, &layout);
    // The chat edit has the focus and takes every key; Enter submits.
    let controller = &mut state.frontend.shell_controller;
    let sent = crate::ui::lan_shell::chat_key(controller, COOP_SCORE_DIALOG, code, text);
    if let Some(action) = sent.map(CoopScoreAction::Chat) {
        crate::app::cooperative_round::apply_action(state, action);
    }
}

/// One pointer message (`down`: `None` moves) on the page's nodes: hover
/// writes the status help, a press on an enabled Continue or Quit plays
/// GUIMainButtonSound, one on the chat edit keeps its focus and one on the
/// log's scrollbar scrolls it (its rows take no click sound). Returns the
/// activated button.
fn pointer(state: &mut AppState, layout: &CoopScoreLayout, down: Option<bool>) -> Option<u16> {
    sync(state, layout);
    let (x, y) = state.match_state.input.cursor_px();
    let event = crate::app::input::dialog_page::event_at(x, y, down);
    state.frontend.shell_controller.on_input(event);
    let events = App::shell_page_events(state, false);
    events.into_iter().rev().find_map(|event| match event {
        PageEvent::Activated(id) => Some(id),
        _ => None,
    })
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    let layout = layout(state);
    pointer(state, &layout, None);
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button != MouseButton::Left || shell_transition::blocks_shell_input(state) {
        return;
    }
    if crate::app::cooperative_round::view(state).is_none()
        || state.frontend.coop_score_page.is_none()
    {
        return;
    }
    let layout = layout(state);
    let controller = &state.frontend.shell_controller;
    let over = crate::ui::shell::menu_page::hovered_id(controller, COOP_SCORE_DIALOG)
        .filter(|id| enabled(state, *id));
    let action = match pointer(state, &layout, Some(pressed)) {
        Some(CONTINUE) => Some(CoopScoreAction::Continue),
        Some(QUIT) => Some(CoopScoreAction::Quit),
        _ => None,
    };
    // A click over a control that takes no input leaves no status help
    // until the pointer moves again.
    if over.is_none() {
        state.frontend.shell_controller.gui_mut().cancel_pointer();
    }
    if let Some(action) = action {
        crate::app::cooperative_round::apply_action(state, action);
    }
}

/// The chat list's scrollbar repeat timer.
pub(crate) fn poll(state: &mut AppState) -> Option<Instant> {
    let window = &state.platform.window;
    state
        .frontend
        .shell_controller
        .poll_then(|| window.request_redraw())
}
