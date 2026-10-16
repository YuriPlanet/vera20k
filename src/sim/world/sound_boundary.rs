//! Borrowed effects at native sound-handle boundaries within one frame, and
//! the frame's bound resources.
//!
//! The process sound owner implements this capability. Simulation retains no
//! callback or device state, and tooling/replay explicitly omit it.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;

use super::{SimSoundEvent, Simulation};

/// Consume the newly reached prefix through the existing process sound owner.
/// Facts remain available to the other frame consumers. Implementations retain
/// only a delivered-prefix count, never the borrowed simulation or events.
pub trait SoundBoundary {
    fn consume(&self, sim: &Simulation, rules: &RuleSet, events: &[SimSoundEvent]);
    fn delivered(&self) -> usize;
}

/// Stack-scoped process effects accompanying this simulation visit.
#[derive(Clone, Copy, Default)]
pub struct FrameEffects<'a> {
    sound: Option<&'a dyn SoundBoundary>,
    rules: Option<&'a RuleSet>,
    overlay_registry: Option<&'a OverlayTypeRegistry>,
}

impl<'a> FrameEffects<'a> {
    pub const fn empty() -> Self {
        Self {
            sound: None,
            rules: None,
            overlay_registry: None,
        }
    }

    pub fn for_sound(sound: &'a dyn SoundBoundary) -> Self {
        Self {
            sound: Some(sound),
            rules: None,
            overlay_registry: None,
        }
    }

    /// Bind the frame's authoritative resources even when a nested native
    /// lifecycle call needs no rules of its own. No borrow survives the frame.
    pub fn with_rules(self, rules: Option<&'a RuleSet>) -> Self {
        Self {
            rules: rules.or(self.rules),
            ..self
        }
    }

    /// Bind the match's OverlayTypeClass table, as [`Self::with_rules`] binds
    /// the rules, for receivers the frame reaches without one of their own:
    /// the radio bus hands its receivers only the rules and these effects.
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
    pub(crate) fn flush_sound(self, sim: &Simulation, rules: Option<&RuleSet>) {
        if let Some(sound) = self.sound {
            let rules = rules
                .or(self.rules)
                .expect("sound boundary requires the frame's bound rules");
            sound.consume(sim, rules, &sim.sound_events);
            assert_eq!(sound.delivered(), sim.sound_events.len());
        }
    }

    pub(crate) fn delivered_sound_events(self) -> usize {
        self.sound.map_or(0, SoundBoundary::delivered)
    }
}
