//! B6 physical input. Shared capture owns the pointer; app modal outcomes
//! retain pause/clock, queued EXIT (native48CAA8→6471A0) and the campaign
//! restart request (native48CB2E→8B41C0) authority.

use crate::app::match_runtime::session::SessionMode;
use crate::app::{App, AppState};
use crate::ui::pause_menu::{AbortConfirmAction, resolve_abort_action};
use crate::ui::shell::abort::{AbortButton, AbortLayout, AbortSecondary, abort_layout};
use winit::event::MouseButton;

/// B6 init `0x004F1938..0x004F1A0E` keys control712 on GameMode: 0 shows
/// Restart, 5 hides it. LAN/WOL show Observe only with two or more live
/// humans; VERA has no Observe yet and hides it there. A campaign VERA cannot
/// restart (a restored saved game, which carries no campaign start) hides
/// Restart instead of offering a press that would end the game.
pub(crate) fn abort_secondary(mode: SessionMode, restartable: bool) -> AbortSecondary {
    match mode {
        SessionMode::Campaign if restartable => AbortSecondary::Restart,
        _ => AbortSecondary::Hidden,
    }
}

/// The live B6 geometry for the running session's mode.
pub(crate) fn current_abort_layout(state: &AppState) -> Option<AbortLayout> {
    let shell = crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(state)?;
    Some(abort_layout(
        shell.width,
        shell.height,
        shell.layout,
        shell.button_size,
        abort_secondary(
            state.match_state.session.mode(),
            state.match_state.startup.installed_campaign().is_some(),
        ),
    ))
}

fn hit(state: &AppState) -> Option<AbortButton> {
    let layout = current_abort_layout(state)?;
    let (x, y) = crate::app::frontend::skirmish_shell_render::in_game_shell_cursor(state);
    layout.hit(x, y)
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    state.match_state.match_presentation.abort_buttons.hovered = hit(state);
}

pub(crate) fn activate(state: &mut AppState, button: AbortButton) {
    let action = match button {
        AbortButton::Leave => AbortConfirmAction::Leave,
        AbortButton::Restart => AbortConfirmAction::Restart,
        AbortButton::Resume => AbortConfirmAction::Cancel,
    };
    App::apply_in_game_modal_outcome(state, resolve_abort_action(action));
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
            .abort_buttons
            .press(over);
        if over.is_some() {
            App::play_skirmish_shell_generic_click_sound(state);
        }
    } else if let Some(button) = state
        .match_state
        .match_presentation
        .abort_buttons
        .release(over)
    {
        activate(state, button);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_campaign_shows_the_restart_action() {
        assert_eq!(
            abort_secondary(SessionMode::Campaign, true),
            AbortSecondary::Restart
        );
        assert_eq!(
            abort_secondary(SessionMode::Campaign, false),
            AbortSecondary::Hidden
        );
        for mode in [SessionMode::Skirmish, SessionMode::Lan, SessionMode::Wol] {
            assert_eq!(
                abort_secondary(mode, true),
                AbortSecondary::Hidden,
                "{mode:?}"
            );
        }
    }
}
