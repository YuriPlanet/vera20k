//! A tick that borrows no app state (docs/sim-thread.md, phase 1).
//!
//! [`super::frame_effects::FrameClientBoundary`] lends the tick the app's
//! sound, radar, planning, sidebar and outcome owners, so the tick must run
//! where they live. [`DeferredClientEffects`] instead carries what the tick
//! reads from them, taken before it starts, and records what it would write.
//! [`DeferredClientEffects::apply`] then writes the record on the main
//! thread, in order. The simulation thread of phase 4 runs ticks this way.
//!
//! Only presentation moves: sound plays after the tick through the frame
//! tail, and the radar cue, the sidebar request clear and Ares bounty text
//! follow the tick instead of interleaving with it. What the tick itself
//! reads keeps its value:
//! - the planning flag, taken before the tick; leaving Planning Mode issues
//!   the same PLANCOMMIT;
//! - the outcome-voice wait, polled on a copy with the voice state taken
//!   before the tick. That wait moves only wall time, never a frame's
//!   result or hash.
//!
//! A network frame stops at Queue_AI instead
//! ([`DeferredClientEffects::hands_off_queue`]): the live session stays on
//! the main thread, which resumes the frame there as it resumes a network
//! wait. Its checksum is recorded for the main thread's history before that
//! exchange compares FRAMEINFO against it.
//!
//! Replay, a campaign's initial save and command observation still need the
//! lent owners, so those frames stay on the borrowing path.

use std::cell::{Cell, RefCell};

use crate::app::AppState;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{CommandEnvelope, CommandRecord};
use crate::sim::intern::InternedId;
use crate::sim::world::{
    LocalPlanningBoundary, NetworkCommandBoundary, NetworkCommandError, NetworkCommandQueues,
    NetworkQueueContext, NetworkQueuePass, OutcomeWaitBoundary, Simulation, TacticalRadarBoundary,
};

use super::scenario_exit::ScenarioOutcomeVoiceWait;

/// One write the tick asked of a client owner, in the order it asked.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DeferredEffect {
    /// `Radar656DF0`: the local radar became available or unavailable.
    TacticalRadar(bool),
    /// `Planning__EndMode`: the tick left Planning Mode.
    PlanningLeft,
    /// `Strip6A8F42`: the production strip's request for this type cleared.
    ProductionRequestCleared(InternedId),
    /// Ares bounty text reached its native call.
    #[cfg(feature = "ares")]
    Bounty(crate::sim::bounty::BountyText),
    /// `Queue_AI_Multiplayer` stored this frame's checksum (history
    /// `0x00B04474[Frame & 0xFF]`).
    FrameChecksum { frame: u32, checksum: u32 },
}

/// What a tick reads from the client, taken before it, and what it writes,
/// recorded for [`Self::apply`].
#[derive(Debug)]
pub(crate) struct DeferredClientEffects {
    planning_active: Cell<bool>,
    voices_active: bool,
    defer_verdict_to_clip: bool,
    queue_handoff: bool,
    outcome_wait: RefCell<Option<ScenarioOutcomeVoiceWait>>,
    recorded: RefCell<Vec<DeferredEffect>>,
}

impl DeferredClientEffects {
    /// Whether the next ordinary offline frame can run without the lent
    /// owners. The simulation thread ticks this way, and so does every such
    /// frame it does not take, wherever the thread exists. A network frame
    /// needs its exchange, which only [`Self::hands_off_queue`] leaves to the
    /// main thread.
    pub(crate) fn applies(
        state: &AppState,
        replaying: bool,
        observing_commands: bool,
        initial_save: bool,
    ) -> bool {
        state.match_state.sim_runtime.thread_enabled()
            && !replaying
            && !observing_commands
            && !initial_save
            && state.match_state.live_network.is_none()
            && !state
                .match_state
                .sim_runtime
                .runtime()
                .is_some_and(|runtime| runtime.simulation.session().game_mode().is_network())
    }

    /// Take what the tick will read from the client owners. With
    /// `queue_handoff` the tick stops at Queue_AI ([`Self::hands_off_queue`]).
    pub(crate) fn take(
        planning_active: bool,
        sfx: Option<&mut crate::audio::sfx::SfxPlayer>,
        assets: &crate::app::process_assets::ProcessAssets,
        outcome_wait: &Option<ScenarioOutcomeVoiceWait>,
        defer_verdict_to_clip: bool,
        queue_handoff: bool,
    ) -> Self {
        let voices_active = match (sfx, assets.manager(), assets.audio_catalog()) {
            (Some(sfx), Some(manager), Some(catalog)) => {
                sfx.pump_and_check_voices(catalog.eva(), manager, catalog.index())
            }
            _ => false,
        };
        Self {
            planning_active: Cell::new(planning_active),
            voices_active,
            defer_verdict_to_clip,
            queue_handoff,
            outcome_wait: RefCell::new(outcome_wait.clone()),
            recorded: RefCell::new(Vec::new()),
        }
    }

    /// Whether this network frame waits at Queue_AI for the main thread,
    /// where the live session runs the exchange and the frame's rest.
    pub(crate) fn hands_off_queue(&self) -> bool {
        self.queue_handoff
    }

    pub(crate) fn record(&self, effect: DeferredEffect) {
        self.recorded.borrow_mut().push(effect);
    }

    /// The writes recorded so far, in order.
    pub(crate) fn recorded(&self) -> std::cell::Ref<'_, Vec<DeferredEffect>> {
        self.recorded.borrow()
    }

    /// Write the record to the client owners on the main thread, after the
    /// tick, in the order the tick asked.
    pub(crate) fn apply(self, state: &mut AppState) {
        state.match_state.scenario_outcome = self.outcome_wait.into_inner();
        for effect in self.recorded.into_inner() {
            match effect {
                DeferredEffect::TacticalRadar(available) => set_tactical_radar(state, available),
                DeferredEffect::PlanningLeft => {
                    let rules = state
                        .match_state
                        .sim_runtime
                        .runtime()
                        .map(|rt| &rt.resources.rules);
                    // The tick already issued the PLANCOMMIT; without a world
                    // this leaves the mode and queues EndPlanningModeSound only.
                    let input = &mut state.match_state.input;
                    let _ = input.planning.set_active(
                        false,
                        None,
                        rules,
                        &mut input.selection_state,
                        &mut state.match_state.match_audio.sound_events,
                    );
                }
                DeferredEffect::ProductionRequestCleared(type_id) => state
                    .match_state
                    .match_presentation
                    .sidebar_projection
                    .clear_production_request(type_id),
                #[cfg(feature = "ares")]
                DeferredEffect::Bounty(text) => {
                    if let Some(rt) = state.match_state.sim_runtime.runtime() {
                        state
                            .match_state
                            .match_presentation
                            .flying_credits
                            .borrow_mut()
                            .admit(&rt.simulation, &rt.resources.rules, text);
                    }
                }
                DeferredEffect::FrameChecksum { frame, checksum } => {
                    if let Some((_, checksums)) = state.match_state.sim_runtime.tick_parts() {
                        checksums.record_frame_checksum(frame, checksum);
                    }
                }
            }
        }
    }
}

/// The client half of `Radar656DF0`, as `FrameClientBoundary` performs it.
fn set_tactical_radar(state: &mut AppState, available: bool) {
    let presentation = &mut state.match_state.match_presentation;
    presentation.has_radar = available;
    let Some(cue) = presentation
        .radar_anim
        .as_mut()
        .and_then(|radar| radar.set_has_radar(available))
    else {
        return;
    };
    let Some(rt) = state.match_state.sim_runtime.runtime() else {
        return;
    };
    if let Some(sfx) = state.audio.sfx_player.as_mut() {
        crate::app::radar_movie::end_radar_sound_loop_with(sfx);
    }
    if let Some(sound) =
        crate::app::presentation::building_anim::radar_availability_sound(&rt.resources.rules, cue)
    {
        state.match_state.match_audio.sound_events.push(sound);
    }
}

impl TacticalRadarBoundary for DeferredClientEffects {
    fn set_available(&self, _sim: &Simulation, _rules: &RuleSet, available: bool) {
        self.record(DeferredEffect::TacticalRadar(available));
    }
}

impl LocalPlanningBoundary for DeferredClientEffects {
    fn leave_planning(&self, sim: &Simulation, _rules: &RuleSet) -> Option<CommandEnvelope> {
        if !self.planning_active.replace(false) {
            return None;
        }
        self.record(DeferredEffect::PlanningLeft);
        crate::app::input::planning_mode::plan_commit_command(sim)
    }

    fn planning_active(&self) -> bool {
        self.planning_active.get()
    }
}

impl OutcomeWaitBoundary for DeferredClientEffects {
    fn wait_complete(
        &self,
        _owner: InternedId,
        kind: crate::sim::house_state::HouseOutcomeKind,
    ) -> bool {
        let mut pending = self.outcome_wait.borrow_mut();
        // As FrameClientBoundary: a line held for the commander's clip waits
        // on a frozen clip, so the wait ends now. Only wall time changes.
        if self.defer_verdict_to_clip {
            *pending = None;
            return true;
        }
        ScenarioOutcomeVoiceWait::poll_pending(
            &mut pending,
            kind,
            super::frame_pacer::wall_clock_ms(),
            self.voices_active,
        )
    }
}

/// Queue_AI on the simulation thread: the frame suspends after Logic and its
/// checksum, before any transfer or dispatch, and the main thread resumes it
/// through the live session ([`super::frame_effects::FrameClientBoundary`]).
impl NetworkCommandBoundary for DeferredClientEffects {
    fn exchange_commands(
        &self,
        _context: NetworkQueueContext,
        _queues: &mut NetworkCommandQueues<'_>,
    ) -> Result<Option<NetworkQueuePass>, NetworkCommandError> {
        Ok(Some(NetworkQueuePass::Waiting))
    }

    fn record_frame_checksum(&self, frame: u32, checksum: u32) {
        self.record(DeferredEffect::FrameChecksum { frame, checksum });
    }

    fn compare_frame_info(
        &self,
        _record: &CommandRecord,
        _current_frame: i32,
    ) -> Result<bool, NetworkCommandError> {
        Err(NetworkCommandError::Unavailable(
            "FRAMEINFO compares after the main thread's exchange".to_owned(),
        ))
    }
}

// Phase 4 moves the record to the simulation thread and back.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<DeferredClientEffects>();
};

#[cfg(test)]
mod tests {
    use super::*;

    fn deferred(planning_active: bool, defer_verdict_to_clip: bool) -> DeferredClientEffects {
        DeferredClientEffects {
            planning_active: Cell::new(planning_active),
            voices_active: false,
            defer_verdict_to_clip,
            queue_handoff: false,
            outcome_wait: RefCell::new(None),
            recorded: RefCell::new(Vec::new()),
        }
    }

    #[test]
    fn records_client_writes_in_order_and_leaves_planning_once() {
        let mut sim = Simulation::with_seed(7);
        let owner = sim.intern("Human");
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 0, 10,
            ));
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        let rules = RuleSet::empty_for_test();
        let effects = deferred(true, false);

        effects.set_available(&sim, rules, false);
        assert!(effects.planning_active());
        let commit = effects.leave_planning(&sim, rules);
        assert_eq!(
            commit,
            crate::app::input::planning_mode::plan_commit_command(&sim),
            "the same PLANCOMMIT the lent planner issues"
        );
        assert!(!effects.planning_active());
        assert_eq!(effects.leave_planning(&sim, rules), None);
        effects.set_available(&sim, rules, true);
        assert_eq!(
            *effects.recorded(),
            [
                DeferredEffect::TacticalRadar(false),
                DeferredEffect::PlanningLeft,
                DeferredEffect::TacticalRadar(true),
            ]
        );
    }

    #[test]
    fn a_clip_held_verdict_ends_the_outcome_wait_on_the_copy() {
        let effects = deferred(false, true);
        *effects.outcome_wait.borrow_mut() = Some(ScenarioOutcomeVoiceWait::start(
            0,
            crate::sim::house_state::HouseOutcomeKind::Victory,
        ));
        assert!(effects.wait_complete(
            InternedId::default(),
            crate::sim::house_state::HouseOutcomeKind::Victory
        ));
        assert!(effects.outcome_wait.borrow().is_none());
    }

    /// The simulation thread's part of a network frame stops at Queue_AI
    /// with its checksum recorded; the main thread's resumed exchange then
    /// compares the peer's FRAMEINFO against that checksum.
    #[test]
    fn a_handed_off_network_frame_compares_the_checksum_its_logic_recorded() {
        use crate::sim::world::{FrameEffects, TickLane};
        let world = super::super::frame_effects::network_command_tests::frame_256_runtime;
        let expected = world()
            .advance_frame(&[], 33, TickLane::Ordinary, FrameEffects::empty())
            .unwrap()
            .multiplayer_checksum
            .unwrap()
            .checksum
            .unwrap()
            .value;
        let mut runtime = world();
        let peer = crate::net::lockstep::FrameInfo {
            house_id: 0,
            event_frame: 256,
            checksum: expected ^ 1,
            timing_word: 0,
            delay: 0,
        };
        runtime
            .simulation
            .admit_native_command_record(peer.encode());
        let mut effects = deferred(false, false);
        effects.queue_handoff = true;

        let logic = runtime
            .advance_frame(
                &[],
                33,
                TickLane::Ordinary,
                FrameEffects::empty().with_network_commands(&effects),
            )
            .unwrap();
        assert!(logic.network_waiting);
        assert!(!logic.tick.frame_committed);
        assert_eq!(
            *effects.recorded(),
            [DeferredEffect::FrameChecksum {
                frame: 256,
                checksum: expected
            }]
        );

        // `apply` stores the recorded checksum in the host's history.
        let mut checksums = crate::sim::command_queue::FrameChecksumState::default();
        checksums.record_frame_checksum(0, 0);
        checksums.record_frame_checksum(256, expected);
        let mut sounds = crate::audio::events::SoundEventQueue::new();
        let mut radar = |_, _, _: &RuleSet| false;
        let boundary = super::super::frame_effects::FrameClientBoundary::new(
            None,
            Some("Human"),
            &mut sounds,
            &mut radar,
        )
        .with_frame_checksums(&mut checksums);
        let rest = runtime
            .resume_network_wait(
                FrameEffects::for_sound(&boundary).with_network_commands(&boundary),
            )
            .unwrap();
        let failure = rest
            .network_failure
            .expect("the peer's FRAMEINFO differs from this frame's checksum");
        assert_eq!(failure.local_checksum, expected);
        assert_eq!(failure.remote_checksum, expected ^ 1);
    }
}
