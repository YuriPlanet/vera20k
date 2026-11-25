//! The init of gamemd's Game Controls (BBB, 4E1FE0 message 0x497) for the
//! in-game menu's Game Controls page (`app::input::pause_menu`), which shows
//! BBB's settings: which way the Game Speed slider runs, whether its row
//! shows and the speed it stands at.

use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::session::SessionMode;
use crate::app::match_runtime::state::MatchState;
use crate::sim::world::Simulation;
use crate::ui::shell::game_speed::SpeedContext;
use crate::ui::shell::in_game_options_state::InGameOptionsState;

/// Every opening of the menu's page runs BBB's init ([`init`]).
pub(crate) fn on_open(frontend: &FrontendState, match_state: &mut MatchState) {
    let sequenced_lockstep = match_state
        .live_network
        .as_ref()
        .is_some_and(|live| live.uses_sequenced_lockstep());
    let mode = match_state.session_mode();
    let world = match_state
        .sim_runtime
        .runtime()
        .map(|runtime| &runtime.simulation);
    init(
        &mut match_state.match_presentation.in_game_options,
        world,
        mode,
        sequenced_lockstep,
        frontend.campaign_speed_control,
    );
}

/// BBB's init: the GameSpeed row hides as [`game_speed_hidden`] decides, and
/// under sequenced lockstep, where the relay owns the pace. The slider stands
/// at the speed in effect: BBB reads `[0xA8EB60]` (`0x004E209E`), which every
/// player's GAMESPEED event writes (`0x004C794E`), so here the world's speed.
fn init(
    options: &mut InGameOptionsState,
    world: Option<&Simulation>,
    mode: SessionMode,
    sequenced_lockstep: bool,
    speed_control: bool,
) {
    options.game_speed_hidden = sequenced_lockstep
        || game_speed_hidden(
            mode,
            world.is_some_and(|world| world.local_player_defeated()),
            speed_control,
        );
    options.speed_context = if matches!(mode, SessionMode::Lan | SessionMode::Wol) {
        SpeedContext::Network
    } else {
        SpeedContext::Offline
    };
    crate::app::loading::transitions::mirror_world_speed(options, world);
}

/// BBB init hides the GameSpeed row in GameMode 0 without `-SPEEDCONTROL`
/// ([0xA8EDDC]), or with Session+0x300, which MPlayer_Defeated (4FC205) sets
/// only for the local House.
///
/// Options init4E2036..4E2079 reads the retained process flag. Ordinary
/// campaign startup installs Main_Tick's speed2 unless that flag is set.
fn game_speed_hidden(mode: SessionMode, local_defeated: bool, speed_control: bool) -> bool {
    (mode == SessionMode::Campaign && !speed_control) || local_defeated
}

#[cfg(all(test, tests_app_input_d_z))]
mod tests {
    use super::*;

    #[test]
    fn gamespeed_row_hides_in_campaign_and_after_local_defeat() {
        assert!(game_speed_hidden(SessionMode::Campaign, false, false));
        assert!(!game_speed_hidden(SessionMode::Campaign, false, true));
        assert!(game_speed_hidden(SessionMode::Campaign, true, true));
        for mode in [SessionMode::Skirmish, SessionMode::Lan, SessionMode::Wol] {
            assert!(!game_speed_hidden(mode, false, false), "{mode:?}");
            assert!(game_speed_hidden(mode, true, true), "{mode:?}");
        }
    }

    /// Another player's speed event changes the world, not the app's copy:
    /// the slider opens at the speed it set, so the player can choose the
    /// one before it again.
    #[test]
    fn game_speed_opens_at_the_speed_another_player_set() {
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::house_state::HouseState;
        let mut world = Simulation::new();
        let local = world.intern("Local");
        let other = world.intern("Other");
        for (side, house) in [local, other].into_iter().enumerate() {
            world.houses_mut_for_test().register(HouseState::new(
                house,
                side as i32,
                None,
                true,
                0,
                10,
            ));
        }
        world
            .session_mut_for_test()
            .set_current_house_for_test(Some(local));
        world.session_mut_for_test().set_game_mode(SessionMode::Lan);
        world
            .session_mut_for_test()
            .game_options_mut_for_test()
            .game_speed = 4;
        // Installing the world copied its speed.
        let mut options = InGameOptionsState::default();
        crate::app::loading::transitions::mirror_world_speed(&mut options, Some(&world));

        world.queue_command(CommandEnvelope::new(
            other,
            1,
            Command::LiveGameSpeed { speed: 1 },
        ));
        let due = world.take_due_commands();
        world.advance_tick(&due, None, None, None, 33);
        assert_eq!(world.session().game_options().game_speed, 1);
        init(&mut options, Some(&world), SessionMode::Lan, false, false);
        assert_eq!(options.game_speed_position(), 5, "Faster, which Other set");
    }
}
