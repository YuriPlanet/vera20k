//! Borrowed client effects at reached native call sites within one frame,
//! and the frame's bound resources.
//!
//! Process sound and local input owners implement these capabilities.
//! Simulation retains no callback, device or input state. Tooling/replay
//! explicitly omit client effects and consume their recorded commands.

use crate::rules::ruleset::RuleSet;
use crate::sim::command::{CommandEnvelope, CommandRecord};

use super::{SimSoundEvent, Simulation};

/// Consume the newly reached prefix through the existing process sound owner.
/// Facts remain available to the other frame consumers. Implementations retain
/// only a delivered-prefix count, never the borrowed simulation or events.
pub trait SoundBoundary {
    fn consume(&mut self, sim: &Simulation, rules: &RuleSet, events: &[SimSoundEvent]);
    fn delivered(&self) -> usize;

    /// One reached750D40 owner update. Headless/replay has no device handle;
    /// a client reuses its existing handle and the coordinate at this phase.
    fn update_looping_sound(
        &mut self,
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
    fn set_available(&mut self, sim: &Simulation, rules: &RuleSet, available: bool);
}

/// HouseClass::Update4F860F/4F8736 waits before publishing its result
/// and visiting later Houses. Audio and the 120-bucket clock stay with the
/// process; a headless frame has idle Vox and completes immediately.
pub trait OutcomeWaitBoundary {
    /// Process effects that native applies before a later sound or House
    /// wait. Return true only when its existing owner consumed the effect.
    fn apply_prefix_effect(
        &mut self,
        _effect: &crate::sim::trigger_runtime::TriggerEffect,
    ) -> bool {
        false
    }

    fn wait_complete(
        &mut self,
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
    fn leave_planning(&mut self, sim: &Simulation, rules: &RuleSet) -> Option<CommandEnvelope>;
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
        &mut self,
        _context: super::network_commands::NetworkQueueContext,
        _queues: &mut super::network_commands::NetworkCommandQueues<'_>,
    ) -> Result<Option<super::network_commands::NetworkQueuePass>, NetworkCommandError> {
        Ok(None)
    }

    fn record_frame_checksum(&mut self, frame: u32, checksum: u32);
    fn compare_frame_info(
        &self,
        record: &crate::sim::command::CommandRecord,
        current_frame: i32,
    ) -> Result<bool, NetworkCommandError>;
}

/// Headless/replay borrows the same checksum owner as the live process. Only
/// the owner instance differs; frame-zero reset, grace and comparison do not.
impl NetworkCommandBoundary for crate::sim::command_queue::FrameChecksumState {
    fn record_frame_checksum(&mut self, frame: u32, checksum: u32) {
        crate::sim::command_queue::FrameChecksumState::record_frame_checksum(self, frame, checksum);
    }

    fn compare_frame_info(
        &self,
        record: &crate::sim::command::CommandRecord,
        current_frame: i32,
    ) -> Result<bool, NetworkCommandError> {
        let info = crate::sim::command_queue::FrameInfo::decode(record).ok_or_else(|| {
            NetworkCommandError::Unavailable("comparison requires FRAMEINFO bytes".to_owned())
        })?;
        crate::sim::command_queue::FrameChecksumState::compare_frame_info(self, info, current_frame)
            .map_err(NetworkCommandError::Mismatch)
    }
}

/// One client owner that lends a frame several roles at once, because those
/// roles share its state (one SFX player and sound queue behind the sound,
/// radar, planning and outcome roles; one record of writes in order). A role
/// it does not lend returns `None`, and a callback it does not take does
/// nothing, exactly as when no owner is lent. A role or callback lent on its
/// own through [`FrameEffects`]'s `with_*` takes precedence over the client's.
pub trait FrameClient {
    fn sound(&mut self) -> Option<&mut dyn SoundBoundary> {
        None
    }
    fn tactical_radar(&mut self) -> Option<&mut dyn TacticalRadarBoundary> {
        None
    }
    fn outcome_wait(&mut self) -> Option<&mut dyn OutcomeWaitBoundary> {
        None
    }
    fn local_planning(&mut self) -> Option<&mut dyn LocalPlanningBoundary> {
        None
    }
    fn network_commands(&mut self) -> Option<&mut dyn NetworkCommandBoundary> {
        None
    }
    /// See [`FrameEffects::with_before_logic`].
    fn before_logic(&mut self, _sim: &Simulation) {}
    /// See [`FrameEffects::with_command_applied`].
    fn command_applied(&mut self, _sim: &Simulation, _command: &CommandRecord, _applied: bool) {}
    /// See [`FrameEffects::with_bounty_text`].
    fn bounty_text(
        &mut self,
        _sim: &Simulation,
        _rules: &RuleSet,
        _text: crate::sim::bounty::BountyText,
    ) {
    }
    /// See [`FrameEffects::with_production_request_cleared`].
    fn production_request_cleared(&mut self, _type_id: crate::sim::intern::InternedId) {}
}

/// Stack-scoped process effects accompanying this simulation visit. The
/// simulation reaches each lent owner through `&mut`, so a callback cannot
/// re-enter a boundary that is already running: the handle is passed on as
/// `&mut FrameEffects`, and [`Self::reborrow`] lends a narrower copy (a
/// nested receiver context, a replay's own network owner) for one call.
#[derive(Default)]
pub struct FrameEffects<'a> {
    sound: Option<&'a mut dyn SoundBoundary>,
    tactical_radar: Option<&'a mut dyn TacticalRadarBoundary>,
    outcome_wait: Option<&'a mut dyn OutcomeWaitBoundary>,
    local_planning: Option<&'a mut dyn LocalPlanningBoundary>,
    selection_voice_suppressed: bool,
    network_commands: Option<&'a mut dyn NetworkCommandBoundary>,
    before_logic: Option<&'a mut dyn FnMut(&Simulation)>,
    command_applied: Option<&'a mut dyn FnMut(&Simulation, &CommandRecord, bool)>,
    bounty_text: Option<&'a mut dyn FnMut(&Simulation, &RuleSet, crate::sim::bounty::BountyText)>,
    production_factories: &'a [crate::sim::production::ProductionFactoryVisit],
    production_request_cleared: Option<&'a mut dyn FnMut(crate::sim::intern::InternedId)>,
    client: Option<&'a mut dyn FrameClient>,
}

impl<'a> FrameEffects<'a> {
    pub(crate) fn exchange_commands(
        &mut self,
        context: super::network_commands::NetworkQueueContext,
        queues: &mut super::network_commands::NetworkCommandQueues<'_>,
    ) -> Result<Option<super::network_commands::NetworkQueuePass>, NetworkCommandError> {
        self.network().map_or(Ok(None), |boundary| {
            boundary.exchange_commands(context, queues)
        })
    }

    pub const fn empty() -> Self {
        Self {
            sound: None,
            tactical_radar: None,
            outcome_wait: None,
            local_planning: None,
            selection_voice_suppressed: false,
            network_commands: None,
            before_logic: None,
            command_applied: None,
            bounty_text: None,
            production_factories: &[],
            production_request_cleared: None,
            client: None,
        }
    }

    /// The same lent owners for a shorter borrow: what a nested context
    /// holds by value, or what a caller narrows with a `with_*` for one call.
    pub fn reborrow(&mut self) -> FrameEffects<'_> {
        FrameEffects {
            sound: self.sound.as_deref_mut().map(|b| b as _),
            tactical_radar: self.tactical_radar.as_deref_mut().map(|b| b as _),
            outcome_wait: self.outcome_wait.as_deref_mut().map(|b| b as _),
            local_planning: self.local_planning.as_deref_mut().map(|b| b as _),
            selection_voice_suppressed: self.selection_voice_suppressed,
            network_commands: self.network_commands.as_deref_mut().map(|b| b as _),
            before_logic: self.before_logic.as_deref_mut().map(|f| f as _),
            command_applied: self.command_applied.as_deref_mut().map(|f| f as _),
            bounty_text: self.bounty_text.as_deref_mut().map(|f| f as _),
            production_factories: self.production_factories,
            production_request_cleared: self
                .production_request_cleared
                .as_deref_mut()
                .map(|f| f as _),
            client: self.client.as_deref_mut().map(|c| c as _),
        }
    }

    pub fn for_sound(sound: &'a mut dyn SoundBoundary) -> Self {
        Self {
            sound: Some(sound),
            ..Self::empty()
        }
    }

    /// Lend every role and callback `client` takes ([`FrameClient`]).
    pub fn for_client(client: &'a mut dyn FrameClient) -> Self {
        Self {
            client: Some(client),
            ..Self::empty()
        }
    }

    fn sound(&mut self) -> Option<&mut dyn SoundBoundary> {
        match self.sound.as_deref_mut() {
            Some(sound) => Some(sound),
            None => self.client.as_deref_mut().and_then(FrameClient::sound),
        }
    }

    fn tactical_radar(&mut self) -> Option<&mut dyn TacticalRadarBoundary> {
        match self.tactical_radar.as_deref_mut() {
            Some(radar) => Some(radar),
            None => self
                .client
                .as_deref_mut()
                .and_then(FrameClient::tactical_radar),
        }
    }

    fn outcome_wait(&mut self) -> Option<&mut dyn OutcomeWaitBoundary> {
        match self.outcome_wait.as_deref_mut() {
            Some(wait) => Some(wait),
            None => self
                .client
                .as_deref_mut()
                .and_then(FrameClient::outcome_wait),
        }
    }

    fn local_planning(&mut self) -> Option<&mut dyn LocalPlanningBoundary> {
        match self.local_planning.as_deref_mut() {
            Some(planning) => Some(planning),
            None => self
                .client
                .as_deref_mut()
                .and_then(FrameClient::local_planning),
        }
    }

    fn network(&mut self) -> Option<&mut dyn NetworkCommandBoundary> {
        match self.network_commands.as_deref_mut() {
            Some(network) => Some(network),
            None => self
                .client
                .as_deref_mut()
                .and_then(FrameClient::network_commands),
        }
    }

    pub fn with_tactical_radar(self, boundary: &'a mut dyn TacticalRadarBoundary) -> Self {
        Self {
            tactical_radar: Some(boundary),
            ..self
        }
    }

    /// Client-only display delivery at the reached native call. The core
    /// consumed its pending sum/deadline before this optional observer runs.
    pub fn with_bounty_text(
        self,
        boundary: &'a mut dyn FnMut(&Simulation, &RuleSet, crate::sim::bounty::BountyText),
    ) -> Self {
        Self {
            bounty_text: Some(boundary),
            ..self
        }
    }

    pub(crate) fn emit_bounty_text(
        &mut self,
        sim: &Simulation,
        rules: &RuleSet,
        text: crate::sim::bounty::BountyText,
    ) {
        if let Some(boundary) = self.bounty_text.as_deref_mut() {
            boundary(sim, rules, text);
        } else if let Some(client) = self.client.as_deref_mut() {
            client.bounty_text(sim, rules, text);
        }
    }

    pub(crate) fn set_tactical_radar_available(
        &mut self,
        sim: &Simulation,
        rules: &RuleSet,
        available: bool,
    ) {
        if let Some(boundary) = self.tactical_radar() {
            boundary.set_available(sim, rules, available);
        }
    }

    pub fn with_outcome_wait(self, boundary: &'a mut dyn OutcomeWaitBoundary) -> Self {
        Self {
            outcome_wait: Some(boundary),
            ..self
        }
    }

    pub(crate) fn consume_prefix_effects(
        &mut self,
        effects: &mut Vec<crate::sim::trigger_runtime::TriggerEffect>,
    ) {
        if let Some(boundary) = self.outcome_wait() {
            effects.retain(|effect| !boundary.apply_prefix_effect(effect));
        }
    }

    pub(crate) fn outcome_wait_complete(
        &mut self,
        owner: crate::sim::intern::InternedId,
        kind: crate::sim::house_state::HouseOutcomeKind,
    ) -> bool {
        self.outcome_wait()
            .is_none_or(|boundary| boundary.wait_complete(owner, kind))
    }

    pub fn with_local_planning(self, boundary: &'a mut dyn LocalPlanningBoundary) -> Self {
        Self {
            local_planning: Some(boundary),
            ..self
        }
    }

    pub fn with_network_commands(self, boundary: &'a mut dyn NetworkCommandBoundary) -> Self {
        Self {
            network_commands: Some(boundary),
            ..self
        }
    }

    /// MainTick55DBCD's process-owned save runs after local input and Ground
    /// sorting, before Logic55DC9E. The callback may observe, never mutate,
    /// this exact world. It is borrowed for this frame and is not retained.
    pub fn with_before_logic(self, boundary: &'a mut dyn FnMut(&Simulation)) -> Self {
        Self {
            before_logic: Some(boundary),
            ..self
        }
    }

    /// Optional process observation after the gameplay receiver handled a command.
    /// The observer stores no command queue and cannot mutate the simulation.
    pub fn with_command_applied(
        self,
        observer: &'a mut dyn FnMut(&Simulation, &CommandRecord, bool),
    ) -> Self {
        Self {
            command_applied: Some(observer),
            ..self
        }
    }

    pub(crate) fn command_applied(
        &mut self,
        sim: &Simulation,
        command: &CommandRecord,
        applied: bool,
    ) {
        if let Some(observer) = self.command_applied.as_deref_mut() {
            observer(sim, command, applied);
        } else if let Some(client) = self.client.as_deref_mut() {
            client.command_applied(sim, command, applied);
        }
    }

    /// Sidebar6A6140's retained links, in Strip AI6A8DC6 order. A headless
    /// frame has no local strip and publishes no automatic PLACE. Whoever
    /// lends the visits also lends the owner of the request clear
    /// ([`Self::with_production_request_cleared`] or a client), or the strip
    /// keeps a stale request.
    pub fn with_production_strip(
        self,
        visits: &'a [crate::sim::production::ProductionFactoryVisit],
    ) -> Self {
        Self {
            production_factories: visits,
            ..self
        }
    }

    /// Clear the same strip entry's request synchronously at6A8F42, even if
    /// OutList is full, before MainTick55DBCD can save the process-owned
    /// sidebar.
    pub fn with_production_request_cleared(
        self,
        clear_request: &'a mut dyn FnMut(crate::sim::intern::InternedId),
    ) -> Self {
        Self {
            production_request_cleared: Some(clear_request),
            ..self
        }
    }

    pub(crate) fn production_factories(
        &self,
    ) -> &'a [crate::sim::production::ProductionFactoryVisit] {
        self.production_factories
    }

    pub(crate) fn clear_production_request(&mut self, type_id: crate::sim::intern::InternedId) {
        if let Some(clear) = self.production_request_cleared.as_deref_mut() {
            clear(type_id);
        } else if let Some(client) = self.client.as_deref_mut() {
            client.production_request_cleared(type_id);
        }
    }

    pub(crate) fn record_frame_checksum(&mut self, frame: u32, checksum: u32) {
        if let Some(boundary) = self.network() {
            boundary.record_frame_checksum(frame, checksum);
        }
    }

    pub(crate) fn compare_frame_info(
        &mut self,
        record: &crate::sim::command::CommandRecord,
        current_frame: i32,
    ) -> Result<bool, NetworkCommandError> {
        self.network()
            .ok_or_else(|| {
                NetworkCommandError::Unavailable(
                    "FRAMEINFO requires the network session checksum owner".to_owned(),
                )
            })?
            .compare_frame_info(record, current_frame)
    }

    pub(crate) fn before_logic(&mut self, sim: &Simulation) {
        if let Some(boundary) = self.before_logic.as_deref_mut() {
            boundary(sim);
        } else if let Some(client) = self.client.as_deref_mut() {
            client.before_logic(sim);
        }
    }

    pub(crate) fn leave_local_planning(
        &mut self,
        sim: &Simulation,
        rules: &RuleSet,
    ) -> Option<CommandEnvelope> {
        self.local_planning()
            .and_then(|boundary| boundary.leave_planning(sim, rules))
    }

    /// Planning Mode byte `0x00AC4CF4`; off without a local planner.
    pub(crate) fn local_planning_active(&mut self) -> bool {
        self.local_planning()
            .is_some_and(|boundary| boundary.planning_active())
    }

    /// Ares clears 0x00822CF2 around a silent Select. Its synchronous tag
    /// actions and any Select they start inherit the same voice setting.
    pub(crate) fn without_selection_voice(&mut self) -> FrameEffects<'_> {
        FrameEffects {
            selection_voice_suppressed: true,
            ..self.reborrow()
        }
    }

    pub(super) fn selection_voice_enabled(&self) -> bool {
        !self.selection_voice_suppressed
    }

    /// Stop/Release must complete at their native call sites before a later
    /// Main draw or object visit. The consumer also delivers earlier sound
    /// facts first, preserving the existing ordered producer channel.
    pub(crate) fn flush_sound(&mut self, sim: &Simulation, rules: &RuleSet) {
        if let Some(sound) = self.sound() {
            sound.consume(sim, rules, &sim.sound_events);
            assert_eq!(sound.delivered(), sim.sound_events.len());
        }
    }

    /// Finish preceding PlayAt/Stop/Release operations before the native
    /// positional update; neither the coordinate nor callback is retained.
    pub(crate) fn update_looping_sound(
        &mut self,
        sim: &Simulation,
        rules: &RuleSet,
        owner: u64,
        world: crate::sim::anim_class::AnimWorldCoord,
    ) {
        self.flush_sound(sim, rules);
        if let Some(sound) = self.sound() {
            sound.update_looping_sound(sim, owner, world);
        }
    }

    pub(crate) fn delivered_sound_events(&mut self) -> usize {
        self.sound().map_or(0, |sound| sound.delivered())
    }
}
