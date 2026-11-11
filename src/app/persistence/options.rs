//! Process-profile projection and the in-game apply of its values.
//!
//! `RetailOptionsProfile` is the only persisted authority. The dialog state,
//! window, render gates, and audio players are process-local projections of it.

use crate::app::AppState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::presentation::target_lines::TargetLineState;
use crate::ui::shell::in_game_options_state::{
    InGameOptionsState, OPTIONS_DETAIL_MAX, OPTIONS_SPEED_MAX,
};
use crate::ui::tooltips::TooltipService;

pub(crate) mod audio;
pub(crate) mod launcher;

fn project_speed_control(value: i32) -> u32 {
    value.clamp(0, OPTIONS_SPEED_MAX as i32) as u32
}

/// Build the bounded dialog/live-consumer projection from the retained signed
/// profile fields. The profile keeps hand-edited out-of-range values until an
/// accepted bounded dialog selection replaces them.
pub(crate) fn in_game_options_from_profile(profile: &RetailOptionsProfile) -> InGameOptionsState {
    InGameOptionsState {
        game_speed: u32::from(crate::sim::game_options::clamp_game_speed(
            profile.game_speed,
        )),
        scroll_rate: project_speed_control(profile.scroll_rate),
        detail_level: profile.detail_level.clamp(0, OPTIONS_DETAIL_MAX as i32) as u32,
        unit_action_lines: profile.unit_action_lines,
        show_hidden: profile.show_hidden,
        tooltips: profile.tooltips,
        ..Default::default()
    }
}

fn apply_target_lines(target_lines: &mut TargetLineState, options: &InGameOptionsState) {
    target_lines.set_unit_action_lines_enabled(options.unit_action_lines);
}

fn in_game_speed_command(
    sim: &crate::sim::world::Simulation,
    speed: u8,
) -> Option<crate::sim::command::Command> {
    // ApplyFromInGameDialog4E1E1B..4E1E22 compares the live speed before
    // choosing a route. Pending events do not change this producer's gate.
    if sim.session().game_options().game_speed == i32::from(speed) {
        return None;
    }
    // ApplyFromInGameDialog4E1E28..4E1E59 queues GAMESPEED in active
    // modes 1..4; campaign/skirmish store it locally.
    Some(
        if sim.session().game_mode().command_queue_route()
            == crate::sim::world::CommandQueueRoute::Multiplayer
        {
            crate::sim::command::Command::LiveGameSpeed { speed }
        } else {
            crate::sim::command::Command::SetGameSpeed { speed }
        },
    )
}

/// Project the two boot/live boolean gates with existing presentation owners.
///
/// Retail provenance: the tail of `OptionsClass__ReadFromINI @ 0x005FA620`
/// synchronizes the action-line option, and
/// `OptionsClass__ApplyFromInGameDialog @ 0x004E1DE0` reapplies accepted
/// controls at the dialog boundary. Tooltip enablement is the Rust-native
/// consumer of the same retained `ToolTips` field.
pub(crate) fn apply_presentation_option_gates(
    target_lines: &mut TargetLineState,
    tooltips: &mut TooltipService,
    options: &InGameOptionsState,
) {
    apply_target_lines(target_lines, options);
    tooltips.set_enabled(options.tooltips);
}

/// The GameSpeed part of `OptionsClass__ApplyFromInGameDialog @ 0x004E1DE0`:
/// the accepted `in_game_options.game_speed` reaches the frame pacer and,
/// in a match, the local house's speed event. The in-game menu's Game
/// Controls page applies its slider through this too.
pub(crate) fn apply_in_game_speed(state: &mut AppState) {
    state.match_state.sim_speed_tps = crate::app::types::tps_for_game_speed(
        state
            .match_state
            .match_presentation
            .in_game_options
            .game_speed,
    );
    if state.match_state.sim_runtime.is_installed() {
        let owner = crate::app::input::commands::preferred_local_owner_name(state);
        let speed = u8::try_from(
            state
                .match_state
                .match_presentation
                .in_game_options
                .game_speed,
        )
        .ok();
        let command = speed.and_then(|speed| {
            in_game_speed_command(
                &state.match_state.sim_runtime.runtime().unwrap().simulation,
                speed,
            )
        });
        if let Some(command) = command
            && owner
                .as_deref()
                .and_then(|owner| {
                    crate::app::input::commands::try_schedule_command(state, owner, command)
                })
                .is_none()
        {
            log::warn!(
                "In-game Options could not queue GameSpeed={} for the local house",
                state
                    .match_state
                    .match_presentation
                    .in_game_options
                    .game_speed
            );
        }
        // Native network Options retains the old speed when its OutList
        // insertion fails. Reflect only the accepted pending/current value.
        crate::app::loading::transitions::sync_in_game_options_speed_from_sim(state);
    }
}

/// Commit the retained complete profile. A settings error is non-fatal for both
/// dialog close and quit.
pub(crate) fn persist_options_profile(state: &AppState) {
    let Some(config) = state.platform.game_config.as_ref() else {
        return;
    };
    if let Err(error) = state
        .persistence
        .options_profile
        .commit_settings(config.paths.settings_dir())
    {
        log::warn!("Failed to persist Options profile to VERA.INI: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::input::commands::schedule_command_in_sim;
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::house_state::HouseState;
    use crate::sim::scenario_session::ScenarioGameMode;
    use crate::sim::world::Simulation;

    fn network_speed_fixture() -> Simulation {
        let mut sim = Simulation::new();
        let owner = sim.intern("Local");
        sim.houses_mut_for_test()
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        sim.session_mut_for_test()
            .set_game_mode(ScenarioGameMode::Wol);
        sim.session_mut_for_test()
            .game_options_mut_for_test()
            .game_speed = 3;
        sim
    }

    #[test]
    fn unchanged_network_options_preserves_the_last_event_slot() {
        let mut sim = network_speed_fixture();
        for _ in 0..127 {
            schedule_command_in_sim(&mut sim, "Local", Command::Stop { entity_id: 42 });
        }
        if let Some(command) = in_game_speed_command(&sim, 3) {
            schedule_command_in_sim(&mut sim, "Local", command);
        }
        schedule_command_in_sim(&mut sim, "Local", Command::LiveGameSpeed { speed: 6 });
        let due = sim.take_due_commands();
        sim.advance_tick(&due, None, None, None, 33);
        assert_eq!(
            sim.session().game_options().game_speed,
            6,
            "unchanged Options must not consume the slot needed by the next event"
        );
    }

    #[test]
    fn options_compares_live_speed_and_does_not_deduplicate_pending_events() {
        let mut sim = network_speed_fixture();
        let owner = sim.interner().get("Local").unwrap();
        sim.queue_command(CommandEnvelope::new(
            owner,
            0,
            Command::LiveGameSpeed { speed: 6 },
        ));
        assert_eq!(in_game_speed_command(&sim, 3), None);
        assert_eq!(
            in_game_speed_command(&sim, 6),
            Some(Command::LiveGameSpeed { speed: 6 })
        );
    }

    #[test]
    fn profile_projection_is_bounded_without_mutating_profile_authority() {
        let profile = RetailOptionsProfile {
            game_speed: -7,
            scroll_rate: 99,
            detail_level: 1,
            unit_action_lines: false,
            show_hidden: true,
            tooltips: false,
            ..Default::default()
        };

        let projected = in_game_options_from_profile(&profile);
        assert_eq!(projected.game_speed, 0);
        assert_eq!(projected.scroll_rate, OPTIONS_SPEED_MAX);
        assert_eq!(projected.detail_level, 1);
        assert!(!projected.unit_action_lines);
        assert!(projected.show_hidden);
        assert!(!projected.tooltips);
        assert_eq!(profile.game_speed, -7);
        assert_eq!(profile.scroll_rate, 99);
    }

    #[test]
    fn boot_and_live_presentation_gates_follow_the_profile_projection() {
        let mut target_lines = TargetLineState::default();
        let mut tooltips = TooltipService::new();
        assert!(tooltips.register(crate::ui::tooltips::TipRegion {
            id: 7,
            rect: crate::ui::tooltips::TipRect::new(0, 0, 10, 10),
            text: "profile tip".to_string(),
        }));

        let disabled = InGameOptionsState {
            unit_action_lines: false,
            tooltips: false,
            ..Default::default()
        };
        apply_presentation_option_gates(&mut target_lines, &mut tooltips, &disabled);
        assert!(!target_lines.unit_action_lines_enabled());
        tooltips.on_mouse_move(5, 5, 0);
        tooltips.poll(crate::ui::tooltips::TOOLTIP_DELAY_MS);
        assert!(
            tooltips.active().is_none(),
            "disabled profile gate must win at boot"
        );

        let enabled = InGameOptionsState::default();
        apply_presentation_option_gates(&mut target_lines, &mut tooltips, &enabled);
        assert!(target_lines.unit_action_lines_enabled());
        tooltips.on_mouse_move(5, 5, 2_000);
        tooltips.poll(2_000 + crate::ui::tooltips::TOOLTIP_DELAY_MS);
        assert_eq!(tooltips.active().map(|tip| tip.id), Some(7));
    }
}
