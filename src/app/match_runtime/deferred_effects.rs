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
//! Network, replay, a campaign's initial save and command observation still
//! need the lent owners, so [`DeferredClientEffects::applies`] keeps those
//! frames on the borrowing path.

use std::cell::{Cell, RefCell};

use crate::app::AppState;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::CommandEnvelope;
use crate::sim::intern::InternedId;
use crate::sim::world::{
    LocalPlanningBoundary, OutcomeWaitBoundary, Simulation, TacticalRadarBoundary,
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
}

/// What a tick reads from the client, taken before it, and what it writes,
/// recorded for [`Self::apply`].
#[derive(Debug)]
pub(crate) struct DeferredClientEffects {
    planning_active: Cell<bool>,
    voices_active: bool,
    defer_verdict_to_clip: bool,
    outcome_wait: RefCell<Option<ScenarioOutcomeVoiceWait>>,
    recorded: RefCell<Vec<DeferredEffect>>,
}

impl DeferredClientEffects {
    /// Whether the next ordinary frame can run without the lent owners.
    /// `VERA20K_DEFERRED_TICK_EFFECTS=1` opts in until phase 4 makes it the
    /// simulation thread's path.
    pub(crate) fn applies(
        state: &AppState,
        replaying: bool,
        observing_commands: bool,
        initial_save: bool,
    ) -> bool {
        std::env::var_os("VERA20K_DEFERRED_TICK_EFFECTS").is_some_and(|value| value == "1")
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

    /// Take what the tick will read from the client owners.
    pub(crate) fn take(
        planning_active: bool,
        sfx: Option<&mut crate::audio::sfx::SfxPlayer>,
        assets: &crate::app::process_assets::ProcessAssets,
        outcome_wait: &Option<ScenarioOutcomeVoiceWait>,
        defer_verdict_to_clip: bool,
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
            outcome_wait: RefCell::new(outcome_wait.clone()),
            recorded: RefCell::new(Vec::new()),
        }
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
}
