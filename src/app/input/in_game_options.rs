//! The init of gamemd's Game Controls (BBB, 4E1FE0 message 0x497) for the
//! in-game menu's Game Controls page (`app::input::pause_menu`), which shows
//! BBB's settings: which way the Game Speed slider runs and whether its row
//! shows.

use crate::app::AppState;

/// Every opening of the menu's page runs BBB's init: the GameSpeed row hides
/// as [`game_speed_hidden`] decides, and under sequenced lockstep, where the
/// relay owns the pace.
pub(crate) fn on_open(state: &mut AppState) {
    let game_speed_hidden = state
        .match_state
        .live_network
        .as_ref()
        .is_some_and(|live| live.uses_sequenced_lockstep())
        || game_speed_hidden(
            state.match_state.session_mode(),
            state
                .match_state
                .sim_runtime
                .runtime()
                .is_some_and(|runtime| runtime.simulation.local_player_defeated()),
            state.frontend.campaign_speed_control,
        );
    let speed_context = if matches!(
        state.match_state.session_mode(),
        crate::app::match_runtime::session::SessionMode::Lan
            | crate::app::match_runtime::session::SessionMode::Wol
    ) {
        crate::ui::shell::game_speed::SpeedContext::Network
    } else {
        crate::ui::shell::game_speed::SpeedContext::Offline
    };
    let options = &mut state.match_state.match_presentation.in_game_options;
    options.speed_context = speed_context;
    options.game_speed_hidden = game_speed_hidden;
}

/// BBB init hides the GameSpeed row in GameMode 0 without `-SPEEDCONTROL`
/// ([0xA8EDDC]), or with Session+0x300, which MPlayer_Defeated (4FC205) sets
/// only for the local House.
///
/// Options init4E2036..4E2079 reads the retained process flag. Ordinary
/// campaign startup installs Main_Tick's speed2 unless that flag is set.
fn game_speed_hidden(
    mode: crate::app::match_runtime::session::SessionMode,
    local_defeated: bool,
    speed_control: bool,
) -> bool {
    (mode == crate::app::match_runtime::session::SessionMode::Campaign && !speed_control)
        || local_defeated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamespeed_row_hides_in_campaign_and_after_local_defeat() {
        use crate::app::match_runtime::session::SessionMode;
        assert!(game_speed_hidden(SessionMode::Campaign, false, false));
        assert!(!game_speed_hidden(SessionMode::Campaign, false, true));
        assert!(game_speed_hidden(SessionMode::Campaign, true, true));
        for mode in [SessionMode::Skirmish, SessionMode::Lan, SessionMode::Wol] {
            assert!(!game_speed_hidden(mode, false, false), "{mode:?}");
            assert!(game_speed_hidden(mode, true, true), "{mode:?}");
        }
    }
}
