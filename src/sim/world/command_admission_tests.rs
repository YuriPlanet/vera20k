use super::*;
use crate::sim::house_state::HouseState;
use crate::sim::scenario_session::ScenarioGameMode;

#[test]
fn synchronized_tail_admits_human_or_player_controlled_houses_only() {
    // Execute_DoList 0x64C4E2..0x64C50D scans registered Houses and tests
    // IsHuman || PlayerControl before any EventClass payload executes.
    for mode in [0, 3, 4, 5] {
        let mut sim = Simulation::new();
        sim.session
            .set_game_mode(ScenarioGameMode::from_game_mode(mode));
        let human = sim.interner.intern("Human");
        let ai = sim.interner.intern("Computer");
        let controlled = sim.interner.intern("PlayerControl");
        let unregistered = sim.interner.intern("Unregistered");
        for (index, owner, is_human, player_control) in [
            (0, human, true, false),
            (1, ai, false, false),
            (2, controlled, false, true),
        ] {
            let mut house = HouseState::new(owner, index, None, is_human, 0, 10);
            house.set_player_control_for_test(player_control);
            sim.houses.register(house);
        }
        let commands = [(unregistered, 6), (controlled, 4), (ai, 5), (human, 3)]
            .map(|(owner, speed)| CommandEnvelope::new(owner, 1, Command::LiveGameSpeed { speed }));
        let result = sim.advance_tick(&commands, Some(RuleSet::empty_for_test()), None, None, 67);
        assert_eq!(result.executed_commands, 2, "mode {mode}");
        assert_eq!(
            sim.session.game_options().game_speed,
            4,
            "registered House order, mode {mode}"
        );

        // The direct EventClass payload owner is independent of the queue
        // admission gate; do not hide that policy inside individual actions.
        assert!(sim.apply_command_with_overlays(
            "Computer",
            &Command::LiveGameSpeed { speed: 5 },
            RuleSet::empty_for_test(),
            None,
            FrameEffects::default()
        ));
        assert_eq!(sim.session.game_options().game_speed, 5);
    }
}

#[test]
fn speed_projection_matches_ingress_then_eligible_event_tails() {
    let mut sim = Simulation::new();
    let human = sim.interner.intern("Human");
    let ai = sim.interner.intern("Computer");
    sim.houses
        .register(HouseState::new(human, 0, None, true, 0, 10));
    sim.houses
        .register(HouseState::new(ai, 1, None, false, 0, 10));
    // Local Options does not use the EventClass gate. All such ingress runs
    // before the tail, even when a later House supplied the local setting.
    sim.queue_commands([
        CommandEnvelope::new(human, 1, Command::LiveGameSpeed { speed: 4 }),
        CommandEnvelope::new(ai, 1, Command::SetGameSpeed { speed: 2 }),
        CommandEnvelope::new(ai, 1, Command::LiveGameSpeed { speed: 6 }),
    ]);
    assert_eq!(sim.projected_in_game_options_speed(), Some(4));
    let due = sim.take_due_commands();
    let result = sim.advance_tick(&due, Some(RuleSet::empty_for_test()), None, None, 67);
    assert_eq!(result.executed_commands, 2);
    assert_eq!(sim.session.game_options().game_speed, 4);
}
