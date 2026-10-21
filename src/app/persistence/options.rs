//! Process-profile projection and the in-game Options apply/commit transaction.
//!
//! `RetailOptionsProfile` is the only persisted authority. The dialog state,
//! window, render gates, and audio players are process-local projections of it.

use crate::app::AppState;
use crate::app::persistence::options_profile::RetailOptionsProfile;
use crate::app::presentation::target_lines::TargetLineState;
use crate::ui::shell::in_game_options_state::{
    InGameOptionsState, OPTIONS_DETAIL_MAX, OPTIONS_SPEED_MAX,
};
use crate::ui::shell::modal::ModalResult;
use crate::ui::tooltips::TooltipService;

pub(crate) mod audio;
pub(crate) mod launcher;

/// Normal Back result. Native result 1 applies then writes; result 2 performs
/// neither operation.
pub(crate) const IN_GAME_OPTIONS_RESULT_BACK: i32 = 1;

fn project_speed_control(value: i32) -> u32 {
    value.clamp(0, OPTIONS_SPEED_MAX as i32) as u32
}

/// Build the bounded dialog/live-consumer projection from the retained signed
/// profile fields. The profile keeps hand-edited out-of-range values until an
/// accepted bounded dialog selection replaces them.
pub(crate) fn in_game_options_from_profile(profile: &RetailOptionsProfile) -> InGameOptionsState {
    InGameOptionsState {
        game_speed: project_speed_control(profile.game_speed),
        scroll_rate: project_speed_control(profile.scroll_rate),
        detail_level: profile.detail_level.clamp(0, OPTIONS_DETAIL_MAX as i32) as u32,
        unit_action_lines: profile.unit_action_lines,
        show_hidden: profile.show_hidden,
        tooltips: profile.tooltips,
        ..Default::default()
    }
}

/// Copy an admitted dialog snapshot into the single process profile.
///
/// Retail provenance: `OptionsClass__ShowInGameDialog @ 0x004E1D00` admits
/// result 1 only, then calls `OptionsClass__ApplyFromInGameDialog @ 0x004E1DE0`
/// before `OptionsClass__WriteToINI @ 0x005FAD10`.
fn update_profile_from_in_game_options(
    profile: &mut RetailOptionsProfile,
    options: &InGameOptionsState,
) {
    profile.game_speed = options.game_speed as i32;
    profile.scroll_rate = options.scroll_rate as i32;
    profile.detail_level = options.detail_level as i32;
    profile.unit_action_lines = options.unit_action_lines;
    profile.show_hidden = options.show_hidden;
    profile.tooltips = options.tooltips;
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

/// Apply accepted controls to their existing production consumers. Interaction
/// itself remains visual-only; this runs at the native close boundary.
///
/// Retail provenance: `OptionsClass__ApplyFromInGameDialog @ 0x004E1DE0`.
fn apply_in_game_options(state: &mut AppState) {
    state.match_state.sim_speed_tps = crate::app::types::tps_for_game_speed(
        state
            .match_state
            .match_presentation
            .in_game_options
            .game_speed,
    );
    if state.match_state.sim_runtime.is_some() {
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
                &state.match_state.sim_runtime.as_ref().unwrap().simulation,
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

    let options = &state.match_state.match_presentation.in_game_options;
    apply_presentation_option_gates(
        &mut state.match_state.match_presentation.target_lines,
        &mut state.match_state.match_presentation.tooltips,
        options,
    );
    // ScrollRate, DetailLevel (lighting and PixelFX), and ShowHidden are read
    // directly from the live projection by their existing consumers.
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
        .commit_ra2md(config.paths.settings_dir())
    {
        log::warn!("Failed to persist Options profile to RA2MD.INI: {error}");
    }
}

/// Narrow operation boundary for the native accepted-dialog transaction.
/// Production and tests both enter through `dispatch_in_game_options_transaction`;
/// the adapter keeps GPU-backed `AppState` construction out of unit tests.
trait InGameOptionsTransactionOperations {
    fn update_profile(&mut self);
    fn apply_consumers(&mut self);
    fn persist_profile(&mut self);
}

struct AppStateOptionsTransaction<'a> {
    state: &'a mut AppState,
}

impl InGameOptionsTransactionOperations for AppStateOptionsTransaction<'_> {
    fn update_profile(&mut self) {
        let options = self
            .state
            .match_state
            .match_presentation
            .in_game_options
            .clone();
        update_profile_from_in_game_options(&mut self.state.persistence.options_profile, &options);
    }

    fn apply_consumers(&mut self) {
        apply_in_game_options(self.state);
    }

    fn persist_profile(&mut self) {
        persist_options_profile(self.state);
    }
}

fn dispatch_in_game_options_transaction(
    operations: &mut impl InGameOptionsTransactionOperations,
    result: i32,
) -> bool {
    if !ModalResult::InGameOptions(result).options_persists() {
        return false;
    }

    operations.update_profile();
    operations.apply_consumers();
    operations.persist_profile();
    true
}

/// Native close transaction for the Options dialog's result 1: mutate the
/// retained profile, apply consumers, then perform one complete write. Result
/// 2 performs none of those operations. It runs when the dialog returns (Back
/// or Escape) and before Sound or Keyboard opens over it; leaving the dialog
/// belongs to the in-game menu state machine (`App::route_in_game_menu_escape`).
///
/// Retail provenance: `OptionsClass__ShowInGameDialog @ 0x004E1D00` and
/// `OptionsClass__ApplyFromInGameDialog @ 0x004E1DE0`.
pub(crate) fn accept_in_game_options(state: &mut AppState) {
    let mut operations = AppStateOptionsTransaction { state };
    dispatch_in_game_options_transaction(&mut operations, IN_GAME_OPTIONS_RESULT_BACK);
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

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TransactionEvent {
        Profile,
        Consumers,
        Write,
    }

    struct RecordingOptionsTransaction {
        profile: RetailOptionsProfile,
        options: InGameOptionsState,
        events: Vec<TransactionEvent>,
        consumer_effects: usize,
        writes: usize,
    }

    impl InGameOptionsTransactionOperations for RecordingOptionsTransaction {
        fn update_profile(&mut self) {
            assert!(self.events.is_empty(), "profile mutation must be first");
            update_profile_from_in_game_options(&mut self.profile, &self.options);
            self.events.push(TransactionEvent::Profile);
        }

        fn apply_consumers(&mut self) {
            assert_eq!(self.events, [TransactionEvent::Profile]);
            assert_eq!(self.profile.game_speed, self.options.game_speed as i32);
            self.consumer_effects += 1;
            self.events.push(TransactionEvent::Consumers);
        }

        fn persist_profile(&mut self) {
            assert_eq!(
                self.events,
                [TransactionEvent::Profile, TransactionEvent::Consumers]
            );
            self.writes += 1;
            self.events.push(TransactionEvent::Write);
        }
    }

    fn recording_transaction(
        profile: RetailOptionsProfile,
        options: InGameOptionsState,
    ) -> RecordingOptionsTransaction {
        RecordingOptionsTransaction {
            profile,
            options,
            events: Vec::new(),
            consumer_effects: 0,
            writes: 0,
        }
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
    fn accepted_dialog_runs_profile_consumers_then_exactly_one_write() {
        let profile = RetailOptionsProfile {
            difficulty: 4,
            sound_volume: 0.25,
            ..Default::default()
        };
        let options = InGameOptionsState {
            game_speed: 5,
            scroll_rate: 2,
            detail_level: 0,
            unit_action_lines: false,
            show_hidden: true,
            tooltips: false,
            ..Default::default()
        };
        let mut transaction = recording_transaction(profile, options);

        assert!(dispatch_in_game_options_transaction(
            &mut transaction,
            IN_GAME_OPTIONS_RESULT_BACK
        ));
        assert_eq!(
            (
                transaction.profile.game_speed,
                transaction.profile.scroll_rate
            ),
            (5, 2)
        );
        assert_eq!(transaction.profile.detail_level, 0);
        assert!(!transaction.profile.unit_action_lines);
        assert!(transaction.profile.show_hidden);
        assert!(!transaction.profile.tooltips);
        assert_eq!(transaction.profile.difficulty, 4);
        assert_eq!(transaction.profile.sound_volume, 0.25);
        assert_eq!(transaction.consumer_effects, 1);
        assert_eq!(transaction.writes, 1);
        assert_eq!(
            transaction.events,
            [
                TransactionEvent::Profile,
                TransactionEvent::Consumers,
                TransactionEvent::Write,
            ]
        );
    }

    #[test]
    fn result_two_runs_no_profile_consumer_or_write_operation() {
        let profile = RetailOptionsProfile::default();
        let original = profile.clone();
        let options = InGameOptionsState {
            game_speed: 6,
            tooltips: false,
            ..Default::default()
        };
        let mut transaction = recording_transaction(profile, options);

        assert!(!dispatch_in_game_options_transaction(&mut transaction, 2));
        assert_eq!(transaction.profile, original);
        assert_eq!(transaction.consumer_effects, 0);
        assert_eq!(transaction.writes, 0);
        assert!(transaction.events.is_empty());
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
