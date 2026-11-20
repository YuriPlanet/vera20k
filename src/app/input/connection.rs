//! Physical input of the connection dialog 0xEA (proc 0x0064AE50). While it
//! is open the wait loop services no tactical input (0x006497EC), so every
//! pointer event belongs to it.

use crate::app::AppState;
use crate::ui::shell::connection::{self, ConnectionButton, ConnectionLayout, connection_layout};
use crate::ui::shell::page::PageEvent;
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

/// One pointer message on 0xEA (`down`: `None` moves).
fn pointer(state: &mut AppState, down: Option<bool>) {
    let Some(layout) = layout(state) else {
        return;
    };
    let event = super::dialog_page::shell_event(state, down);
    let presentation = &mut state.match_state.match_presentation;
    let Some(page) = state.match_state.network_wait.page_mut() else {
        return;
    };
    let ui = &mut presentation.connection_ui;
    if !std::mem::replace(&mut page.controls_built, true) {
        *ui = Default::default();
    }
    let events = super::dialog_page::route(
        ui,
        connection::DIALOG,
        connection::spec,
        |id| page.rect(&layout, id),
        super::dialog_page::no_sync,
        event,
    );
    // Owner-draw buttons play GUIMainButtonSound at button-down
    // (0x0061374B..0x00613771).
    for event in super::dialog_page::play_cues(state, events) {
        let PageEvent::Activated(id) = event else {
            continue;
        };
        let Some(button) = state
            .match_state
            .network_wait
            .page()
            .and_then(|page| page.button(id))
        else {
            continue;
        };
        match button {
            ConnectionButton::Quit => {
                crate::app::match_runtime::network_match::request_leave(state)
            }
            ConnectionButton::Player(row) => {
                crate::app::match_runtime::network_match::propose_kick(state, row)
            }
            ConnectionButton::PreviousPage | ConnectionButton::NextPage => {
                if let Some(page) = state.match_state.network_wait.page_mut()
                    && page.change_page(button == ConnectionButton::NextPage)
                {
                    let ui = &mut state.match_state.match_presentation.connection_ui;
                    ui.restart_instance(connection::DIALOG);
                }
                state.platform.window.request_redraw();
            }
        }
    }
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    pointer(state, None);
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button == MouseButton::Left {
        pointer(state, Some(pressed));
    }
}
