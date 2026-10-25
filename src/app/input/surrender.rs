//! The WOL tournament Surrender box: State_Machine state 2
//! (`0x0048CB3A..0x0048CC2A`, jump table `0x0048CC84` entry 2), which B5's
//! Abort requests in a tournament game (`0x004F16C0..0x004F16EA`).
//! `0x005C60D0` runs ShellMessageBox `0x005D3490` with `TXT_SURRENDER` and
//! `TXT_OK`/`TXT_CANCEL` on controls `0x5AE` and 2 (`0x005D35AE..0x005D35CA`);
//! only OK (result 0) surrenders. The box's callback `0x005D36A0` answers
//! controls 1 and 2 with result 1, so Cancel, Enter and Escape cancel.
//!
//! RESIDUAL: the box is drawn over the running battlefield like the
//! in-match `0xCF`; what native shows behind it after B5 closes is not
//! captured.

use crate::app::{App, AppState};
use crate::ui::pause_menu::{InGameMenuState, ModalOutcome};
use crate::ui::shell::modal::{QuitConfirmLayout, quit_confirm_layout};
use winit::event::MouseButton;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurrenderButton {
    /// `0x5AE`.
    Ok,
    /// Control 2 (IDCANCEL).
    Cancel,
}

/// State 2's arm. An observer (`[0xAC10C8]`) leaves at once through
/// Queue_Exit (`0x0048CB3A..0x0048CB47`); a House that is defeated, flagged
/// to die, has won or has lost (`+0x1F5..+0x1F8`) returns to state 0;
/// otherwise the box opens.
pub(crate) fn open(state: &mut AppState) {
    if state.frontend.multiplayer_shell_runtime.lan_observer() {
        App::apply_in_game_modal_outcome(state, ModalOutcome::LeaveMatch { destruct: false });
        return;
    }
    let finished = state
        .match_state
        .sim_runtime
        .as_ref()
        .and_then(|runtime| {
            let sim = &runtime.simulation;
            let house = sim.houses().get(&sim.session().current_house()?)?;
            Some(house.is_defeated() || house.is_to_die() || house.has_won() || house.has_lost())
        })
        .unwrap_or(true);
    App::enter_in_game_menu_state(
        state,
        if finished {
            InGameMenuState::Closed
        } else {
            InGameMenuState::Surrender
        },
    );
}

/// `0x120` centred over the frame the battlefield draws into.
pub(crate) fn layout(state: &AppState) -> QuitConfirmLayout {
    quit_confirm_layout(state.render_width() as i32, state.render_height() as i32)
}

fn hit(state: &AppState) -> Option<SurrenderButton> {
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    [
        (layout.ok, SurrenderButton::Ok),
        (layout.cancel, SurrenderButton::Cancel),
    ]
    .into_iter()
    .find_map(|(rect, button)| rect.contains(x, y).then_some(button))
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    state
        .match_state
        .match_presentation
        .surrender_buttons
        .hovered = hit(state);
    state.platform.window.request_redraw();
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button != MouseButton::Left {
        return;
    }
    let over = hit(state);
    if pressed {
        state
            .match_state
            .match_presentation
            .surrender_buttons
            .press(over);
        if over.is_some() {
            // Owner-draw button-down: GUIMainButtonSound (0x0061376B).
            App::play_main_menu_button_sound(state);
        }
    } else if let Some(button) = state
        .match_state
        .match_presentation
        .surrender_buttons
        .release(over)
    {
        activate(state, button);
    }
    state.platform.window.request_redraw();
}

/// Enter (IDOK) and Escape (IDCANCEL) both give result 1.
pub(crate) fn key(state: &mut AppState) {
    activate(state, SurrenderButton::Cancel);
}

/// OK queues EVENT_DESTRUCT for the player and Queue_Exit, clearing the
/// latch (`0x0048CBAE..0x0048CC23`); anything else returns to state 0.
fn activate(state: &mut AppState, button: SurrenderButton) {
    App::apply_in_game_modal_outcome(
        state,
        match button {
            SurrenderButton::Ok => ModalOutcome::LeaveMatch { destruct: true },
            SurrenderButton::Cancel => ModalOutcome::Enter(InGameMenuState::Closed),
        },
    );
}
