//! Physical input of the connection dialog 0xEA (proc 0x0064AE50). While it
//! is open the wait loop services no tactical input (0x006497EC), so every
//! pointer event belongs to it.

use crate::app::{App, AppState};
use crate::ui::shell::connection::{ConnectionButton, ConnectionLayout, connection_layout};
use winit::event::MouseButton;

fn layout(state: &AppState) -> Option<ConnectionLayout> {
    let shell = crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(state)?;
    Some(connection_layout(
        shell.width,
        shell.height,
        shell.layout,
        shell.button_size,
    ))
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    let Some(layout) = layout(state) else {
        return;
    };
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    if let Some(page) = state.match_state.network_wait.page_mut() {
        page.hovered = page.control_at(&layout, x, y);
        page.buttons.hovered = page.button_at(&layout, x, y);
    }
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button != MouseButton::Left {
        return;
    }
    let Some(layout) = layout(state) else {
        return;
    };
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    let Some(page) = state.match_state.network_wait.page_mut() else {
        return;
    };
    page.hovered = page.control_at(&layout, x, y);
    let target = page.button_at(&layout, x, y);
    if pressed {
        page.buttons.press(target);
        if target.is_some() {
            // Owner-draw buttons click at button-down (0x0061374B..0x00613771).
            App::play_skirmish_shell_generic_click_sound(state);
        }
        return;
    }
    match page.buttons.release(target) {
        Some(ConnectionButton::Quit) => {
            crate::app::match_runtime::network_match::request_leave(state)
        }
        Some(ConnectionButton::Player(row)) => {
            crate::app::match_runtime::network_match::propose_kick(state, row)
        }
        None => {}
    }
}
