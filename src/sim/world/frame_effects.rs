//! Borrowed client effects at reached native call sites within one frame,
//! and the frame's bound resources.
//!
//! Process sound and local input owners implement these capabilities.
//! Simulation retains no callback, device or input state. Tooling/replay
//! explicitly omit client effects and consume their recorded commands.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::CommandEnvelope;

use super::{SimSoundEvent, Simulation};

/// Consume the newly reached prefix through the existing process sound owner.
/// Facts remain available to the other frame consumers. Implementations retain
/// only a delivered-prefix count, never the borrowed simulation or events.
pub trait SoundBoundary {
    fn consume(&self, sim: &Simulation, rules: &RuleSet, events: &[SimSoundEvent]);
    fn delivered(&self) -> usize;
}

/// The local player's Planning Mode (`0x00AC4CF4`), owned by client input.
/// House4FC1EE leaves it before the defeat announcement; its event joins the
/// frame's existing post-Logic dispatcher. TechnoClass::Select's planning-node
/// add (`0x00637840`) reads it. Headless/replay visits have no local planner:
/// Planning Mode is off and there is nothing to leave.
pub trait LocalPlanningBoundary {
    fn leave_planning(&self, sim: &Simulation, rules: &RuleSet) -> Option<CommandEnvelope>;
    fn planning_active(&self) -> bool;
}

/// Stack-scoped process effects accompanying this simulation visit.
#[derive(Clone, Copy, Default)]
pub struct FrameEffects<'a> {
    sound: Option<&'a dyn SoundBoundary>,
    local_planning: Option<&'a dyn LocalPlanningBoundary>,
    overlay_registry: Option<&'a OverlayTypeRegistry>,
}

impl<'a> FrameEffects<'a> {
    pub const fn empty() -> Self {
        Self {
            sound: None,
            local_planning: None,
            overlay_registry: None,
        }
    }

    pub fn for_sound(sound: &'a dyn SoundBoundary) -> Self {
        Self {
            sound: Some(sound),
            local_planning: None,
            overlay_registry: None,
        }
    }

    pub fn with_local_planning(self, boundary: &'a dyn LocalPlanningBoundary) -> Self {
        Self {
            local_planning: Some(boundary),
            ..self
        }
    }

    pub(crate) fn leave_local_planning(
        self,
        sim: &Simulation,
        rules: &RuleSet,
    ) -> Option<CommandEnvelope> {
        self.local_planning
            .and_then(|boundary| boundary.leave_planning(sim, rules))
    }

    /// Planning Mode byte `0x00AC4CF4`; off without a local planner.
    pub(crate) fn local_planning_active(self) -> bool {
        self.local_planning
            .is_some_and(LocalPlanningBoundary::planning_active)
    }

    /// Bind the match's OverlayTypeClass table for receivers the frame reaches
    /// without one of their own: the radio bus hands its receivers only the
    /// rules and these effects.
    pub(crate) fn with_overlay_registry(self, registry: Option<&'a OverlayTypeRegistry>) -> Self {
        Self {
            overlay_registry: registry.or(self.overlay_registry),
            ..self
        }
    }

    /// The overlay table [`Self::with_overlay_registry`] bound, if any.
    pub(crate) const fn overlay_registry(self) -> Option<&'a OverlayTypeRegistry> {
        self.overlay_registry
    }

    /// Stop/Release must complete at their native call sites before a later
    /// Main draw or object visit. The consumer also delivers earlier sound
    /// facts first, preserving the existing ordered producer channel.
    pub(crate) fn flush_sound(self, sim: &Simulation, rules: &RuleSet) {
        if let Some(sound) = self.sound {
            sound.consume(sim, rules, &sim.sound_events);
            assert_eq!(sound.delivered(), sim.sound_events.len());
        }
    }

    pub(crate) fn delivered_sound_events(self) -> usize {
        self.sound.map_or(0, SoundBoundary::delivered)
    }
}
