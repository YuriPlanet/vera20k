use super::*;
use crate::sim::house_state::HouseState;
use crate::sim::scenario_session::ScenarioGameMode;

fn scene(mode: ScenarioGameMode) -> (Simulation, InternedId) {
    let mut sim = Simulation::new();
    sim.session.set_game_mode(mode);
    let owner = sim.interner.intern("Local");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    (sim, owner)
}

fn flash(owner: InternedId) -> CommandEnvelope {
    CommandEnvelope::new(owner, 0, Command::MoveFlash { rx: 12, ry: 15 })
}

#[test]
fn network_move_flash_leaves_the_packet_fifo_and_preserves_other_events() {
    for mode in [ScenarioGameMode::Lan, ScenarioGameMode::Wol] {
        let (mut sim, owner) = scene(mode);
        let flash = flash(owner);
        let stop = CommandEnvelope::synchronized(sim.encode_stop_record(owner, 42).unwrap(), 0);
        let unencoded = CommandEnvelope::new(
            owner,
            0,
            Command::Attack {
                attacker_id: 42,
                target_id: 43,
            },
        );
        sim.issue_local_commands([flash.clone(), stop.clone(), unencoded.clone()]);

        let local: Vec<_> = sim
            .local_commands
            .iter()
            .map(QueuedCommand::replay_envelope)
            .collect();
        assert_eq!(local, [stop, unencoded], "{mode:?}");
        assert_eq!(sim.take_due_commands(), [flash], "{mode:?}");
        assert!(sim.pending_commands.is_empty());
    }
}

#[test]
fn offline_move_flash_keeps_its_original_tail_order() {
    for mode in [
        ScenarioGameMode::Campaign,
        ScenarioGameMode::Skirmish,
        ScenarioGameMode::Other,
    ] {
        let (mut sim, owner) = scene(mode);
        let flash = flash(owner);
        let stop = CommandEnvelope::synchronized(sim.encode_stop_record(owner, 42).unwrap(), 0);
        let expected = [flash, stop];
        sim.issue_local_commands(expected.clone());
        assert!(sim.take_due_commands().is_empty(), "{mode:?}");
        assert_eq!(
            sim.local_commands
                .iter()
                .map(QueuedCommand::replay_envelope)
                .collect::<Vec<_>>(),
            expected,
            "{mode:?}"
        );
        sim.transfer_local_commands();
        assert_eq!(
            sim.pending_commands
                .iter()
                .map(QueuedCommand::replay_envelope)
                .collect::<Vec<_>>(),
            expected,
            "{mode:?}"
        );
    }
}

#[test]
fn network_move_flash_ingress_and_replay_do_not_touch_animation_state() {
    for (replay, local_issue) in [(false, true), (false, false), (true, true), (true, false)] {
        let (mut sim, owner) = scene(ScenarioGameMode::Lan);
        sim.install_native_unique_id_cursor(
            crate::sim::native_identity::NativeUniqueIdCursor::test_at_current_value(42),
        );
        let flash = flash(owner);
        if local_issue {
            assert!(sim.issue_local_command(flash.clone()));
        } else {
            sim.queue_command(flash.clone());
        }
        assert!(sim.frame_command_admissions.is_empty());
        let due = if replay {
            sim.take_due_replay_commands([flash.clone()])
        } else {
            sim.take_due_commands()
        };
        assert_eq!(due, [flash.clone()]);
        assert!(sim.local_commands.is_empty());
        assert!(sim.pending_commands.is_empty());
        let rng = sim.rng_state();
        let next_id = sim.next_stable_object_id();
        let native_id = sim.native_unique_ids().unwrap().current_raw();

        assert_eq!(
            sim.apply_due_frame_ingress_commands(
                &due,
                RuleSet::empty_for_test(),
                1,
                FrameEffects::default(),
            ),
            1
        );
        sim.admit_frame_commands(&due, RuleSet::empty_for_test(), FrameEffects::default());
        let tail = sim
            .dispatch_owned_commands(
                &due,
                RuleSet::empty_for_test(),
                1,
                None,
                FrameEffects::default(),
            )
            .unwrap();
        assert_eq!(tail, (0, false), "ingress must not execute twice");
        assert_eq!(sim.frame_command_admissions, [flash]);
        assert!(sim.pending_commands.is_empty());
        assert_eq!(sim.substrate.anims.len(), 0);
        assert_eq!(sim.next_stable_object_id(), next_id);
        assert_eq!(sim.native_unique_ids().unwrap().current_raw(), native_id);
        assert_eq!(sim.rng_state(), rng);
    }
}
