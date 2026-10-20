//! B6 physical input. Shared capture owns the pointer; app modal outcomes
//! retain pause/clock, queued EXIT (native48CAA8→6471A0), DESTRUCT
//! (48CA34/48CAB2) and the campaign restart request (native48CB2E→8B41C0)
//! authority.

use crate::app::match_runtime::session::SessionMode;
use crate::app::{App, AppState};
use crate::ui::pause_menu::{AbortConfirmAction, resolve_abort_action};
use crate::ui::shell::abort::{AbortButton, AbortLayout, AbortSecondary, abort_layout};
use winit::event::MouseButton;

/// B6 init `0x004F1938..0x004F1A0E` keys control712 on GameMode: 0 shows
/// Restart, 5 hides it, LAN/WOL show Observe with two or more live humans
/// while the local House is not defeated. A campaign VERA cannot restart (a
/// restored saved game, which carries no campaign start) hides Restart
/// instead of offering a press that would end the game.
fn select_abort_secondary(state: &AppState) -> AbortSecondary {
    secondary_for(
        state.match_state.session_mode(),
        state.match_state.startup.installed_campaign().is_some(),
        state
            .match_state
            .sim_runtime
            .as_ref()
            .map(|runtime| &runtime.simulation),
    )
}

fn secondary_for(
    mode: SessionMode,
    campaign_installed: bool,
    sim: Option<&crate::sim::world::Simulation>,
) -> AbortSecondary {
    let live_humans = sim.map_or(0, |sim| {
        sim.houses()
            .values()
            .filter(|house| house.is_human() && !house.is_defeated())
            .count()
    });
    let local_defeated = sim.is_some_and(|sim| sim.local_player_defeated());
    AbortSecondary::for_mode(
        (mode == SessionMode::Campaign).then_some(campaign_installed),
        mode.is_network(),
        live_humans,
        local_defeated,
    )
}

/// B6 init runs once per dialog: its stored results and control712 stay
/// fixed until the dialog closes, whatever the match does meanwhile.
pub(crate) fn on_open(state: &mut AppState) {
    state.match_state.match_presentation.abort_secondary = select_abort_secondary(state);
}

/// The open B6's geometry.
pub(crate) fn current_abort_layout(state: &AppState) -> Option<AbortLayout> {
    let shell = crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(state)?;
    Some(abort_layout(
        shell.width,
        shell.height,
        shell.layout,
        shell.button_size,
        state.match_state.match_presentation.abort_secondary,
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

/// Callback4F18B0 writes the result B6 init stored for the pressed button:
/// Leave6C9 `[0xA8EE7C]` (2, or 3 beside Observe), 712 `[0xA8EE80]`.
pub(crate) fn activate(state: &mut AppState, button: AbortButton) {
    let Some(layout) = current_abort_layout(state) else {
        return;
    };
    let action = match (button, layout.secondary_kind) {
        (AbortButton::Leave, AbortSecondary::Observe) => AbortConfirmAction::LeaveDestruct,
        (AbortButton::Leave, _) => AbortConfirmAction::Leave,
        (AbortButton::Secondary, AbortSecondary::Restart) => AbortConfirmAction::Restart,
        (AbortButton::Secondary, AbortSecondary::Observe) => AbortConfirmAction::Observe,
        (AbortButton::Secondary, AbortSecondary::Hidden) => return,
        (AbortButton::Resume, _) => AbortConfirmAction::Cancel,
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
    use crate::sim::house_state::HouseState;
    use crate::sim::world::Simulation;

    #[test]
    fn observe_counts_undefeated_humans_and_the_local_house() {
        let mut sim = Simulation::new();
        let names = ["Local", "Peer", "Ai"];
        let ids = names.map(|name| sim.intern(name));
        for (index, id) in ids.iter().enumerate() {
            sim.houses_mut_for_test().register(HouseState::new(
                *id,
                index as i32,
                None,
                index < 2,
                0,
                10,
            ));
        }
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(ids[0]));
        let secondary = |sim: &Simulation, mode| secondary_for(mode, false, Some(sim));
        assert_eq!(secondary(&sim, SessionMode::Lan), AbortSecondary::Observe);
        assert_eq!(secondary(&sim, SessionMode::Wol), AbortSecondary::Observe);
        assert_eq!(
            secondary(&sim, SessionMode::Skirmish),
            AbortSecondary::Hidden
        );
        assert_eq!(
            secondary_for(SessionMode::Campaign, true, Some(&sim)),
            AbortSecondary::Restart
        );
        // A defeated peer leaves one live human: the AI House never counts.
        sim.houses_mut_for_test()
            .get_mut(&ids[1])
            .unwrap()
            .set_is_defeated_for_test(true);
        assert_eq!(secondary(&sim, SessionMode::Lan), AbortSecondary::Hidden);
    }
}
