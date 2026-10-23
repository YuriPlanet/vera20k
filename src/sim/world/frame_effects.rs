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

    /// One reached750D40 owner update. Headless/replay has no device handle;
    /// a client reuses its existing handle and the coordinate at this phase.
    fn update_looping_sound(
        &self,
        _sim: &Simulation,
        _owner: u64,
        _world: crate::sim::anim_class::AnimWorldCoord,
    ) {
    }
}

/// The process-owned Radar656DF0 setter at reached House call sites. The
/// implementation keeps animation/audio state outside the simulation and
/// delivers preceding sound facts before applying the requested transition.
pub trait TacticalRadarBoundary {
    fn set_available(&self, sim: &Simulation, rules: &RuleSet, available: bool);
}

/// HouseClass::Update4F860F/4F8736 waits before publishing its result
/// and visiting later Houses. Audio and the 120-bucket clock stay with the
/// process; a headless frame has idle Vox and completes immediately.
pub trait OutcomeWaitBoundary {
    /// Process effects that native applies before a later sound or House
    /// wait. Return true only when its existing owner consumed the effect.
    fn apply_prefix_effect(&self, _effect: &crate::sim::trigger_runtime::TriggerEffect) -> bool {
        false
    }

    fn wait_complete(
        &self,
        owner: crate::sim::intern::InternedId,
        kind: crate::sim::house_state::HouseOutcomeKind,
    ) -> bool;
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

/// A checksum disagreement is a modal network failure, distinct from a
/// missing process owner or malformed callback input (a programming failure).
#[derive(Debug, thiserror::Error)]
pub enum NetworkCommandError {
    #[error(transparent)]
    Mismatch(#[from] crate::sim::command_queue::ChecksumMismatch),
    #[error(transparent)]
    Interrupted(#[from] super::network_commands::NetworkInterruption),
    #[error("{0}")]
    Unavailable(String),
}

/// The process network session owns checksum history and its grace timer.
/// A frame borrows that owner at the native post-Logic queue boundary; no
/// history or transport state is copied into Simulation.
pub trait NetworkCommandBoundary {
    /// `None` keeps diagnostic/replay's already-admitted event route. A live
    /// transport must return its Queue_AI decision and own all local transfer.
    fn exchange_commands(
        &self,
        _context: super::network_commands::NetworkQueueContext,
        _queues: &mut super::network_commands::NetworkCommandQueues<'_>,
    ) -> Result<Option<super::network_commands::NetworkQueuePass>, NetworkCommandError> {
        Ok(None)
    }

    fn record_frame_checksum(&self, frame: u32, checksum: u32);
    fn compare_frame_info(
        &self,
        record: &crate::sim::command::CommandRecord,
        current_frame: i32,
    ) -> Result<bool, NetworkCommandError>;
}

/// Headless/replay borrows the same checksum owner as the live process. Only
/// the owner instance differs; frame-zero reset, grace and comparison do not.
impl NetworkCommandBoundary for std::cell::RefCell<crate::sim::command_queue::FrameChecksumState> {
    fn record_frame_checksum(&self, frame: u32, checksum: u32) {
        self.borrow_mut().record_frame_checksum(frame, checksum);
    }

    fn compare_frame_info(
        &self,
        record: &crate::sim::command::CommandRecord,
        current_frame: i32,
    ) -> Result<bool, NetworkCommandError> {
        let info = crate::sim::command_queue::FrameInfo::decode(record).ok_or_else(|| {
            NetworkCommandError::Unavailable("comparison requires FRAMEINFO bytes".to_owned())
        })?;
        self.borrow()
            .compare_frame_info(info, current_frame)
            .map_err(NetworkCommandError::Mismatch)
    }
}

/// Stack-scoped process effects accompanying this simulation visit.
#[derive(Clone, Copy, Default)]
pub struct FrameEffects<'a> {
    sound: Option<&'a dyn SoundBoundary>,
    tactical_radar: Option<&'a dyn TacticalRadarBoundary>,
    outcome_wait: Option<&'a dyn OutcomeWaitBoundary>,
    local_planning: Option<&'a dyn LocalPlanningBoundary>,
    network_commands: Option<&'a dyn NetworkCommandBoundary>,
    overlay_registry: Option<&'a OverlayTypeRegistry>,
    before_logic: Option<&'a dyn Fn(&Simulation)>,
    production_factories: &'a [crate::sim::production::ProductionFactoryVisit],
    production_request_cleared: Option<&'a dyn Fn(crate::sim::intern::InternedId)>,
}

impl<'a> FrameEffects<'a> {
    pub(crate) fn exchange_commands(
        self,
        context: super::network_commands::NetworkQueueContext,
        queues: &mut super::network_commands::NetworkCommandQueues<'_>,
    ) -> Result<Option<super::network_commands::NetworkQueuePass>, NetworkCommandError> {
        self.network_commands.map_or(Ok(None), |boundary| {
            boundary.exchange_commands(context, queues)
        })
    }
    pub const fn empty() -> Self {
        Self {
            sound: None,
            tactical_radar: None,
            outcome_wait: None,
            local_planning: None,
            network_commands: None,
            overlay_registry: None,
            before_logic: None,
            production_factories: &[],
            production_request_cleared: None,
        }
    }

    pub fn for_sound(sound: &'a dyn SoundBoundary) -> Self {
        Self {
            sound: Some(sound),
            tactical_radar: None,
            outcome_wait: None,
            local_planning: None,
            network_commands: None,
            overlay_registry: None,
            before_logic: None,
            production_factories: &[],
            production_request_cleared: None,
        }
    }

    pub fn with_tactical_radar(self, boundary: &'a dyn TacticalRadarBoundary) -> Self {
        Self {
            tactical_radar: Some(boundary),
            ..self
        }
    }

    pub(crate) fn set_tactical_radar_available(
        self,
        sim: &Simulation,
        rules: &RuleSet,
        available: bool,
    ) {
        if let Some(boundary) = self.tactical_radar {
            boundary.set_available(sim, rules, available);
        }
    }

    pub fn with_outcome_wait(self, boundary: &'a dyn OutcomeWaitBoundary) -> Self {
        Self {
            outcome_wait: Some(boundary),
            ..self
        }
    }

    pub(crate) fn consume_prefix_effects(
        self,
        effects: &mut Vec<crate::sim::trigger_runtime::TriggerEffect>,
    ) {
        if let Some(boundary) = self.outcome_wait {
            effects.retain(|effect| !boundary.apply_prefix_effect(effect));
        }
    }

    pub(crate) fn outcome_wait_complete(
        self,
        owner: crate::sim::intern::InternedId,
        kind: crate::sim::house_state::HouseOutcomeKind,
    ) -> bool {
        self.outcome_wait
            .is_none_or(|boundary| boundary.wait_complete(owner, kind))
    }

    pub fn with_local_planning(self, boundary: &'a dyn LocalPlanningBoundary) -> Self {
        Self {
            local_planning: Some(boundary),
            ..self
        }
    }

    pub fn with_network_commands(self, boundary: &'a dyn NetworkCommandBoundary) -> Self {
        Self {
            network_commands: Some(boundary),
            ..self
        }
    }

    /// MainTick55DBCD's process-owned save runs after local input and Ground
    /// sorting, before Logic55DC9E. The callback may observe, never mutate,
    /// this exact world. It is borrowed for this frame and is not retained.
    pub fn with_before_logic(self, boundary: &'a dyn Fn(&Simulation)) -> Self {
        Self {
            before_logic: Some(boundary),
            ..self
        }
    }

    /// Sidebar6A6140's retained links, in Strip AI6A8DC6 order. A headless
    /// frame has no local strip and publishes no automatic PLACE. Clear the
    /// same entry's request synchronously at6A8F42, even if OutList is full,
    /// before MainTick55DBCD can save the process-owned sidebar.
    pub fn with_production_strip(
        self,
        visits: &'a [crate::sim::production::ProductionFactoryVisit],
        clear_request: &'a dyn Fn(crate::sim::intern::InternedId),
    ) -> Self {
        Self {
            production_factories: visits,
            production_request_cleared: Some(clear_request),
            ..self
        }
    }

    pub(crate) fn production_factories(
        self,
    ) -> &'a [crate::sim::production::ProductionFactoryVisit] {
        self.production_factories
    }

    pub(crate) fn clear_production_request(self, type_id: crate::sim::intern::InternedId) {
        if let Some(clear) = self.production_request_cleared {
            clear(type_id);
        }
    }

    pub(crate) fn record_frame_checksum(self, frame: u32, checksum: u32) {
        if let Some(boundary) = self.network_commands {
            boundary.record_frame_checksum(frame, checksum);
        }
    }

    pub(crate) fn compare_frame_info(
        self,
        record: &crate::sim::command::CommandRecord,
        current_frame: i32,
    ) -> Result<bool, NetworkCommandError> {
        self.network_commands
            .ok_or_else(|| {
                NetworkCommandError::Unavailable(
                    "FRAMEINFO requires the network session checksum owner".to_owned(),
                )
            })?
            .compare_frame_info(record, current_frame)
    }

    pub(crate) fn before_logic(self, sim: &Simulation) {
        if let Some(boundary) = self.before_logic {
            boundary(sim);
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

    /// Finish preceding PlayAt/Stop/Release operations before the native
    /// positional update; neither the coordinate nor callback is retained.
    pub(crate) fn update_looping_sound(
        self,
        sim: &Simulation,
        rules: &RuleSet,
        owner: u64,
        world: crate::sim::anim_class::AnimWorldCoord,
    ) {
        self.flush_sound(sim, rules);
        if let Some(sound) = self.sound {
            sound.update_looping_sound(sim, owner, world);
        }
    }

    pub(crate) fn delivered_sound_events(self) -> usize {
        self.sound.map_or(0, SoundBoundary::delivered)
    }
}
