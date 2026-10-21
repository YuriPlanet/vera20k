//! Frame-scoped adaptation of existing client input and audio owners.
//! Native House4FC1EE calls the planner before the outcome sound boundary;
//! both use the same queue, and no client borrow survives advance_frame.

use std::cell::{Cell, RefCell};

use super::sound_dispatch::dispatch_sim_sound_events;
use crate::app::input::planning_mode::PlanningInputState;
use crate::app::presentation::building_anim::SoundPlaybackInputs;
use crate::audio::events::SoundEventQueue;
use crate::audio::sfx::SfxPlayer;
use crate::rules::ruleset::RuleSet;
use crate::sim::selection::SelectionState;
use crate::sim::world::{
    LocalPlanningBoundary, NetworkCommandBoundary, NetworkCommandError, SimSoundEvent, Simulation,
    SoundBoundary,
};

#[cfg(test)]
#[path = "planning_defeat_tests.rs"]
mod planning_defeat_tests;

/// A frame-local borrow of the existing app/audio owners. Native Gattling
/// Stop405D40 must finish before Report's Main65C780 draw; Update70E000 and
/// Limbo6F6C6B similarly release at their reached boundaries. No callback or
/// simulation reference survives this scope.
/// Source: `tools/input_oracle/gattling_loop.json` and its native harness.
type RadarAdmission<'a> =
    dyn FnMut(crate::sim::radar::RadarEventRequest, u64, &RuleSet) -> bool + 'a;

pub(crate) struct FrameClientBoundary<'a> {
    delivered: Cell<usize>,
    playback: RefCell<Option<(&'a mut SfxPlayer, SoundPlaybackInputs<'a>)>>,
    local_owner: Option<&'a str>,
    output: RefCell<&'a mut SoundEventQueue>,
    admit_radar: RefCell<&'a mut RadarAdmission<'a>>,
    planning: Option<RefCell<(&'a mut PlanningInputState, &'a mut SelectionState)>>,
    network: Option<RefCell<&'a mut super::session::MatchSession>>,
}

impl<'a> FrameClientBoundary<'a> {
    pub(crate) fn new(
        playback: Option<(&'a mut SfxPlayer, SoundPlaybackInputs<'a>)>,
        local_owner: Option<&'a str>,
        output: &'a mut SoundEventQueue,
        admit_radar: &'a mut RadarAdmission<'a>,
    ) -> Self {
        Self {
            delivered: Cell::new(0),
            playback: RefCell::new(playback),
            local_owner,
            output: RefCell::new(output),
            admit_radar: RefCell::new(admit_radar),
            planning: None,
            network: None,
        }
    }

    pub(crate) fn with_planning(
        mut self,
        planning: &'a mut PlanningInputState,
        selection: &'a mut SelectionState,
    ) -> Self {
        self.planning = Some(RefCell::new((planning, selection)));
        self
    }

    pub(crate) fn with_network_session(
        mut self,
        session: &'a mut super::session::MatchSession,
    ) -> Self {
        self.network = Some(RefCell::new(session));
        self
    }
}

impl NetworkCommandBoundary for FrameClientBoundary<'_> {
    fn record_frame_checksum(&self, frame: u32, checksum: u32) {
        if let Some(network) = &self.network {
            network.borrow_mut().record_frame_checksum(frame, checksum);
        }
    }

    fn compare_frame_info(
        &self,
        record: &crate::sim::command::CommandRecord,
        current_frame: i32,
    ) -> Result<bool, NetworkCommandError> {
        let network = self.network.as_ref().ok_or_else(|| {
            NetworkCommandError::Unavailable(
                "FRAMEINFO requires the network session checksum owner".to_owned(),
            )
        })?;
        let info = crate::net::lockstep::FrameInfo::decode(record).ok_or_else(|| {
            NetworkCommandError::Unavailable(
                "network comparison received a non-FRAMEINFO record".to_owned(),
            )
        })?;
        network
            .borrow()
            .compare_frame_info(info, current_frame)
            .map_err(NetworkCommandError::Mismatch)
    }
}

impl LocalPlanningBoundary for FrameClientBoundary<'_> {
    fn leave_planning(
        &self,
        sim: &Simulation,
        rules: &RuleSet,
    ) -> Option<crate::sim::command::CommandEnvelope> {
        let mut input = self.planning.as_ref()?.borrow_mut();
        let (planning, selection) = &mut *input;
        planning.set_active(
            false,
            Some(sim),
            Some(rules),
            selection,
            &mut self.output.borrow_mut(),
        )
    }

    fn planning_active(&self) -> bool {
        self.planning
            .as_ref()
            .is_some_and(|input| input.borrow().0.active())
    }
}

impl SoundBoundary for FrameClientBoundary<'_> {
    fn consume(&self, sim: &Simulation, rules: &RuleSet, events: &[SimSoundEvent]) {
        let delivered = self.delivered.get();
        let pending = events
            .get(delivered..)
            .expect("frame sound prefix shrank after synchronous delivery");
        if pending.is_empty() {
            return;
        }
        let mut output = self.output.borrow_mut();
        {
            let mut admit_radar = self.admit_radar.borrow_mut();
            dispatch_sim_sound_events(
                pending.iter().cloned(),
                sim,
                rules,
                self.local_owner,
                &mut |request| (*admit_radar)(request, sim.session().tick(), rules),
                &mut output,
            );
        }
        let events = output.drain();
        if let Some((sfx, inputs)) = self.playback.borrow_mut().as_mut() {
            inputs.consume(sfx, &events, Some(sim), self.local_owner);
        }
        self.delivered.set(delivered + pending.len());
    }

    fn delivered(&self) -> usize {
        self.delivered.get()
    }
}

#[cfg(test)]
mod network_command_tests {
    use super::*;
    use crate::sim::house_state::HouseState;
    use crate::sim::runtime::SimRuntime;
    use crate::sim::world::{FrameEffects, TickLane};

    fn frame_256_runtime() -> SimRuntime {
        let mut sim = Simulation::with_seed(73);
        let owner = sim.intern("Human");
        sim.houses_mut_for_test()
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        sim.session_mut_for_test()
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Wol);
        sim.session_mut_for_test().set_tick_for_test(256);
        sim.session_mut_for_test().set_binary_frame_for_test(256);
        SimRuntime::from_simulation(sim)
    }

    #[test]
    fn production_boundary_publishes_current_checksum_before_comparison_and_resumes_once() {
        let control = frame_256_runtime()
            .advance_frame(&[], 33, TickLane::Ordinary, FrameEffects::empty())
            .unwrap();
        let expected = control
            .multiplayer_checksum
            .unwrap()
            .checksum
            .unwrap()
            .value;
        let mut runtime = frame_256_runtime();
        let mut session = crate::app::match_runtime::session::MatchSession::default();
        session.record_frame_checksum(0, 0);
        session.record_frame_checksum(255, expected ^ 2);
        let info = crate::net::lockstep::FrameInfo {
            house_id: 0,
            event_frame: 256,
            checksum: expected ^ 1,
            timing_word: 0,
            delay: 0,
        };
        runtime
            .simulation
            .admit_native_command_record(info.encode());
        let mut sounds = SoundEventQueue::new();
        let mut radar = |_, _, _: &RuleSet| false;
        let prefix = {
            let boundary = FrameClientBoundary::new(None, Some("Human"), &mut sounds, &mut radar)
                .with_network_session(&mut session);
            runtime
                .advance_frame(
                    &[],
                    33,
                    TickLane::Ordinary,
                    FrameEffects::for_sound(&boundary).with_network_commands(&boundary),
                )
                .unwrap()
        };
        let failure = prefix
            .network_failure
            .expect("real session comparator rejects the peer CRC");
        assert_eq!(failure.local_checksum, expected);
        assert_eq!(failure.remote_checksum, expected ^ 1);
        assert_eq!(session.checksum_history().get(256), expected);
        assert_eq!(runtime.simulation.session().binary_frame(), 256);
        assert!(!prefix.tick.frame_committed);
        let suffix = runtime
            .resume_after_network_failure(FrameEffects::empty())
            .unwrap();
        assert!(suffix.tick.frame_committed);
        assert_eq!(runtime.simulation.session().binary_frame(), 257);
        assert!(
            !suffix
                .lifecycle_outputs
                .iter()
                .any(|event| matches!(event, crate::sim::world::LifecycleOutput::LogicVisit))
        );
        assert!(
            runtime
                .resume_after_network_failure(FrameEffects::empty())
                .is_err()
        );
    }
}
