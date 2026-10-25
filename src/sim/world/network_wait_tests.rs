use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::GameSpeedRecord;
use crate::sim::house_state::HouseState;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::{FrameEffects, NetworkCommandBoundary, NetworkCommandError, TickLane};
use std::cell::Cell;

struct Boundary {
    visits: Cell<u32>,
    checksums: Cell<u32>,
    wait_visits: u32,
    skip: bool,
}

impl NetworkCommandBoundary for Boundary {
    fn exchange_commands(
        &self,
        context: NetworkQueueContext,
        _queues: &mut NetworkCommandQueues<'_>,
    ) -> Result<Option<NetworkQueuePass>, NetworkCommandError> {
        assert_eq!(context.frame, 0);
        let visits = self.visits.get() + 1;
        self.visits.set(visits);
        Ok(Some(if visits <= self.wait_visits {
            NetworkQueuePass::Waiting
        } else if self.skip {
            NetworkQueuePass::SkipDispatch
        } else {
            NetworkQueuePass::Dispatch
        }))
    }

    fn record_frame_checksum(&self, _frame: u32, _checksum: u32) {
        self.checksums.set(self.checksums.get() + 1);
    }

    fn compare_frame_info(
        &self,
        _record: &CommandRecord,
        _frame: i32,
    ) -> Result<bool, NetworkCommandError> {
        Ok(true)
    }
}

fn world() -> Simulation {
    let mut sim = Simulation::with_seed(73);
    let owner = sim.interner.intern("Human");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    sim.session.set_game_mode(ScenarioGameMode::Lan);
    sim.session.game_options_mut_for_test().game_speed = 3;
    assert!(
        sim.admit_native_command_record(
            GameSpeedRecord {
                house_id: 0,
                frame: 0,
                speed: 4
            }
            .encode()
            .unwrap()
        )
    );
    sim
}

#[test]
fn repeated_network_waits_resume_only_dispatch_and_commit_after_one_logic_visit() {
    let mut sim = world();
    let boundary = Boundary {
        visits: Cell::new(0),
        checksums: Cell::new(0),
        wait_visits: 2,
        skip: false,
    };
    let effects = FrameEffects::empty().with_network_commands(&boundary);
    let first = sim
        .advance_app_frame(
            &[],
            RuleSet::empty_for_test(),
            None,
            33,
            TickLane::Ordinary,
            effects,
        )
        .unwrap();
    assert!(first.network_waiting);
    assert!(!first.tick.frame_committed);
    assert_eq!(sim.session.tick(), 0);
    assert_eq!(sim.session.game_options().game_speed, 3);
    assert_eq!(
        first
            .lifecycle_outputs
            .iter()
            .filter(|output| matches!(output, crate::sim::world::LifecycleOutput::LogicVisit))
            .count(),
        1
    );
    assert!(
        sim.advance_app_frame(
            &[],
            RuleSet::empty_for_test(),
            None,
            33,
            TickLane::Ordinary,
            effects
        )
        .is_err()
    );
    // The suspended frame belongs to the snapshot, not to a transient callback.
    let bytes = serde_json::to_vec(&sim).unwrap();
    let mut sim: Simulation = serde_json::from_slice(&bytes).unwrap();
    let second = sim
        .resume_network_wait(RuleSet::empty_for_test(), None, effects)
        .unwrap();
    assert!(second.network_waiting);
    assert!(second.lifecycle_outputs.is_empty());
    assert_eq!(sim.session.tick(), 0);
    let mut third = sim
        .resume_network_wait(RuleSet::empty_for_test(), None, effects)
        .unwrap();
    assert!(!third.network_waiting);
    assert!(third.network_failure.is_none());
    assert!(third.tick.frame_committed);
    assert_eq!(sim.session.tick(), 1);
    assert_eq!(sim.session.game_options().game_speed, 4);
    assert_eq!(third.tick.executed_commands, 1);
    assert_eq!(third.take_admitted_commands().len(), 1);
    assert_eq!(boundary.checksums.get(), 1);
    assert_eq!(boundary.visits.get(), 3);
    assert!(!sim.pending_network_wait());
    assert!(
        sim.resume_network_wait(RuleSet::empty_for_test(), None, effects)
            .is_err()
    );
}

#[test]
fn natural_outcome_waits_for_terminal_network_dispatch_and_score_snapshot() {
    let mut sim = world();
    let owner = sim.interner.get("Human").unwrap();
    sim.session.set_current_house_for_test(Some(owner));
    let house = sim.houses.get_mut(&owner).unwrap();
    house.economy_mut().set_score_for_test(120);
    assert!(house.accept_win_result(0, 0, false));
    let boundary = Boundary {
        visits: Cell::new(0),
        checksums: Cell::new(0),
        wait_visits: 1,
        skip: false,
    };
    let effects = FrameEffects::empty().with_network_commands(&boundary);
    let waiting = sim
        .advance_app_frame(
            &[],
            RuleSet::empty_for_test(),
            None,
            33,
            TickLane::Ordinary,
            effects,
        )
        .unwrap();
    assert!(waiting.network_waiting);
    assert!(sim.session.outcome_request().is_some());
    assert!(sim.terminal_score_snapshot().is_none());
    assert!(sim.ready_outcome_for_owner(owner).is_none());
    assert_eq!(sim.session.game_options().game_speed, 3);
    let rng_before_score = sim.scenario_rng.state();

    let complete = sim
        .resume_network_wait(RuleSet::empty_for_test(), None, effects)
        .unwrap();
    assert!(!complete.network_waiting);
    assert!(!complete.tick.frame_committed);
    assert!(complete.tick.terminal_score_finalized);
    assert!(complete.tick.state_hash.is_some());
    assert_eq!(complete.tick.executed_commands, 1);
    assert_eq!(sim.session.game_options().game_speed, 4);
    assert_eq!(sim.session.binary_frame(), 0);
    assert_eq!(sim.terminal_score_snapshot().unwrap().rows.len(), 1);
    assert_ne!(sim.scenario_rng.state(), rng_before_score);
    assert_eq!(
        sim.ready_outcome_for_owner(owner).unwrap().kind,
        crate::sim::house_state::HouseOutcomeKind::Victory
    );
    assert_eq!(boundary.checksums.get(), 1);
}

#[test]
fn skipped_network_dispatch_commits_without_consuming_or_retiring_dolist() {
    let mut sim = world();
    let boundary = Boundary {
        visits: Cell::new(0),
        checksums: Cell::new(0),
        wait_visits: 0,
        skip: true,
    };
    let before = sim.pending_commands.clone();
    let output = sim
        .advance_app_frame(
            &[],
            RuleSet::empty_for_test(),
            None,
            33,
            TickLane::Ordinary,
            FrameEffects::empty().with_network_commands(&boundary),
        )
        .unwrap();
    assert!(output.tick.frame_committed);
    assert_eq!(output.tick.executed_commands, 0);
    assert_eq!(sim.session.game_options().game_speed, 3);
    assert_eq!(sim.pending_commands, before);
    assert_eq!(boundary.checksums.get(), 1);
}

#[test]
fn fatal_receive_failure_suspends_before_dispatch_and_acknowledges_only_frame_tail() {
    struct FullQueue;
    impl NetworkCommandBoundary for FullQueue {
        fn record_frame_checksum(&self, _: u32, _: u32) {}
        fn compare_frame_info(
            &self,
            _: &CommandRecord,
            _: i32,
        ) -> Result<bool, NetworkCommandError> {
            panic!("queue failure must precede DoList")
        }
        fn exchange_commands(
            &self,
            _: NetworkQueueContext,
            _: &mut NetworkCommandQueues<'_>,
        ) -> Result<Option<NetworkQueuePass>, NetworkCommandError> {
            Err(crate::sim::world::NetworkInterruption::QueueFull.into())
        }
    }
    let mut sim = world();
    let before = sim.pending_commands.clone();
    let output = sim
        .advance_app_frame(
            &[],
            RuleSet::empty_for_test(),
            None,
            33,
            TickLane::Ordinary,
            FrameEffects::empty().with_network_commands(&FullQueue),
        )
        .unwrap();
    assert!(!output.tick.frame_committed);
    assert!(!output.network_waiting);
    assert_eq!(sim.pending_commands, before);
    assert_eq!(
        sim.pending_network_interruption(),
        Some(crate::sim::world::NetworkInterruption::QueueFull)
    );
    let bytes = serde_json::to_vec(&sim).unwrap();
    let mut sim: Simulation = serde_json::from_slice(&bytes).unwrap();
    let output = sim
        .resume_after_network_failure(RuleSet::empty_for_test(), None, FrameEffects::empty())
        .unwrap();
    assert!(output.tick.frame_committed);
    assert_eq!(sim.session.binary_frame(), 1);
    assert_eq!(sim.session.game_options().game_speed, 3);
    assert_eq!(sim.pending_commands, before);
    assert!(sim.pending_network_interruption().is_none());
    assert!(
        sim.resume_after_network_failure(RuleSet::empty_for_test(), None, FrameEffects::empty())
            .is_err()
    );
}

#[test]
fn external_wait_timeout_or_quit_resumes_the_same_tail_without_dispatch() {
    for timeout in [false, true] {
        let mut sim = world();
        let boundary = Boundary {
            visits: Cell::new(0),
            checksums: Cell::new(0),
            wait_visits: 1,
            skip: false,
        };
        let output = sim
            .advance_app_frame(
                &[],
                RuleSet::empty_for_test(),
                None,
                33,
                TickLane::Ordinary,
                FrameEffects::empty().with_network_commands(&boundary),
            )
            .unwrap();
        assert!(output.network_waiting);
        let output = if timeout {
            assert!(
                sim.interrupt_network_wait(
                    crate::sim::world::NetworkInterruption::ConnectionFailed
                )
            );
            assert!(!sim.pending_network_wait());
            assert!(sim.pending_network_interruption().is_some());
            sim.resume_after_network_failure(RuleSet::empty_for_test(), None, FrameEffects::empty())
                .unwrap()
        } else {
            sim.leave_network_wait(RuleSet::empty_for_test(), None, FrameEffects::empty())
                .unwrap()
        };
        assert!(output.tick.frame_committed);
        assert!(output.tick.state_hash.is_some());
        assert_eq!(sim.session.binary_frame(), 1);
        assert_eq!(sim.session.game_options().game_speed, 3);
        assert_eq!(boundary.visits.get(), 1);
        assert_eq!(boundary.checksums.get(), 1);
        assert!(
            output
                .lifecycle_outputs
                .iter()
                .all(|output| !matches!(output, crate::sim::world::LifecycleOutput::LogicVisit))
        );
        assert!(
            !sim.interrupt_network_wait(crate::sim::world::NetworkInterruption::ConnectionFailed)
        );
        assert!(
            sim.leave_network_wait(RuleSet::empty_for_test(), None, FrameEffects::empty())
                .is_err()
        );
    }
}
