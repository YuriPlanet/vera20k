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

#[test]
fn options_opens_only_after_accepted_local_dispatch_and_never_during_replay() {
    let make = || {
        let mut sim = Simulation::new();
        let local = sim.interner.intern("Local");
        let peer = sim.interner.intern("Peer");
        sim.houses
            .register(HouseState::new(local, 0, None, true, 0, 10));
        sim.houses
            .register(HouseState::new(peer, 1, None, true, 0, 10));
        sim.session.set_current_house_for_test(Some(local));
        (sim, local, peer)
    };
    let (mut sim, local, peer) = make();
    for _ in 0..128 {
        assert!(sim.issue_local_command(CommandEnvelope::new(
            local,
            0,
            Command::Stop { entity_id: 999 }
        )));
    }
    assert!(!sim.queue_options_request());
    let mut full = sim
        .advance_app_frame(
            &[],
            RuleSet::empty_for_test(),
            None,
            33,
            crate::sim::world::TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
    assert!(
        !full
            .lifecycle_outputs
            .contains(&crate::sim::world::LifecycleOutput::OptionsRequested)
    );
    assert_eq!(full.take_admitted_commands().len(), 128);

    assert!(sim.queue_options_request());
    assert!(
        !sim.lifecycle_outputs
            .take()
            .contains(&crate::sim::world::LifecycleOutput::OptionsRequested)
    );
    let peer_event = CommandEnvelope::new(peer, 1, Command::OpenOptions);
    let mut output = sim
        .advance_app_frame(
            &[peer_event],
            RuleSet::empty_for_test(),
            None,
            33,
            crate::sim::world::TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
    assert_eq!(
        output
            .lifecycle_outputs
            .iter()
            .filter(|event| **event == crate::sim::world::LifecycleOutput::OptionsRequested)
            .count(),
        1
    );
    let recorded = output.take_admitted_commands();
    let (mut replay, _, _) = make();
    replay.session.set_tick_for_test(1);
    let due = replay.take_due_replay_commands(recorded);
    let replay_output = replay
        .advance_app_frame(
            &due,
            RuleSet::empty_for_test(),
            None,
            33,
            crate::sim::world::TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
    assert!(
        !replay_output
            .lifecycle_outputs
            .contains(&crate::sim::world::LifecycleOutput::OptionsRequested)
    );
}

#[test]
fn latency_event_occupies_a_slot_and_publishes_its_unclamped_session_write() {
    let mut sim = Simulation::new();
    let local = sim.interner.intern("Local");
    sim.houses
        .register(HouseState::new(local, 0, None, true, 0, 10));
    for _ in 0..127 {
        assert!(sim.issue_local_command(CommandEnvelope::new(
            local,
            0,
            Command::Stop { entity_id: 999 }
        )));
    }
    assert!(sim.issue_local_command(CommandEnvelope::new(
        local,
        0,
        Command::SetLatencyFudge { value: -17 }
    )));
    assert!(!sim.issue_local_command(CommandEnvelope::new(
        local,
        0,
        Command::LiveGameSpeed { speed: 6 }
    )));
    let output = sim
        .advance_app_frame(
            &[],
            RuleSet::empty_for_test(),
            None,
            33,
            crate::sim::world::TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
    assert!(output.lifecycle_outputs.contains(
        &crate::sim::world::LifecycleOutput::LatencyFudgeChanged {
            owner: local,
            value: -17
        }
    ));
    assert_eq!(sim.session.game_options().game_speed, 1);
}

#[test]
fn local_extensions_keep_paid_selection_order_without_using_event_slots() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Local");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    let selection = crate::sim::superweapon::PaidSuperWeaponSelection {
        source: sim.interner.intern("Source"),
        destination: sim.interner.intern("Destination"),
        sequence: 1,
    };
    for _ in 0..127 {
        assert!(sim.issue_local_command(CommandEnvelope::new(
            owner,
            0,
            Command::Stop { entity_id: 999 }
        )));
    }
    let begin = CommandEnvelope::new(
        owner,
        0,
        Command::BeginPaidSuperWeapon {
            selection,
            target_rx: 10,
            target_ry: 10,
        },
    );
    let cancel = CommandEnvelope::new(owner, 0, Command::CancelPaidSuperWeapon { selection });
    assert!(sim.issue_local_command(begin.clone()));
    assert_eq!(
        sim.projected_paid_super_weapon_selection(owner),
        Some((selection, true))
    );
    assert!(
        sim.issue_local_command(cancel.clone()),
        "local cancellation is not a native event"
    );
    assert_eq!(sim.projected_paid_super_weapon_selection(owner), None);
    assert!(
        !sim.issue_local_command(begin.clone()),
        "all native slots are occupied"
    );
    let transferred = sim.take_frame_commands_for_test();
    assert_eq!(&transferred[127..], &[begin.clone(), cancel.clone()]);

    sim.issue_local_commands([cancel, begin]);
    assert_eq!(
        sim.projected_paid_super_weapon_selection(owner),
        Some((selection, true)),
        "the inverse order selects the source"
    );
}

#[test]
fn replay_keeps_future_local_work_on_the_admitted_scheduler() {
    let make = || {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Local");
        sim.houses
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        let future = CommandEnvelope::new(owner, 2, Command::LiveGameSpeed { speed: 4 });
        assert!(sim.issue_local_command(future.clone()));
        (sim, future)
    };
    let (mut live, future) = make();
    let first = live
        .advance_app_frame(
            &[],
            RuleSet::empty_for_test(),
            None,
            33,
            crate::sim::world::TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
    assert!(first.admitted_commands.is_empty());
    assert_eq!(live.pending_command_snapshot(), [future.clone()]);
    let (mut replay, _) = make();
    let due = replay.take_due_replay_commands([]);
    replay
        .advance_app_frame(
            &due,
            RuleSet::empty_for_test(),
            None,
            33,
            crate::sim::world::TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
    assert_eq!(replay.pending_command_snapshot(), [future.clone()]);
    let due = replay.take_due_replay_commands([future.clone()]);
    assert_eq!(due, [future]);
    replay
        .advance_app_frame(
            &due,
            RuleSet::empty_for_test(),
            None,
            33,
            crate::sim::world::TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
    assert_eq!(replay.session.game_options().game_speed, 4);
    assert!(replay.pending_command_snapshot().is_empty());
}
