//! BC6 (`0xBC6`) window input adapter. The dialog produces intents;
//! `app::cooperative_round` and `MatchSession` decide their effects.

use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::{App, AppState};
use crate::ui::coop_score_shell::*;
use crate::ui::lan_shell::LanEditKey;
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

pub(crate) fn update_geometry(state: &mut AppState) {
    let layout = layout(state);
    let (Some(chat_log), Some(chat)) = (layout.control_rect(CHAT_LOG), layout.control_rect(CHAT))
    else {
        return;
    };
    let Some(page) = state.frontend.coop_score_page.as_mut() else {
        return;
    };
    let ui = &mut page.ui;
    ui.chat
        .list
        .reconcile_geometry(chat_log, ui.chat.lines.len(), layout.page.scale);
    ui.chat.edit.scroll_to_caret(
        (layout.page.scale.base_rect(chat).w - 3).max(0) as u32,
        |text| state.renderer.bit_font.text_width(text),
    );
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
    let Some(page) = state.frontend.coop_score_page.as_mut() else {
        return;
    };
    // The chat edit has the focus and takes every key; Enter submits.
    let key = match code {
        Some(KeyCode::Backspace) => Some(LanEditKey::Backspace),
        Some(KeyCode::Delete) => Some(LanEditKey::Delete),
        Some(KeyCode::ArrowLeft) => Some(LanEditKey::Left),
        Some(KeyCode::ArrowRight) => Some(LanEditKey::Right),
        Some(KeyCode::Home) => Some(LanEditKey::Home),
        Some(KeyCode::End) => Some(LanEditKey::End),
        Some(KeyCode::Enter | KeyCode::NumpadEnter) => Some(LanEditKey::Enter),
        _ => None,
    };
    let action = if let Some(key) = key {
        page.ui.chat.key(key).map(CoopScoreAction::Chat)
    } else {
        if !matches!(code, Some(KeyCode::Escape | KeyCode::Tab))
            && let Some(text) = text
        {
            page.ui.chat.insert_text(text);
        }
        None
    };
    if let Some(action) = action {
        crate::app::cooperative_round::apply_action(state, action);
    }
}

/// One pointer message (`down`: `None` moves) on the page's nodes: hover
/// writes the status help, and a press on an enabled Continue or Quit plays
/// GUIMainButtonSound. Returns the activated button.
fn pointer(state: &mut AppState, layout: &CoopScoreLayout, down: Option<bool>) -> Option<u16> {
    let enabled = [CONTINUE, QUIT].map(|id| (id, enabled(state, id)));
    let controller = &mut state.frontend.shell_controller;
    controller.ensure_active(COOP_SCORE_DIALOG, false);
    let rect_of = |id| layout.control_rect(id);
    App::page_pointer(state, COOP_SCORE_DIALOG, nodes, rect_of, &enabled, down)
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let Some(chat_log) = layout.control_rect(CHAT_LOG) else {
        return;
    };
    pointer(state, &layout, None);
    let Some(page) = state.frontend.coop_score_page.as_mut() else {
        return;
    };
    let ui = &mut page.ui;
    let geometry = ui
        .chat
        .list
        .geometry(chat_log, ui.chat.lines.len(), layout.page.scale);
    ui.chat
        .list
        .scroll
        .pointer_moved(geometry, &mut ui.chat.list.top_index, x, y);
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
    let (x, y) = state.match_state.input.cursor_px();
    let over = layout.control_at(x, y).filter(|id| enabled(state, *id));
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
    let ui = &mut state
        .frontend
        .coop_score_page
        .as_mut()
        .expect("checked above")
        .ui;
    match over.filter(|_| pressed) {
        Some(CHAT) => ui.chat.edit.focused = true,
        Some(CHAT_LOG) => {
            let rect = layout.control_rect(CHAT_LOG).expect("chat list rect");
            let geometry = ui
                .chat
                .list
                .geometry(rect, ui.chat.lines.len(), layout.page.scale);
            if let Some(part) = geometry.scroll_part_at(x, y) {
                ui.chat.list.scroll.press(
                    part,
                    geometry,
                    &mut ui.chat.list.top_index,
                    y,
                    Instant::now(),
                );
            }
        }
        _ if !pressed => ui.chat.list.scroll.cancel(),
        _ => {}
    }
    if let Some(action) = action {
        crate::app::cooperative_round::apply_action(state, action);
    }
}

/// The chat list's scrollbar repeat timer.
pub(crate) fn poll(state: &mut AppState) -> Option<Instant> {
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let rect = layout.control_rect(CHAT_LOG)?;
    let page = state.frontend.coop_score_page.as_mut()?;
    let ui = &mut page.ui;
    let geometry = ui
        .chat
        .list
        .geometry(rect, ui.chat.lines.len(), layout.page.scale);
    if ui
        .chat
        .list
        .scroll
        .poll(geometry, &mut ui.chat.list.top_index, x, y, Instant::now())
    {
        state.platform.window.request_redraw();
    }
    ui.chat.list.scroll.repeat_at()
}
