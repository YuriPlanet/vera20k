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
use crate::sim::world::{LocalDefeatBoundary, SimSoundEvent, Simulation, SoundBoundary};

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
}

impl LocalDefeatBoundary for FrameClientBoundary<'_> {
    fn leave_planning(
        &self,
        sim: &Simulation,
        rules: Option<&RuleSet>,
    ) -> Option<crate::sim::command::CommandEnvelope> {
        let mut input = self.planning.as_ref()?.borrow_mut();
        let (planning, selection) = &mut *input;
        planning.set_active(
            false,
            Some(sim),
            rules,
            selection,
            &mut self.output.borrow_mut(),
        )
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
