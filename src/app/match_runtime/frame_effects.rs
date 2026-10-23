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
    LocalPlanningBoundary, NetworkCommandBoundary, NetworkCommandError, OutcomeWaitBoundary,
    SimSoundEvent, Simulation, SoundBoundary,
};

#[cfg(test)]
#[path = "planning_defeat_tests.rs"]
mod planning_defeat_tests;

#[cfg(test)]
#[path = "outcome_wait_tests.rs"]
mod outcome_wait_tests;

/// A frame-local borrow of the existing app/audio owners. Native Gattling
/// Stop405D40 must finish before Report's Main65C780 draw; Update70E000 and
/// Limbo6F6C6B similarly release at their reached boundaries. No callback or
/// simulation reference survives this scope.
/// Source: `tools/input_oracle/gattling_loop.json` and its native harness.
type RadarAdmission<'a> =
    dyn FnMut(crate::sim::radar::RadarEventRequest, u64, &RuleSet) -> bool + 'a;

struct ScenarioPauseOwners<'a> {
    clock: &'a mut super::scenario_pause::ScenarioPauseClock,
    elapsed: &'a RefCell<&'a mut super::frame_pacer::ScenarioElapsedClock>,
    mode: super::session::SessionMode,
    other_local_pause: bool,
}

pub(crate) struct FrameClientBoundary<'a> {
    delivered: Cell<usize>,
    playback: RefCell<Option<(&'a mut SfxPlayer, SoundPlaybackInputs<'a>)>>,
    local_owner: Option<&'a str>,
    output: RefCell<&'a mut SoundEventQueue>,
    admit_radar: RefCell<&'a mut RadarAdmission<'a>>,
    planning: Option<RefCell<(&'a mut PlanningInputState, &'a mut SelectionState)>>,
    network: Option<RefCell<&'a mut super::session::MatchSession>>,
    live_network: Option<RefCell<&'a mut crate::net::live_commands::LiveCommandSession>>,
    outcome_wait: Option<RefCell<&'a mut Option<super::scenario_exit::ScenarioOutcomeVoiceWait>>>,
    scenario_pause: Option<RefCell<ScenarioPauseOwners<'a>>>,
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
            live_network: None,
            outcome_wait: None,
            scenario_pause: None,
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

    pub(crate) fn with_live_network(
        mut self,
        live: Option<&'a mut crate::net::live_commands::LiveCommandSession>,
    ) -> Self {
        self.live_network = live.map(RefCell::new);
        self
    }

    pub(crate) fn with_outcome_wait(
        mut self,
        pending: &'a mut Option<super::scenario_exit::ScenarioOutcomeVoiceWait>,
    ) -> Self {
        self.outcome_wait = Some(RefCell::new(pending));
        self
    }

    pub(crate) fn with_scenario_pause(
        mut self,
        clock: &'a mut super::scenario_pause::ScenarioPauseClock,
        elapsed: &'a RefCell<&'a mut super::frame_pacer::ScenarioElapsedClock>,
        mode: super::session::SessionMode,
        other_local_pause: bool,
    ) -> Self {
        self.scenario_pause = Some(RefCell::new(ScenarioPauseOwners {
            clock,
            elapsed,
            mode,
            other_local_pause,
        }));
        self
    }

    fn apply_prefix_effect_at(
        &self,
        effect: &crate::sim::trigger_runtime::TriggerEffect,
        wall_ms: u64,
    ) -> bool {
        use crate::sim::trigger_runtime::TriggerEffect;
        if !matches!(
            effect,
            TriggerEffect::ScenarioPause { .. } | TriggerEffect::ScenarioResume
        ) {
            return false;
        }
        let Some(owners) = &self.scenario_pause else {
            return false;
        };
        let mut owners = owners.borrow_mut();
        let ScenarioPauseOwners {
            clock,
            elapsed,
            mode,
            other_local_pause,
        } = &mut *owners;
        let mut elapsed = elapsed.borrow_mut();
        let mut playback = self.playback.borrow_mut();
        let sfx = playback.as_mut().map(|(sfx, _)| &mut **sfx);
        match *effect {
            TriggerEffect::ScenarioPause {
                duration,
                outermost,
            } => {
                super::scenario_pause::enter_with_owners(
                    clock,
                    &mut elapsed,
                    *mode,
                    sfx,
                    duration,
                    outermost,
                    wall_ms,
                );
            }
            TriggerEffect::ScenarioResume => super::scenario_pause::resume_with_owners(
                &mut elapsed,
                sfx,
                *other_local_pause,
                wall_ms,
            ),
            _ => unreachable!(),
        }
        true
    }
}

impl OutcomeWaitBoundary for FrameClientBoundary<'_> {
    fn apply_prefix_effect(&self, effect: &crate::sim::trigger_runtime::TriggerEffect) -> bool {
        self.apply_prefix_effect_at(effect, super::frame_pacer::wall_clock_ms())
    }

    fn wait_complete(
        &self,
        _owner: crate::sim::intern::InternedId,
        kind: crate::sim::house_state::HouseOutcomeKind,
    ) -> bool {
        let Some(pending) = &self.outcome_wait else {
            return true;
        };
        let wall_ms = super::frame_pacer::wall_clock_ms();
        // The outer event loop services the existing AudioServiceClock and
        // network owners between visits. This is only native7529E0's Vox poll.
        let voices_active = self
            .playback
            .borrow_mut()
            .as_mut()
            .is_some_and(|(sfx, inputs)| {
                sfx.pump_and_check_voices(
                    inputs.catalog.eva(),
                    inputs.assets,
                    inputs.catalog.index(),
                )
            });
        super::scenario_exit::ScenarioOutcomeVoiceWait::poll_pending(
            &mut pending.borrow_mut(),
            kind,
            wall_ms,
            voices_active,
        )
    }
}

impl NetworkCommandBoundary for FrameClientBoundary<'_> {
    fn exchange_commands(
        &self,
        context: crate::sim::world::NetworkQueueContext,
        queues: &mut crate::sim::world::NetworkCommandQueues<'_>,
    ) -> Result<Option<crate::sim::world::NetworkQueuePass>, NetworkCommandError> {
        use crate::net::command_exchange::CommandExchangeError;
        use crate::net::event_packet::DecodeFailureKind;
        use crate::net::live_commands::LiveCommandError;
        use crate::sim::world::NetworkInterruption;
        let Some(live) = &self.live_network else {
            return Ok(None);
        };
        let max_ahead = queues.max_ahead_frames().ok_or_else(|| {
            NetworkCommandError::Unavailable("live network has no admitted MaxAhead".to_owned())
        })?;
        let now_ms = super::frame_pacer::wall_clock_ms();
        let now = super::frame_pacer::frame_bucket(now_ms);
        live.borrow_mut()
            .poll(queues, context, max_ahead, now, now_ms as u32)
            .map(Some)
            .map_err(|error| {
                log::error!("live network command exchange: {error}");
                match error {
                    LiveCommandError::Exchange(CommandExchangeError::InitialChecksum {
                        ..
                    }) => NetworkInterruption::ScenariosDiffer.into(),
                    LiveCommandError::Exchange(CommandExchangeError::Decode(error))
                        if error.kind == DecodeFailureKind::AdmissionFull =>
                    {
                        NetworkInterruption::QueueFull.into()
                    }
                    error => NetworkCommandError::Unavailable(error.to_string()),
                }
            })
    }

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

    fn update_looping_sound(
        &self,
        sim: &Simulation,
        owner: u64,
        world: crate::sim::anim_class::AnimWorldCoord,
    ) {
        if let Some((sfx, inputs)) = self.playback.borrow_mut().as_mut() {
            inputs.update_looping_sound(sfx, sim, self.local_owner, owner, world);
        }
    }
}

#[cfg(test)]
mod radar_persistence_tests {
    use super::*;
    use crate::render::radar::{ClientRadarEvents, saved_radar_fixture};
    use crate::sim::radar::{RadarEventRequest, RadarEventType};

    #[test]
    fn same_frame_admission_does_not_tick_restored_alerts_twice() {
        let saved = saved_radar_fixture();
        let retained_count = saved.events.len();
        let mut restored = ClientRadarEvents::from_saved(saved.clone()).unwrap();
        let mut uninterrupted = ClientRadarEvents::from_saved(saved).unwrap();
        let mut sim = Simulation::with_seed(73);
        sim.session_mut_for_test().set_tick_for_test(100);
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        let mut sounds = SoundEventQueue::new();
        {
            let mut admit = |request, frame, rules: &RuleSet| {
                restored.admit(
                    request,
                    frame,
                    Some(((50, 50), (140, 108))),
                    &rules.radar_event_config,
                )
            };
            let boundary = FrameClientBoundary::new(None, None, &mut sounds, &mut admit);
            // Action55 flushes this boundary before complete_tick increments
            // the simulation frame, which may already have been presented.
            boundary.consume(
                &sim,
                &rules,
                &[SimSoundEvent::RadarEvent {
                    radar: RadarEventRequest::new(RadarEventType::Noncombat, 147, 99),
                }],
            );
            assert_eq!(boundary.delivered(), 1);
        }
        restored.advance_to_frame(101, &rules.radar_event_config);
        uninterrupted.advance_to_frame(101, &rules.radar_event_config);
        let after = restored.saved();
        assert_eq!(after.events.len(), retained_count + 1);
        assert_eq!(
            after.events[..retained_count],
            uninterrupted.saved().events,
            "a later alert must not replay a retained alert's previous animation frame"
        );
        assert_eq!(after.last_advanced_frame, Some(101));
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
