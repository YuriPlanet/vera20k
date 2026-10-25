use super::*;
use crate::sim::house_state::HouseState;
use crate::sim::scenario_session::ScenarioGameMode;

fn native_dolist_world() -> Simulation {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Human");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    sim.session.set_game_mode(ScenarioGameMode::Wol);
    sim.session.install_network_timing(10);
    sim.session.game_options_mut_for_test().game_speed = 3;
    sim
}

#[test]
fn borrowed_execution_observer_reports_actual_timing_acceptance() {
    use crate::sim::command::{
        PROCESS_TIME_OPCODE, ProcessTimeRecord, TIMING_OPCODE, TimingRecord,
    };
    use std::cell::RefCell;
    use std::num::NonZeroU8;
    let mut sim = native_dolist_world();
    for (index, name) in [(1, "Publisher"), (2, "Unadmitted")] {
        let owner = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(owner, index, None, true, 0, 10));
    }
    sim.network_command_queues()
        .install_timing(1, NonZeroU8::new(5).unwrap(), [0, 1]);
    for house in [0, 1] {
        sim.admit_native_command_record(
            TimingRecord {
                house_id: house,
                frame: 0,
                frames_per_second: if house == 1 { 45 } else { 60 },
                max_ahead: 10,
                frame_send_rate: NonZeroU8::new(5).unwrap(),
            }
            .encode()
            .unwrap(),
        );
    }
    for house in [0, 2] {
        sim.admit_native_command_record(
            ProcessTimeRecord {
                house_id: house,
                frame: 0,
                milliseconds: 12,
            }
            .encode()
            .unwrap(),
        );
    }
    let observed = RefCell::new(Vec::new());
    let observer = |sim: &Simulation, record: &crate::sim::command::CommandRecord, applied| {
        observed.borrow_mut().push((
            record.opcode(),
            record.house_id(),
            applied,
            sim.network_timing_host_house(),
            sim.session
                .network_command_timing()
                .unwrap()
                .frames_per_second(),
        ));
    };
    sim.advance_master_frame(
        &[],
        RuleSet::empty_for_test(),
        None,
        33,
        crate::sim::world::TickLane::Ordinary,
        false,
        FrameEffects::default().with_command_applied(&observer),
    )
    .unwrap();
    let rows = observed.into_inner();
    assert_eq!(rows.len(), 4);
    assert!(
        rows.iter()
            .any(|row| row.0 == TIMING_OPCODE && row.1 == 0 && !row.2)
    );
    assert!(
        rows.iter()
            .any(|row| row.0 == PROCESS_TIME_OPCODE && row.1 == 0 && row.2)
    );
    assert!(
        rows.iter()
            .any(|row| row.0 == PROCESS_TIME_OPCODE && row.1 == 2 && !row.2)
    );
    assert!(
        rows.iter()
            .any(|row| *row == (TIMING_OPCODE, 1, true, Some(1), 45))
    );
    assert_eq!(
        sim.session
            .network_command_timing()
            .unwrap()
            .maximum_processing_millis([0]),
        Some(12)
    );
    assert_eq!(
        sim.session
            .network_command_timing()
            .unwrap()
            .maximum_processing_millis([2]),
        None
    );
}

#[test]
fn admitted_host_timing_retimes_future_events_and_resets_for_new_match() {
    use crate::sim::command::{CommandRecord, ProcessTimeRecord, TimingRecord};
    use std::num::NonZeroU8;
    let mut sim = native_dolist_world();
    let guest = sim.interner.intern("Host");
    sim.houses
        .register(HouseState::new(guest, 1, None, true, 0, 10));
    sim.session.install_network_timing(10);
    sim.session
        .install_network_command_timing(1, NonZeroU8::new(5).unwrap(), [0, 1]);
    sim.session.game_options_mut_for_test().fog_of_war = true;
    for (house_id, max_ahead) in [(0, 200), (1, 30)] {
        sim.admit_native_command_record(
            TimingRecord {
                house_id,
                frame: 0,
                frames_per_second: 45,
                max_ahead,
                frame_send_rate: NonZeroU8::new(10).unwrap(),
            }
            .encode()
            .unwrap(),
        );
        sim.admit_native_command_record(
            ProcessTimeRecord {
                house_id,
                frame: 0,
                milliseconds: 10 + house_id as u16,
            }
            .encode()
            .unwrap(),
        );
    }
    // A malformed zero-rate record cannot cause a divide-by-zero or clear the
    // valid host's window. The source0 TIMING above is likewise not authoritative.
    sim.admit_native_command_record(CommandRecord::encode(0x20, 1, 0, &[0; 5]).unwrap());
    sim.admit_native_command_record(
        crate::sim::command::GameSpeedRecord {
            house_id: 0,
            frame: 5,
            speed: 6,
        }
        .encode()
        .unwrap(),
    );
    sim.advance_tick(&[], None, None, None, 33);
    assert_eq!(sim.session.network_max_ahead_frames(), Some(20));
    assert_eq!(sim.pending_commands.retime_window().bounds(), (0, 20));
    assert_eq!(
        sim.session
            .network_command_timing()
            .unwrap()
            .frames_per_second(),
        45
    );
    assert_eq!(
        sim.session
            .network_command_timing()
            .unwrap()
            .maximum_processing_millis([0, 1]),
        Some(11)
    );
    sim.advance_tick(&[], None, None, None, 33);
    assert!(sim.pending_commands.iter().any(|entry| {
        entry
            .record()
            .is_some_and(|record| record.opcode() == 0x0d && record.frame_stamp() == 20)
    }));
    assert_eq!(sim.session.game_options().game_speed, 3);
    let saved = crate::util::save_codec::serialize(&sim.session).unwrap();
    let restored: crate::sim::scenario_session::ScenarioSession =
        crate::util::save_codec::deserialize(&saved).unwrap();
    assert_eq!(
        restored.network_command_timing(),
        sim.session.network_command_timing()
    );
    sim.session.set_game_mode(ScenarioGameMode::Skirmish);
    assert!(sim.session.network_command_timing().is_none());
    sim.session.set_game_mode(ScenarioGameMode::Wol);
    sim.session.install_network_timing(10);
    sim.network_command_queues()
        .install_timing(0, NonZeroU8::new(5).unwrap(), [0, 1]);
    assert_eq!(sim.pending_commands.retime_window().bounds(), (0, 0));
    let fresh = sim.session.network_command_timing().unwrap();
    assert_eq!(fresh.frames_per_second(), 30);
    assert_eq!(fresh.maximum_processing_millis([0, 1]), None);
}

#[test]
fn lan_timing_uses_live_house_order_instead_of_the_setup_host() {
    use crate::sim::command::TimingRecord;
    use std::num::NonZeroU8;
    let mut sim = native_dolist_world();
    let first = sim.houses.order()[0];
    let other = sim.interner.intern("SetupHost");
    sim.houses
        .register(HouseState::new(other, 1, None, true, 0, 10));
    sim.session.set_game_mode(ScenarioGameMode::Lan);
    sim.session.install_network_timing(10);
    sim.session
        .install_network_command_timing(1, NonZeroU8::new(5).unwrap(), [0, 1]);
    assert_eq!(sim.network_command_queues().timing_host_house(), Some(0));
    sim.admit_native_command_record(
        TimingRecord {
            house_id: 0,
            frame: 0,
            max_ahead: 10,
            frames_per_second: 45,
            frame_send_rate: NonZeroU8::new(5).unwrap(),
        }
        .encode()
        .unwrap(),
    );
    sim.advance_tick(&[], None, None, None, 33);
    assert_eq!(
        sim.session
            .network_command_timing()
            .unwrap()
            .frames_per_second(),
        45
    );
    sim.houses
        .get_mut(&first)
        .unwrap()
        .set_is_defeated_for_test(true);
    assert_eq!(sim.network_command_queues().timing_host_house(), Some(1));
}

#[test]
fn wol_defeat_keeps_the_publisher_until_shared_exit_or_removal() {
    use crate::sim::command::{ExitRecord, TimingRecord};
    use std::num::NonZeroU8;
    let mut sim = native_dolist_world();
    for (index, name) in [(1, "Publisher"), (2, "Survivor")] {
        let owner = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(owner, index, None, true, 0, 10));
    }
    let publisher = sim.houses.order()[1];
    let survivor = sim.houses.order()[2];
    sim.session.set_current_house_for_test(Some(survivor));
    sim.session.install_network_timing(10);
    sim.session
        .install_network_command_timing(1, NonZeroU8::new(5).unwrap(), [0, 1, 2]);
    sim.houses
        .get_mut(&publisher)
        .unwrap()
        .set_is_defeated_for_test(true);
    assert_eq!(sim.network_command_queues().timing_host_house(), Some(1));
    sim.admit_native_command_record(
        TimingRecord {
            house_id: 1,
            frame: 0,
            max_ahead: 10,
            frames_per_second: 45,
            frame_send_rate: NonZeroU8::new(5).unwrap(),
        }
        .encode()
        .unwrap(),
    );
    sim.advance_tick(&[], None, None, None, 33);
    assert_eq!(
        sim.session
            .network_command_timing()
            .unwrap()
            .frames_per_second(),
        45
    );
    sim.admit_native_command_record(
        ExitRecord {
            house_id: 1,
            frame: 1,
        }
        .encode()
        .unwrap(),
    );
    sim.advance_tick(&[], None, None, None, 33);
    assert!(sim.houses[&publisher].left_game());
    assert_eq!(sim.network_command_queues().timing_host_house(), Some(0));

    let removed = sim.encode_remove_player_record(survivor, 0).unwrap();
    sim.admit_native_command_record(removed);
    for (house_id, frames_per_second) in [(1, 60), (2, 25)] {
        sim.admit_native_command_record(
            TimingRecord {
                house_id,
                frame: 2,
                max_ahead: 10,
                frames_per_second,
                frame_send_rate: NonZeroU8::new(5).unwrap(),
            }
            .encode()
            .unwrap(),
        );
    }
    sim.advance_tick(&[], None, None, None, 33);
    assert_eq!(sim.network_command_queues().timing_host_house(), Some(2));
    let timing = sim.session.network_command_timing().unwrap();
    assert_eq!(timing.frames_per_second(), 25);
    assert!(!timing.admits_source(0) && !timing.admits_source(1));

    // A source already removed by the loading wait must not fail installation
    // or make a connected-but-defeated stale publisher authoritative again.
    sim.network_command_queues()
        .install_timing(1, NonZeroU8::new(5).unwrap(), [2]);
    assert_eq!(sim.network_command_queues().timing_host_house(), Some(2));
}

#[test]
fn native_frame_and_house_override_carrier_metadata() {
    let mut sim = native_dolist_world();
    let record = crate::sim::command::GameSpeedRecord {
        house_id: 0,
        frame: 2,
        speed: 6,
    }
    .encode()
    .unwrap();
    let mut carrier = CommandEnvelope::synchronized(record, u64::MAX);
    carrier.owner = InternedId::from_index(u32::MAX);
    sim.queue_command(carrier);
    for _ in 0..2 {
        sim.advance_tick(&[], None, None, None, 33);
        assert_eq!(sim.session.game_options().game_speed, 3);
    }
    let result = sim.advance_tick(&[], None, None, None, 33);
    assert_eq!(sim.session.game_options().game_speed, 6);
    assert_eq!(result.executed_commands, 1);
    assert!(sim.pending_commands.is_empty());
}

#[test]
fn native_speed_projection_uses_the_record_frame_instead_of_diagnostic_tick() {
    for (frame, diagnostic_tick, expected) in [(0, u64::MAX, 6), (2, 0, 3), (-1, 0, 3)] {
        let mut sim = native_dolist_world();
        let record = crate::sim::command::GameSpeedRecord {
            house_id: 0,
            frame,
            speed: 6,
        }
        .encode()
        .unwrap();
        sim.queue_command(CommandEnvelope::synchronized(
            record.clone(),
            diagnostic_tick,
        ));
        let diagnostic = sim.pending_command_snapshot();
        assert_eq!(diagnostic[0].execute_tick, diagnostic_tick);
        assert_eq!(sim.projected_in_game_options_speed(), Some(expected));
        assert_eq!(sim.pending_command_snapshot(), diagnostic);
        let replay = sim
            .pending_commands
            .iter()
            .next()
            .unwrap()
            .replay_envelope();
        assert_eq!(replay.execute_tick, diagnostic_tick);
        assert!(
            matches!(replay.payload, Command::SynchronizedRecord { record: retained } if retained == record)
        );
    }
}

#[test]
fn processed_native_records_do_not_reapply_to_input_projections() {
    let mut sim = native_dolist_world();
    let record = crate::sim::command::GameSpeedRecord {
        house_id: 0,
        frame: 0,
        speed: 6,
    }
    .encode()
    .unwrap();
    sim.admit_native_command_record(record);
    sim.pending_commands
        .iter_mut()
        .next()
        .unwrap()
        .mark_processed();
    assert_eq!(sim.projected_in_game_options_speed(), Some(3));
}

#[test]
fn unresolved_native_megamissions_still_consume_the_production_staging_slots() {
    let mut sim = native_dolist_world();
    sim.session.set_game_mode(ScenarioGameMode::Skirmish);
    for _ in 0..257 {
        let record = crate::sim::command::CommandRecord::encode(4, 0, 0, &[0; 23]).unwrap();
        assert!(sim.admit_native_command_record(record));
    }
    let result = sim.advance_tick(&[], None, None, None, 33);
    assert_eq!(result.executed_commands, 256);
    assert!(sim.pending_commands.is_empty());
}

#[test]
fn late_native_record_stays_unmarked_behind_a_future_head() {
    let mut sim = native_dolist_world();
    for (frame, speed) in [(3, 6), (-1, 4)] {
        assert!(
            sim.admit_native_command_record(
                crate::sim::command::GameSpeedRecord {
                    house_id: 0,
                    frame,
                    speed,
                }
                .encode()
                .unwrap()
            )
        );
    }
    sim.advance_tick(&[], None, None, None, 33);
    assert_eq!(sim.session.game_options().game_speed, 3);
    assert_eq!(sim.pending_commands.len(), 2);
    assert!(
        !sim.pending_commands
            .iter()
            .nth(1)
            .unwrap()
            .record()
            .unwrap()
            .is_processed()
    );
    let saved = serde_json::to_vec(&sim).unwrap();
    let mut restored: Simulation = serde_json::from_slice(&saved).unwrap();
    assert_eq!(restored.pending_commands.len(), 2);
    for _ in 0..3 {
        restored.advance_tick(&[], None, None, None, 33);
    }
    assert_eq!(restored.session.game_options().game_speed, 6);
    assert!(restored.pending_commands.is_empty());
}

#[test]
fn processed_native_tail_survives_snapshot_without_executing_twice() {
    let mut sim = native_dolist_world();
    for (frame, speed) in [(3, 6), (0, 4)] {
        sim.admit_native_command_record(
            crate::sim::command::GameSpeedRecord {
                house_id: 0,
                frame,
                speed,
            }
            .encode()
            .unwrap(),
        );
    }
    sim.advance_tick(&[], None, None, None, 33);
    assert_eq!(sim.session.game_options().game_speed, 4);
    assert_eq!(sim.pending_commands.len(), 2);
    assert!(
        sim.pending_commands
            .iter()
            .nth(1)
            .unwrap()
            .record()
            .unwrap()
            .is_processed()
    );
    let saved = serde_json::to_vec(&sim).unwrap();
    let mut restored: Simulation = serde_json::from_slice(&saved).unwrap();
    assert!(
        restored
            .pending_commands
            .iter()
            .nth(1)
            .unwrap()
            .record()
            .unwrap()
            .is_processed()
    );
    for _ in 0..3 {
        restored.advance_tick(&[], None, None, None, 33);
    }
    assert_eq!(restored.session.game_options().game_speed, 6);
    assert!(restored.pending_commands.is_empty());
}

#[test]
fn future_native_admission_is_recorded_once_and_replays_at_its_raw_frame() {
    let mut live = native_dolist_world();
    let initial = serde_json::to_vec(&live).unwrap();
    live.admit_native_command_record(
        crate::sim::command::GameSpeedRecord {
            house_id: 0,
            frame: 2,
            speed: 6,
        }
        .encode()
        .unwrap(),
    );
    let mut replay: Simulation = serde_json::from_slice(&initial).unwrap();
    for frame in 0..4 {
        let mut output = live
            .advance_app_frame(
                &[],
                RuleSet::empty_for_test(),
                None,
                33,
                crate::sim::world::TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        let recorded = output.take_admitted_commands();
        assert_eq!(recorded.len(), usize::from(frame == 0));
        let replayed = replay.take_due_replay_commands(recorded);
        replay
            .advance_app_frame(
                &replayed,
                RuleSet::empty_for_test(),
                None,
                33,
                crate::sim::world::TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        assert_eq!(
            live.session.game_options().game_speed,
            replay.session.game_options().game_speed
        );
        assert_eq!(live.pending_commands, replay.pending_commands);
    }
}

#[test]
fn production_dolist_preserves_unknown_records_and_enforces_native_capacity() {
    let mut sim = native_dolist_world();
    sim.session.set_game_mode(ScenarioGameMode::Skirmish);
    let record = crate::sim::command::CommandRecord::encode(0xff, -1, 10, &[0xaa]).unwrap();
    for _ in 0..crate::sim::command_queue::DO_LIST_CAPACITY {
        assert!(sim.admit_native_command_record(record.clone()));
    }
    assert!(!sim.admit_native_command_record(record));
    assert_eq!(
        sim.pending_commands.native_len(),
        crate::sim::command_queue::DO_LIST_CAPACITY
    );
    sim.advance_tick(&[], None, None, None, 33);
    assert_eq!(
        sim.pending_commands.native_len(),
        crate::sim::command_queue::DO_LIST_CAPACITY
    );
}

#[test]
fn frame_info_failure_resumes_only_the_remaining_tail_after_acknowledgement() {
    use std::cell::Cell;
    struct NetworkBoundary {
        published: Cell<Option<u32>>,
        compared: Cell<usize>,
    }
    impl crate::sim::world::NetworkCommandBoundary for NetworkBoundary {
        fn record_frame_checksum(&self, frame: u32, _checksum: u32) {
            self.published.set(Some(frame));
        }
        fn compare_frame_info(
            &self,
            _record: &crate::sim::command::CommandRecord,
            current_frame: i32,
        ) -> Result<bool, crate::sim::world::NetworkCommandError> {
            assert_eq!(self.published.get(), Some(current_frame as u32));
            self.compared.set(self.compared.get() + 1);
            Err(crate::sim::command_queue::ChecksumMismatch {
                house_id: 0,
                event_frame: current_frame,
                delay: 0,
                history_index: 0,
                local_checksum: 1,
                remote_checksum: 2,
            }
            .into())
        }
    }
    for terminal in [false, true] {
        let mut sim = native_dolist_world();
        sim.set_quit_requested_for_test(terminal);
        // Ordinary work before the failing event stands. The staged copy is
        // marked but never executed; failure/later entries remain unmarked.
        sim.admit_native_command_record(
            crate::sim::command::GameSpeedRecord {
                house_id: 0,
                frame: 0,
                speed: 4,
            }
            .encode()
            .unwrap(),
        );
        sim.admit_native_command_record(
            crate::sim::command::CommandRecord::encode(4, 0, 0, &[0; 23]).unwrap(),
        );
        sim.admit_native_command_record(
            crate::sim::command::CommandRecord::encode(0x1c, 0, 0, &[0; 7]).unwrap(),
        );
        sim.admit_native_command_record(
            crate::sim::command::GameSpeedRecord {
                house_id: 0,
                frame: 0,
                speed: 6,
            }
            .encode()
            .unwrap(),
        );
        let boundary = NetworkBoundary {
            published: Cell::new(None),
            compared: Cell::new(0),
        };
        let output = sim
            .advance_app_frame(
                &[],
                RuleSet::empty_for_test(),
                None,
                33,
                crate::sim::world::TickLane::Ordinary,
                FrameEffects::default().with_network_commands(&boundary),
            )
            .unwrap();
        assert!(output.network_failure.is_some());
        assert_eq!(boundary.compared.get(), 1);
        assert!(!output.tick.frame_committed);
        assert!(output.admitted_commands.is_empty());
        assert_eq!(sim.session.binary_frame(), 0);
        assert_eq!(sim.session.game_options().game_speed, 4);
        assert_eq!(sim.dispatch_reserved_slots.total, 0);
        let flags: Vec<_> = sim
            .pending_commands
            .iter()
            .map(|entry| entry.record().unwrap().is_processed())
            .collect();
        assert_eq!(flags, [true, true, false, false]);
        assert_eq!(
            output
                .lifecycle_outputs
                .iter()
                .filter(|event| matches!(event, crate::sim::world::LifecycleOutput::LogicVisit))
                .count(),
            1
        );
        let mut restored = if terminal {
            // EXIT is an existing process-local latch, deliberately not saved.
            sim
        } else {
            let saved = serde_json::to_vec(&sim).unwrap();
            serde_json::from_slice::<Simulation>(&saved).unwrap()
        };
        // The pending native call frame survives a snapshot and blocks Logic.
        assert!(restored.pending_network_failure().is_some());
        assert!(
            restored
                .advance_app_frame(
                    &[],
                    RuleSet::empty_for_test(),
                    None,
                    33,
                    crate::sim::world::TickLane::Ordinary,
                    FrameEffects::default()
                )
                .is_err()
        );
        let resumed = restored
            .resume_after_network_failure(RuleSet::empty_for_test(), None, FrameEffects::default())
            .unwrap();
        assert!(resumed.network_failure.is_none());
        assert_eq!(resumed.tick.frame_committed, !terminal);
        assert_eq!(restored.session.binary_frame(), u32::from(!terminal));
        assert_eq!(resumed.tick.executed_commands, 1);
        assert_eq!(resumed.admitted_commands.len(), 4);
        assert!(
            !resumed
                .lifecycle_outputs
                .iter()
                .any(|event| matches!(event, crate::sim::world::LifecycleOutput::LogicVisit))
        );
        assert_eq!(restored.session.game_options().game_speed, 4);
        assert_eq!(
            restored.pending_commands.len(),
            4,
            "failure never retires DoList"
        );
        assert!(
            restored
                .resume_after_network_failure(
                    RuleSet::empty_for_test(),
                    None,
                    FrameEffects::default()
                )
                .is_err()
        );
    }
}

#[test]
fn frame_info_without_its_process_owner_is_not_silently_acknowledged() {
    let mut sim = native_dolist_world();
    sim.admit_native_command_record(
        crate::sim::command::CommandRecord::encode(0x1c, 0, 0, &[0; 7]).unwrap(),
    );
    let error = sim
        .advance_app_frame(
            &[],
            RuleSet::empty_for_test(),
            None,
            33,
            crate::sim::world::TickLane::Ordinary,
            FrameEffects::default(),
        )
        .err()
        .expect("missing callback fails");
    assert!(error.cause.contains("network session checksum owner"));
    assert_eq!(sim.pending_commands.len(), 1);
    assert_eq!(sim.session.binary_frame(), 0);
}

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
        sim.projected_paid_super_weapon_selection(owner, RuleSet::empty_for_test()),
        Some((selection, true))
    );
    assert!(
        sim.issue_local_command(cancel.clone()),
        "local cancellation is not a native event"
    );
    assert_eq!(
        sim.projected_paid_super_weapon_selection(owner, RuleSet::empty_for_test()),
        None
    );
    assert!(
        !sim.issue_local_command(begin.clone()),
        "all native slots are occupied"
    );
    let transferred = sim.take_frame_commands_for_test();
    assert_eq!(&transferred[127..], &[begin.clone(), cancel.clone()]);

    sim.issue_local_commands([cancel, begin]);
    assert_eq!(
        sim.projected_paid_super_weapon_selection(owner, RuleSet::empty_for_test()),
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
    assert_eq!(first.admitted_commands, [future.clone()]);
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
