//! Physical input of the connection dialog 0xEA (proc 0x0064AE50). While it
//! is open the wait loop services no tactical input (0x006497EC), so every
//! pointer event belongs to it.

use super::dialog_page::InGameDialog;
use crate::app::AppState;
use crate::ui::shell::connection::{self, ConnectionButton, ConnectionLayout, connection_layout};
use crate::ui::shell::page::PageEvent;
use winit::event::MouseButton;

/// 0xEA: no title, Quit Game as the lower right-panel button, on its own
/// controller.
pub(crate) const DIALOG: InGameDialog = InGameDialog {
    id: connection::DIALOG,
    title: None,
    lower: (0x2, connection::QUIT_LABEL),
    label: "Active Connection Shell EA",
    ui: |state| Some(&state.match_state.match_presentation.connection_ui),
    ui_mut: |state| Some(&mut state.match_state.match_presentation.connection_ui),
    own_checkbox_sound: false,
    help_under_held: false,
    help_fallback: |_| "",
};

/// Whether 0xEA is open; its controls start afresh on a new dialog.
fn open_controls(state: &mut AppState) -> bool {
    let Some(page) = state.match_state.network_wait.page_mut() else {
        return false;
    };
    if !std::mem::replace(&mut page.controls_built, true) {
        state.match_state.match_presentation.connection_ui = Default::default();
    }
    true
}

/// Makes 0xEA's nodes current without input (rects, the bars, the list):
/// its renderer's call every drawn frame before it paints from them.
pub(crate) fn sync(state: &mut AppState) -> Option<ConnectionLayout> {
    let layout = super::dialog_page::layout(state, connection_layout)?;
    if !open_controls(state) {
        return None;
    }
    let match_state = &mut state.match_state;
    let wait = &match_state.network_wait;
    let page = wait.page()?;
    let ui = &mut match_state.match_presentation.connection_ui;
    super::dialog_page::sync_dialog(
        ui,
        connection::DIALOG,
        connection::spec,
        |id| page.rect(&layout, id),
        |gui, root| connection::sync_nodes(gui, root, page, |source| wait.bar_timing(source)),
    );
    Some(layout)
}

/// One pointer message on 0xEA (`down`: `None` moves).
fn pointer(state: &mut AppState, down: Option<bool>) {
    let Some(layout) = super::dialog_page::layout(state, connection_layout) else {
        return;
    };
    let event = super::dialog_page::shell_event(state, down);
    if !open_controls(state) {
        return;
    }
    let match_state = &mut state.match_state;
    let wait = &match_state.network_wait;
    let Some(page) = wait.page() else {
        return;
    };
    let ui = &mut match_state.match_presentation.connection_ui;
    let events = super::dialog_page::route(
        ui,
        connection::DIALOG,
        connection::spec,
        |id| page.rect(&layout, id),
        |gui, root| connection::sync_nodes(gui, root, page, |source| wait.bar_timing(source)),
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
