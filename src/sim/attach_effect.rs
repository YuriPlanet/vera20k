//! One attached AttachEffect instance: its remaining duration and the delays
//! that re-create or end it.
//!
//! Native identity: Phobos `AttachEffectClass`
//! (`src/New/Entity/AttachEffectClass.{h,cpp}`). `AI()` runs one phase per
//! frame in a fixed order: an initial delay, then a pending re-creation delay,
//! then a queued duration refresh, then the duration tick. `RefreshDuration`
//! takes the override, the type's `Duration` and the two duration multipliers.
//!
//! This module owns the timing state and the per-frame AI walk, including the
//! animation each effect shows. Stat multipliers, tint rendering and discard
//! conditions read this state from their own owners.

use crate::rules::attach_effect::ExpireWeaponCondition;
use crate::rules::attach_effect::{AttachEffectType, AttachedAnimFlag};
use crate::rules::phobos_shields::AffectedHouse;
use crate::sim::world::Simulation;

mod combat;
mod stats;
mod update;

pub(crate) use stats::PhobosStatMultipliers;
#[cfg(all(test, tests_sim_a_b))]
pub(crate) use update::DiscardFacts;

/// `AttachEffectClass`'s constructor arguments, as the attach sites supply
/// them (`AEAttachParams`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct AttachEffectParams {
    /// `DurationOverrides`, paired with the attach list; `0` keeps the type's.
    pub duration_override: i32,
    /// `Delays`: re-creation delay after a natural end. Negative means the
    /// effect ends instead of re-creating.
    pub delay: i32,
    /// `InitialDelays`: frames before the effect becomes active.
    pub initial_delay: i32,
    /// `RecreationDelays`: frames after a full removal before re-attaching.
    pub recreation_delay: i32,
}

/// One live instance on a techno.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AttachEffectInstance {
    /// Catalog index of the type, so the instance survives a rules reload
    /// whose types keep their order.
    pub type_index: usize,
    remaining: i32,
    duration_override: i32,
    delay: i32,
    initial_delay: i32,
    recreation_delay: i32,
    current_delay: i32,
    should_refresh: bool,
    has_initialized: bool,
    /// Who attached it: `IsFromSource` compares this pair.
    source: Option<AttachSource>,
    /// `IsOnline`: only a `Powered` type follows its owner.
    is_online: bool,
    /// `Animation`: the live `AnimClass` showing this effect, owned by the
    /// same tree as the entity so a save restores the pair.
    #[serde(default)]
    anim: Option<u64>,
    /// `ShouldBeDiscarded`: set by the effect's event conditions (a fired
    /// count, received damage, entry, owner change, invoker death).
    #[serde(default)]
    should_be_discarded: bool,
    /// `FiringCount`: shots counted towards `DiscardOn.Firing.Count`.
    #[serde(default)]
    firing_count: i32,
    /// `ReceivedDamageCount`: hits counted towards
    /// `DiscardOn.ReceivedDamage.Count`.
    #[serde(default)]
    received_damage_count: i32,
    /// `LastSequenceCheck`: the infantry art sequence the previous frame
    /// played, for the deferred `DiscardOn.Sequences` form.
    #[serde(default)]
    last_sequence_check: Option<crate::rules::animation_sequence::SequenceKind>,
    /// `ArmorMultiplierTimer`: a real hit that applied a restricted
    /// `ArmorMultiplier` with `.Delay > 0` blocks further rolls until it runs
    /// out (`TechnoHooks.cpp:552`).
    #[serde(default)]
    armor_multiplier_timer: crate::sim::timer::CdTimer,
    /// `ReflectDamageTimer`: a reflection under `.Delay > 0` blocks the next
    /// one until it runs out (`Hooks.ReceiveDamage.cpp:429`).
    #[serde(default)]
    reflect_damage_timer: crate::sim::timer::CdTimer,
    /// `Animation.ResetOnReapply` (`New/Entity/AttachEffectClass.cpp:549-555`,
    /// `:928-934`, `:991-997`): every `RefreshDuration` and the shorter-duration
    /// reapply arm kill and re-create the animation. The flag asks the
    /// animation owner (`maintain_phobos_attach_effect_anim`) to retire the
    /// live one before its next pass.
    #[serde(default)]
    reset_anim: bool,
    /// `LaserTrailClass::LastLocation`: the endpoint the next trail segment
    /// starts from. The trail is created and retired with this instance
    /// (`AttachEffectClass::AttachEffect` / destructor).
    #[serde(default)]
    trail_last_location: Option<[i32; 3]>,
}

impl AttachEffectInstance {
    /// `AttachEffectClass`'s constructor: the duration starts at the override
    /// when present, otherwise the type's, and nothing is active yet.
    pub fn new(type_index: usize, params: AttachEffectParams, effect: &AttachEffectType) -> Self {
        let mut instance = Self {
            type_index,
            remaining: 0,
            duration_override: params.duration_override,
            delay: params.delay,
            initial_delay: params.initial_delay,
            recreation_delay: params.recreation_delay,
            current_delay: 0,
            should_refresh: false,
            has_initialized: false,
            source: None,
            is_online: true,
            anim: None,
            should_be_discarded: false,
            firing_count: 0,
            received_damage_count: 0,
            last_sequence_check: None,
            armor_multiplier_timer: crate::sim::timer::CdTimer::default(),
            reflect_damage_timer: crate::sim::timer::CdTimer::default(),
            reset_anim: false,
            trail_last_location: None,
        };
        instance.refresh(effect);
        instance
    }

    pub fn remaining(&self) -> i32 {
        self.remaining
    }

    pub fn current_delay(&self) -> i32 {
        self.current_delay
    }

    pub fn should_refresh(&self) -> bool {
        self.should_refresh
    }

    /// `HasInitialized`: the AI reached its first active frame.
    pub fn has_initialized(&self) -> bool {
        self.has_initialized
    }

    /// `IsSelfOwned`: the attach site said the effect belongs to the type
    /// itself (only the type/country list entry does so today); an effect
    /// attached from outside carries its invoker/source pair instead.
    pub fn is_self_owned(&self) -> bool {
        self.source.is_none()
    }

    /// A copy for `TransferAttachedEffects` (`AttachEffectClass.cpp:1294..1332`):
    /// the delays, source pair, duration and counters travel, while the target
    /// creates its own animation and starts a fresh `CurrentDelay`. The trail
    /// starts unset too: `CreateAndAttach` builds a new `LaserTrailClass`
    /// (`AttachEffectClass.cpp:89`) whose `LastLocation` the first `Update`
    /// seeds, and the transfer's sync list (`:1297..1315`) never names it.
    fn transfer_copy(&self) -> Self {
        Self {
            type_index: self.type_index,
            remaining: self.remaining,
            duration_override: self.duration_override,
            delay: self.delay,
            initial_delay: self.initial_delay,
            recreation_delay: self.recreation_delay,
            current_delay: 0,
            should_refresh: false,
            has_initialized: self.has_initialized,
            source: self.source,
            is_online: self.is_online,
            anim: None,
            should_be_discarded: self.should_be_discarded,
            firing_count: self.firing_count,
            received_damage_count: self.received_damage_count,
            last_sequence_check: self.last_sequence_check,
            armor_multiplier_timer: self.armor_multiplier_timer,
            reflect_damage_timer: self.reflect_damage_timer,
            reset_anim: false,
            trail_last_location: None,
        }
    }

    /// The frame's `AI()` walk reaches the animation check: not in the initial
    /// delay, the re-creation delay or the queued refresh (those return early),
    /// and the effect still has a duration to show.
    pub fn animation_phase(&self) -> bool {
        self.initial_delay <= 0
            && self.has_initialized
            && self.current_delay == 0
            && !self.should_refresh
            && self.remaining != 0
    }

    /// The live animation this effect shows, if one is attached.
    pub fn anim(&self) -> Option<u64> {
        self.anim
    }

    /// `HasExpired()`: a re-creating effect (`Delay >= 0`) never expires; a
    /// terminal one expires once its duration runs out.
    pub fn has_expired(&self) -> bool {
        self.delay < 0 && self.remaining == 0
    }

    /// `ResetIfRecreatable()`: a negative recreation delay refuses the reset.
    /// Otherwise the animation is dropped, the duration cleared and the
    /// re-creation delay armed.
    pub fn reset_if_recreatable(&mut self) -> bool {
        if self.recreation_delay < 0 {
            return false;
        }
        self.remaining = 0;
        self.current_delay = self.recreation_delay;
        self.should_refresh = true;
        true
    }

    /// `RefreshDuration` (`AttachEffectClass.cpp:528..555`): the override wins
    /// over the type's `Duration`. RESIDUAL: `Duration.ApplyFirepowerMult`
    /// (the invoker's current firepower) and `Duration.ApplyArmorMultOnTarget`
    /// (`GetCurrentArmorMultiplier` against the invoker house) are read but
    /// not applied, because the instance has no world access here; an effect
    /// that sets either keeps its unscaled duration.
    fn refresh(&mut self, effect: &AttachEffectType) {
        self.remaining = if self.duration_override != 0 {
            self.duration_override
        } else {
            effect.duration
        };
        if effect.animation_reset_on_reapply {
            self.request_anim_reset();
        }
    }

    /// `RefreshDuration` for a caller that already folded the duration
    /// multipliers into `duration`. `AttachEffectClass.cpp:549-555` ends it
    /// with the `Animation.ResetOnReapply` kill-and-re-create.
    pub fn refresh_with(&mut self, duration: i32) {
        self.remaining = duration.max(0);
    }

    /// The shorter-duration reapply arm (`AttachEffectClass.cpp:928-934` and
    /// `:991-997`): the duration is not refreshed, but the animation still
    /// restarts when the type asks for it.
    fn request_reapply_anim_reset(&mut self, effect: &AttachEffectType) {
        if effect.animation_reset_on_reapply {
            self.request_anim_reset();
        }
    }

    /// `KillAnim()` when there is one: the animation owner re-creates it on the
    /// next pass if `CanShowAnim`.
    fn request_anim_reset(&mut self) {
        if self.anim.is_some() {
            self.reset_anim = true;
        }
    }

    pub fn is_online(&self) -> bool {
        self.is_online
    }

    /// `OnlineCheck`: a `Powered` type follows its owner's activity (not
    /// deactivated, not under EMP, and for a building power-online); every
    /// other type stays online.
    pub fn online_check(&mut self, effect: &AttachEffectType, owner_active: bool) {
        if effect.powered {
            self.is_online = owner_active;
        }
    }

    /// `IsActiveIgnorePowered()`: a re-creating or self-owned effect waits for
    /// its delays to clear; a terminal one is active while its duration runs.
    fn active_ignoring_power(&self, self_owned: bool) -> bool {
        if self_owned || self.delay >= 0 {
            self.initial_delay <= 0
                && self.current_delay == 0
                && self.has_initialized
                && !self.should_refresh
        } else {
            self.remaining > 0
        }
    }

    /// `IsActive()`.
    pub fn is_active(&self, self_owned: bool) -> bool {
        self.is_online && self.active_ignoring_power(self_owned)
    }

    /// `IsActiveIgnorePowered()`: the delay rules without the power gate.
    pub fn is_active_ignoring_power(&self, self_owned: bool) -> bool {
        self.active_ignoring_power(self_owned)
    }

    /// `ShouldBeDiscarded`: an event condition asked for removal.
    pub fn is_flagged_discarded(&self) -> bool {
        self.should_be_discarded
    }

    pub(crate) fn mark_discarded(&mut self) {
        self.should_be_discarded = true;
    }

    pub(crate) fn clear_discarded(&mut self) {
        self.should_be_discarded = false;
    }

    /// `CanShowAnim` with the two flag actions: an offline owner and a temporal
    /// freeze hide, pause or freeze the animation.
    pub fn animation_state(
        &self,
        effect: &AttachEffectType,
        under_temporal: bool,
    ) -> AttachAnimState {
        let mut state = AttachAnimState {
            show: true,
            paused: false,
            temporal: false,
        };
        let mut apply = |action: AttachedAnimFlag| match action {
            AttachedAnimFlag::None => {}
            AttachedAnimFlag::Hides => state.show = false,
            AttachedAnimFlag::Temporal => state.temporal = true,
            AttachedAnimFlag::Paused => state.paused = true,
            AttachedAnimFlag::PausedTemporal => {
                state.paused = true;
                state.temporal = true;
            }
        };
        if effect.powered && !self.is_online {
            apply(effect.animation_offline_action);
        }
        if under_temporal {
            apply(effect.animation_temporal_action);
        }
        state
    }

    /// One `AI()` frame. `discarded` is `ShouldBeDiscardedNow()`: a pending
    /// delay or refresh does not advance while the effect is being discarded.
    pub fn tick(&mut self, effect: &AttachEffectType, discarded: bool) {
        if self.initial_delay > 0 {
            self.initial_delay -= 1;
            return;
        }
        if !self.has_initialized {
            self.has_initialized = true;
        }
        if self.current_delay > 0 {
            if !discarded {
                self.current_delay -= 1;
                if self.current_delay == 0 {
                    self.should_refresh = true;
                }
            }
            return;
        }
        if self.should_refresh {
            if !discarded {
                self.refresh(effect);
                self.should_refresh = false;
            }
            return;
        }
        if self.remaining > 0 {
            self.remaining -= 1;
        }
        if self.remaining != 0 {
            return;
        }
        // `Duration` reached zero: a negative delay leaves the effect ended.
        if self.delay < 0 {
            return;
        }
        self.current_delay = self.delay;
        if self.delay > 0 || discarded {
            // A zero delay behind a discard check stays queued instead of
            // refilling this frame, as `AI()`'s shared branch does.
            if self.delay == 0 {
                self.should_refresh = true;
            }
        } else {
            self.refresh(effect);
        }
    }
}

/// One weapon an effect asks the world to fire (`AEWeaponParams`): the weapon
/// name, the invoker that owns the shot, and the house to use when the
/// invoker is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttachEffectWeaponShot {
    pub weapon: String,
    pub invoker: Option<u64>,
    /// The firing house the shot carries (`InvokerHouse`, or the effect
    /// owner's house for an owner shot).
    pub house: Option<crate::sim::intern::InternedId>,
}

impl AttachEffectInstance {
    /// `AttachEffectClass::AddExpireWeaponParams` (`AttachEffectClass.cpp:1345`):
    /// the weapon this instance fires for `condition`, if it does. `owner` and
    /// `owner_house` are the effect's techno, used unless the shot takes the
    /// invoker. `active_count` is `GetAttachedEffectCumulativeCount` at the
    /// call site: the expiring instance is already inactive, so only the
    /// others count, while a discarded one still counts itself.
    pub(crate) fn expire_weapon_shot(
        &self,
        effect: &AttachEffectType,
        condition: ExpireWeaponCondition,
        owner: u64,
        owner_house: crate::sim::intern::InternedId,
        active_count: i32,
        ignore_cumulative_count_check: bool,
    ) -> Option<AttachEffectWeaponShot> {
        if effect.cumulative
            && effect.expire_weapon_cumulative_only_once
            && (ignore_cumulative_count_check || active_count >= 1)
        {
            return None;
        }
        self.expire_weapon_shot_identity(effect, condition, owner, owner_house)
    }

    /// The weapon and identity `AddExpireWeaponParams` gives a shot under
    /// `condition`, before any cumulative-count gate. The death trigger applies
    /// its own once-per-type gate (`TechnoClass_ReceiveDamage_AttachEffectExpireWeapon`).
    pub(crate) fn expire_weapon_shot_identity(
        &self,
        effect: &AttachEffectType,
        condition: ExpireWeaponCondition,
        owner: u64,
        owner_house: crate::sim::intern::InternedId,
    ) -> Option<AttachEffectWeaponShot> {
        let weapon = effect.expire_weapon.as_ref()?;
        if !effect.expire_weapon_trigger_on.contains(condition) {
            return None;
        }
        if effect.expire_weapon_use_invoker_as_owner {
            return Some(AttachEffectWeaponShot {
                weapon: weapon.clone(),
                invoker: self.source.and_then(|source| source.invoker),
                house: self.source.and_then(|source| source.invoker_house),
            });
        }
        Some(AttachEffectWeaponShot {
            weapon: weapon.clone(),
            invoker: Some(owner),
            house: Some(owner_house),
        })
    }

    /// `TechnoExt::ApplyRevengeWeapon`'s per-effect arm: the weapon and the
    /// identity of the shot. The house gate belongs to the caller.
    pub(crate) fn revenge_weapon_shot(
        &self,
        effect: &AttachEffectType,
        owner: u64,
        owner_house: crate::sim::intern::InternedId,
    ) -> Option<AttachEffectWeaponShot> {
        let weapon = effect.revenge_weapon.as_ref()?;
        if effect.revenge_weapon_use_invoker_as_owner {
            return Some(AttachEffectWeaponShot {
                weapon: weapon.clone(),
                invoker: self.source.and_then(|source| source.invoker),
                house: self.source.and_then(|source| source.invoker_house),
            });
        }
        Some(AttachEffectWeaponShot {
            weapon: weapon.clone(),
            invoker: Some(owner),
            house: Some(owner_house),
        })
    }
}

/// What the effect's animation should do this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AttachAnimState {
    pub show: bool,
    pub paused: bool,
    pub temporal: bool,
}

impl Default for AttachAnimState {
    fn default() -> Self {
        Self {
            show: true,
            paused: false,
            temporal: false,
        }
    }
}

/// Who handed the effect over: `IsFromSource` compares both the invoker and
/// the source object, so a warhead and the unit that fired it are distinct.
///
/// `source` is the attaching `AbstractClass`: VERA's attach sites are warheads,
/// so it holds the interned warhead handle (compared by name through the
/// interner); a self-owned attach keeps it empty. `invoker_house` is the
/// `InvokerHouse` the instance retains for a shot whose invoker is gone.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub struct AttachSource {
    pub invoker: Option<u64>,
    pub source: Option<u64>,
    #[serde(default)]
    pub invoker_house: Option<crate::sim::intern::InternedId>,
}

impl AttachSource {
    /// `AttachEffectClass::IsFromSource(pInvoker, pSource)` compares the two
    /// pointers only; the retained house is not part of it. `warhead` is the
    /// querying weapon's warhead name, resolved against the stored handle.
    pub(crate) fn matches(
        self,
        invoker: Option<u64>,
        warhead: Option<&str>,
        interner: &crate::sim::intern::StringInterner,
    ) -> bool {
        if self.invoker != invoker {
            return false;
        }
        match (self.source, warhead) {
            (None, None) => true,
            (Some(source), Some(warhead)) => interner
                .try_resolve(crate::sim::intern::InternedId::from_index(source as u32))
                .is_some_and(|name| name.eq_ignore_ascii_case(warhead)),
            _ => false,
        }
    }
}

/// The attach site's cumulative switches (`AEAttachInfoTypeClass`), separate
/// from the type's own `Cumulative` fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AttachPolicy {
    /// `AttachEffect.ReplaceLongerDuration`, defaulted from `[General]`.
    pub replace_longer_duration: bool,
    /// `AttachEffect.CumulativeSourceMaxCount`; negative means no cap.
    pub cumulative_source_max_count: i32,
    pub cumulative_refresh_all: bool,
    pub cumulative_refresh_all_on_attach: bool,
    pub cumulative_refresh_same_source_only: bool,
}

impl Default for AttachPolicy {
    /// `CumulativeRefreshSameSourceOnly` defaults to `true` in Phobos; the
    /// source cap is unbounded and the refresh switches are off.
    fn default() -> Self {
        Self {
            replace_longer_duration: false,
            cumulative_source_max_count: -1,
            cumulative_refresh_all: false,
            cumulative_refresh_all_on_attach: false,
            cumulative_refresh_same_source_only: true,
        }
    }
}

/// What one attach attempt did, mirroring `CreateAndAttach` returning a new
/// instance or refreshing an existing one and returning null.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachOutcome {
    /// Appended at this index.
    Attached(usize),
    /// An existing instance absorbed the attach; nothing was added.
    Refreshed,
}

/// One techno's live effects, in attach order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AttachEffectList {
    instances: Vec<AttachEffectInstance>,
}

impl AttachEffectList {
    pub fn instances(&self) -> &[AttachEffectInstance] {
        &self.instances
    }

    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// `CreateAndAttach`'s cumulative decision: a non-cumulative type refreshes
    /// the one existing instance, and a capped cumulative type refreshes the
    /// shortest match (or every match under `CumulativeRefreshAll`) instead of
    /// appending.
    pub fn attach(
        &mut self,
        type_index: usize,
        params: AttachEffectParams,
        effect: &AttachEffectType,
        policy: AttachPolicy,
        source: Option<AttachSource>,
    ) -> AttachOutcome {
        let duration = if params.duration_override != 0 {
            params.duration_override
        } else {
            effect.duration
        };
        let mut current_type_count = 0i32;
        let mut current_source_count = 0i32;
        let mut shortest: Option<usize> = None;
        let mut matches: Vec<usize> = Vec::new();

        for index in 0..self.instances.len() {
            let instance = &self.instances[index];
            if instance.type_index != type_index {
                continue;
            }
            current_type_count += 1;
            if !effect.cumulative {
                // Both arms restart the animation under `ResetOnReapply`:
                // `RefreshDuration` does it itself, the shorter-duration arm
                // does it explicitly.
                if policy.replace_longer_duration || duration >= instance.remaining {
                    self.instances[index].refresh_with(duration);
                }
                self.instances[index].request_reapply_anim_reset(effect);
                return AttachOutcome::Refreshed;
            }
            // `IsFromSource(pInvoker, pSource)`: the retained house is no
            // part of the match.
            let same_source = instance.source.map(|held| (held.invoker, held.source))
                == source.map(|incoming| (incoming.invoker, incoming.source));
            if same_source {
                current_source_count += 1;
            }
            if !policy.cumulative_refresh_same_source_only || same_source {
                matches.push(index);
                let shorter =
                    shortest.is_none_or(|best| instance.remaining < self.instances[best].remaining);
                if shorter {
                    shortest = Some(index);
                }
            }
        }

        if effect.cumulative {
            let type_capped = effect.cumulative_max_count >= 0
                && current_type_count >= effect.cumulative_max_count;
            let source_capped = policy.cumulative_source_max_count >= 0
                && current_source_count >= policy.cumulative_source_max_count;
            if type_capped || source_capped {
                if policy.cumulative_refresh_all {
                    for index in matches {
                        self.instances[index].refresh_with(duration);
                        self.instances[index].request_reapply_anim_reset(effect);
                    }
                } else if let Some(index) = shortest {
                    if policy.replace_longer_duration || duration >= self.instances[index].remaining
                    {
                        self.instances[index].refresh_with(duration);
                    }
                    self.instances[index].request_reapply_anim_reset(effect);
                }
                return AttachOutcome::Refreshed;
            } else if policy.cumulative_refresh_all && policy.cumulative_refresh_all_on_attach {
                // `CumulativeRefreshAll.OnAttach` refreshes under the cap and
                // still appends the new instance.
                for index in matches {
                    self.instances[index].refresh_with(duration);
                    self.instances[index].request_reapply_anim_reset(effect);
                }
            }
        }

        let mut instance = AttachEffectInstance::new(type_index, params, effect);
        instance.source = source;
        self.instances.push(instance);
        AttachOutcome::Attached(self.instances.len() - 1)
    }

    /// `OnlineCheck` against one owner state, for every instance.
    pub fn online_check_all(&mut self, effects: &[AttachEffectType], owner_active: bool) {
        for index in 0..self.instances.len() {
            let type_index = self.instances[index].type_index;
            let Some(effect) = effects.get(type_index) else {
                continue;
            };
            self.instances[index].online_check(effect, owner_active);
        }
    }

    /// One frame for every instance, in attach order.
    pub fn tick_all(
        &mut self,
        effects: &[AttachEffectType],
        mut discarded: impl FnMut(usize, &AttachEffectInstance, &AttachEffectType) -> bool,
    ) {
        for index in 0..self.instances.len() {
            let type_index = self.instances[index].type_index;
            let Some(effect) = effects.get(type_index) else {
                continue;
            };
            let discarded = discarded(index, &self.instances[index], effect);
            self.instances[index].tick(effect, discarded);
        }
    }
}

/// One value of a paired list: the index's own entry, else the last, else the
/// key's default. Phobos' `ValueableVector` reads extend the same way.
fn paired(list: &[i32], index: usize, default: i32) -> i32 {
    list.get(index)
        .or_else(|| list.last())
        .copied()
        .unwrap_or(default)
}

/// `EnumFunctions::IsTechnoEligible`'s type side: the category mask, then
/// `AffectTypes` (when non-empty) and `IgnoreTypes` against the target's type
/// id. Shared by every attach gate and by the transfer hook.
fn effect_target_eligible(
    category: crate::rules::object_type::ObjectCategory,
    object: &crate::rules::object_type::ObjectType,
    effect: &AttachEffectType,
) -> bool {
    effect.affects_target.allows(category)
        && (effect.affect_types.is_empty()
            || effect
                .affect_types
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&object.id)))
        && !effect
            .ignore_types
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&object.id))
}

/// `AttachEffectClass::CreateAndAttach` (`AttachEffectClass.cpp:883..898`)
/// gates every attach site before the cumulative decision: an active Iron
/// Curtain or Force Shield needs the type's matching penetration flag,
/// `AffectsTarget` must admit the target's class, and `AffectTypes` (when
/// non-empty) then `IgnoreTypes` filter the target's type id.
pub fn can_attach_effect(
    category: crate::rules::object_type::ObjectCategory,
    invulnerability: Option<&crate::sim::superweapon::invulnerability::InvulnerabilityState>,
    object: &crate::rules::object_type::ObjectType,
    effect: &AttachEffectType,
    frame: u32,
) -> bool {
    if crate::sim::superweapon::invulnerability::is_invulnerable(invulnerability, frame) {
        // `pTarget->ForceShielded ? PenetratesForceShield.Get(PenetratesIronCurtain) : PenetratesIronCurtain`.
        let kind = invulnerability.expect("active invulnerability").kind();
        let penetrates =
            if kind == crate::sim::superweapon::invulnerability::InvulnKind::ForceShield {
                effect.penetrates_force_shield()
            } else {
                effect.penetrates_iron_curtain
            };
        if !penetrates {
            return false;
        }
    }
    effect_target_eligible(category, object, effect)
}

/// Phobos attaches a techno type's `AttachEffect.AttachTypes` once when the
/// entity is created, pairing `DurationOverrides`, `Delays`, `InitialDelays`
/// and `RecreationDelays` by index. `source` is the invoking pair when the
/// attachment came from outside (a warhead or weapon); a type's own list has
/// none. Each entry passes [`can_attach_effect`] first, as `CreateAndAttach`
/// does for every attach site.
pub fn attach_initial_effects(
    list: &mut AttachEffectList,
    category: crate::rules::object_type::ObjectCategory,
    invulnerability: Option<&crate::sim::superweapon::invulnerability::InvulnerabilityState>,
    object: &crate::rules::object_type::ObjectType,
    rules: &crate::rules::ruleset::RuleSet,
    frame: u32,
    source: Option<AttachSource>,
) -> usize {
    attach_effect_names(
        list,
        object.attach_effect_attach_types(),
        object.attach_effect_duration_overrides(),
        object.attach_effect_delays(),
        object.attach_effect_initial_delays(),
        object.attach_effect_recreation_delays(),
        category,
        invulnerability,
        object,
        rules,
        frame,
        source,
    )
}

/// Phobos `TechnoExt::InitializeAttachEffects`' second call
/// (`Body.Internal.cpp:157`) and the owner-change hook
/// (`House/Hooks.cpp:342-343`): the owner country's
/// `AttachEffect.AttachTypes`, paired with its own overrides and delays,
/// attaches with the techno itself as the source object.
#[allow(clippy::too_many_arguments)]
pub fn attach_country_effects(
    list: &mut AttachEffectList,
    category: crate::rules::object_type::ObjectCategory,
    invulnerability: Option<&crate::sim::superweapon::invulnerability::InvulnerabilityState>,
    object: &crate::rules::object_type::ObjectType,
    rules: &crate::rules::ruleset::RuleSet,
    frame: u32,
    source: Option<AttachSource>,
    country: &str,
) -> usize {
    let Some(country) = rules.country_rules(country) else {
        return 0;
    };
    attach_effect_names(
        list,
        &country.attach_effect_attach_types,
        &country.attach_effect_duration_overrides,
        &country.attach_effect_delays,
        &country.attach_effect_initial_delays,
        &country.attach_effect_recreation_delays,
        category,
        invulnerability,
        object,
        rules,
        frame,
        source,
    )
}

#[allow(clippy::too_many_arguments)]
fn attach_effect_names(
    list: &mut AttachEffectList,
    names: &[String],
    overrides: &[i32],
    delays: &[i32],
    initial_delays: &[i32],
    recreation_delays: &[i32],
    category: crate::rules::object_type::ObjectCategory,
    invulnerability: Option<&crate::sim::superweapon::invulnerability::InvulnerabilityState>,
    object: &crate::rules::object_type::ObjectType,
    rules: &crate::rules::ruleset::RuleSet,
    frame: u32,
    source: Option<AttachSource>,
) -> usize {
    if names.is_empty() {
        return 0;
    }
    let policy = AttachPolicy {
        replace_longer_duration: rules.attach_effect_globals().replace_longer_duration,
        ..AttachPolicy::default()
    };
    let catalog = rules.attach_effect_types();
    let mut attached = 0;
    for (index, name) in names.iter().enumerate() {
        let Some(type_index) = catalog.index_of(name) else {
            continue;
        };
        let effect = &catalog.types()[type_index];
        if !can_attach_effect(category, invulnerability, object, effect, frame) {
            continue;
        }
        let params = AttachEffectParams {
            duration_override: paired(overrides, index, 0),
            delay: paired(delays, index, 0),
            initial_delay: paired(initial_delays, index, 0),
            recreation_delay: paired(recreation_delays, index, -1),
        };
        if let AttachOutcome::Attached(_) = list.attach(type_index, params, effect, policy, source)
        {
            attached += 1;
        }
    }
    attached
}

/// C++ `static_cast<int>(value * multiplier)` as MSVC compiles it: truncation
/// toward zero, and `i32::MIN` for a NaN or out-of-range product. Phobos
/// performs its integer stat hooks in C++ doubles, so each one crosses this
/// boundary; gamemd's own x87 steps stay with `native_x87`.
pub fn scale_i32(value: i32, multiplier: f64) -> i32 {
    cpp_i32(f64::from(value) * multiplier)
}

/// C++ `static_cast<int>(value / divisor)` (MSVC `cvttsd2si`): the armor
/// divide Phobos' receive hook performs on its own.
pub fn divide_i32(value: i32, divisor: f64) -> i32 {
    cpp_i32(f64::from(value) / divisor)
}

fn cpp_i32(value: f64) -> i32 {
    if value.is_nan() || value < f64::from(i32::MIN) || value >= 2_147_483_648.0 {
        i32::MIN
    } else {
        value.trunc() as i32
    }
}

/// Phobos `GeneralUtils::SafeMultiply(int, double)`: the product clamped to
/// the `int` range, then MSVC's cast. The Wasm module range multiplier uses
/// it in every build.
fn safe_multiply(value: i32, multiplier: f64) -> i32 {
    let product = f64::from(value) * multiplier;
    if product > f64::from(i32::MAX) {
        i32::MAX
    } else if product < f64::from(i32::MIN) {
        i32::MIN
    } else {
        cpp_i32(product)
    }
}

/// `WeaponTypeExt::GetRangeWithModifiers(WeaponTypeClass*, TechnoClass*)`
/// (`Ext/WeaponType/Body.cpp:374-396`): the weapon's own range behind the
/// `-512` always-in-range sentinel, then the three-argument fold.
///
/// RESIDUAL: the native `CanOccupyFire` arm (`OccupyWeaponRange *
/// LeptonsPerCell`) has no VERA equivalent on this owner.
pub(crate) fn weapon_range_with_modifiers(
    rules: &crate::rules::ruleset::RuleSet,
    interner: &crate::sim::intern::StringInterner,
    entities: Option<&crate::sim::entity_store::EntityStore>,
    firer: &crate::sim::game_entity::GameEntity,
    weapon: &crate::rules::weapon_type::WeaponType,
) -> i32 {
    let range = weapon.range_leptons;
    if i64::from(range) == crate::util::lepton::WEAPON_RANGE_ALWAYS_IN_RANGE_LEPTONS {
        return range;
    }
    get_range_with_modifiers(rules, interner, entities, firer, &weapon.id, range)
}

/// `WeaponTypeExt::GetRangeWithModifiers(WeaponTypeClass*, TechnoClass*, int)`
/// (`Ext/WeaponType/Body.cpp:398-443`): the veteran `Range` ability's
/// `VeteranRange` multiplier (max with 0) and then the AttachEffect fold,
/// both over the transport an open-topped passenger resolves its range
/// against ([`range_modifier_source`]). The explicit-range overload has no
/// `-512` arm; [`weapon_range_with_modifiers`] owns that short-circuit.
pub(crate) fn get_range_with_modifiers(
    rules: &crate::rules::ruleset::RuleSet,
    interner: &crate::sim::intern::StringInterner,
    entities: Option<&crate::sim::entity_store::EntityStore>,
    firer: &crate::sim::game_entity::GameEntity,
    weapon: &str,
    range: i32,
) -> i32 {
    let mut range = range;
    let source = range_modifier_source(firer, entities, rules, interner);
    if interner.try_resolve(source.type_ref()).is_some()
        && let Some(object) = interner.object_type(source.type_ref(), rules)
        && crate::sim::combat::crit::has_phobos_ability(
            source.veterancy.rank_kind(),
            object,
            crate::rules::phobos_crit::PhobosAbilityFlags::RANGE,
        )
    {
        let global = f64::from_bits(rules.phobos_globals.veteran_range.bits());
        let multiplier = object.phobos_techno_extras.veteran_range_multiplier(global);
        range = safe_multiply(range, multiplier.max(0.0));
    }
    range_with_attach_effects(source, weapon, range, rules)
}

/// `BulletClass_AI_Ranged @ 0x467B8E` (`Ext/Bullet/Hooks.cpp:636-659`): a
/// `Ranged=` projectile's `maxRange` passes [`get_range_with_modifiers`] when
/// it is positive, its weapon's nullable key else `[CombatDamage]
/// ProjectileRange.ApplyModifiers` (default true) admits it. The native
/// `maxRange > 0` guard runs before the key, so zero and negative ranges are
/// left untouched.
pub(crate) fn initial_projectile_range(
    rules: &crate::rules::ruleset::RuleSet,
    interner: &crate::sim::intern::StringInterner,
    entities: Option<&crate::sim::entity_store::EntityStore>,
    firer: &crate::sim::game_entity::GameEntity,
    weapon: &crate::rules::weapon_type::WeaponType,
    range: i32,
) -> i32 {
    if range <= 0 {
        return range;
    }
    let apply = weapon
        .phobos_projectile_range_apply_modifiers
        .unwrap_or(rules.combat_damage.phobos_projectile_range_apply_modifiers);
    if apply {
        get_range_with_modifiers(rules, interner, entities, firer, &weapon.id, range)
    } else {
        range
    }
}

/// `WeaponTypeExt::GetRangeWithModifiers`'s entity choice
/// (`Ext/WeaponType/Body.cpp:400-410`): the entity whose AttachEffect range
/// fold applies. A passenger inside an `OpenTopped` transport whose type
/// enables `OpenTopped.UseTransportRangeModifiers` (defaulting to `[General]`)
/// folds the transport's active effects instead of its own. `None` entities
/// (mapless probes) keep the firer.
pub(crate) fn range_modifier_source<'a>(
    entity: &'a crate::sim::game_entity::GameEntity,
    entities: Option<&'a crate::sim::entity_store::EntityStore>,
    rules: &crate::rules::ruleset::RuleSet,
    interner: &crate::sim::intern::StringInterner,
) -> &'a crate::sim::game_entity::GameEntity {
    let Some(entities) = entities else {
        return entity;
    };
    let Some(transport_id) = entity.passenger_role().inside_transport_id() else {
        return entity;
    };
    let Some(transport) = entities.get(transport_id) else {
        return entity;
    };
    let Some(kind) = interner.object_type(transport.type_ref(), rules) else {
        return entity;
    };
    let enabled = kind.open_topped_use_transport_range_modifiers().unwrap_or(
        rules
            .attach_effect_globals()
            .open_topped_use_transport_range_modifiers,
    );
    if enabled && kind.open_topped {
        transport
    } else {
        entity
    }
}

/// Resolve the open-topped carrier's `ObjectType` for a passenger. The Phobos
/// `OpenTopped.*` hooks read the transport's type extension
/// (`TechnoExt::Fetch(pThis->Transporter)->TypeExtData`), e.g.
/// `TechnoClass_IsCloseEnoughToTarget_OpenTopped_RangeBonus @ 0x6F72D2`
/// (`Ext/Techno/Hooks.Transport.cpp:173-176`); VERA's carrier relation
/// (`inside_transport_id`) is that entity. `None` for a mapless probe or a
/// carrier whose type does not resolve.
pub(crate) fn open_topped_transport_type<'r>(
    entity: &crate::sim::game_entity::GameEntity,
    entities: &crate::sim::entity_store::EntityStore,
    rules: &'r crate::rules::ruleset::RuleSet,
    interner: &crate::sim::intern::StringInterner,
) -> Option<&'r crate::rules::object_type::ObjectType> {
    let id = entity.passenger_role.inside_transport_id()?;
    let transport = entities.get(id)?;
    let kind = interner.object_type(transport.type_ref(), rules)?;
    kind.open_topped.then_some(kind)
}

/// Phobos `WeaponTypeExt::GetRangeWithModifiers`'s AttachEffect arm: every
/// active effect whose weapon lists admit `weapon` multiplies the range (in
/// leptons, clamped) and sums its `WeaponRange.ExtraRange`; the summed cells
/// are converted to leptons and added after every multiplier, and the result
/// floors at zero. A type with nothing to add leaves the range untouched.
/// `entity` is the fold source [`range_modifier_source`] resolved.
///
/// Wasm module `WeaponRangeMultiplier` and `WeaponRangeExtra` modifiers on
/// `entity` (`ext_modules::modifiers`) compose as one more effect for every
/// weapon: their multiplier after the effects' multipliers, their leptons
/// beside the summed extra range, then the same floor. Without them, and
/// in every build, the range reads as before.
pub(crate) fn range_with_attach_effects(
    entity: &crate::sim::game_entity::GameEntity,
    weapon: &str,
    range: i32,
    rules: &crate::rules::ruleset::RuleSet,
) -> i32 {
    // The explicit-range overload has no `-512` short-circuit; the weapon
    // wrapper above owns that sentinel before it reaches this fold.
    let module = entity.module_range_terms();
    if entity.phobos_stat_multipliers().has_range_modifier {
        let effects = rules.attach_effect_types().types();
        let mut range = range;
        let mut extra_range = 0.0;
        for (_, effect) in entity.phobos_attach_effects().active_effects(effects) {
            if !effect.has_range_modifier() {
                continue;
            }
            if !effect.weapon_range_allow_weapons.is_empty()
                && !effect
                    .weapon_range_allow_weapons
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(weapon))
            {
                continue;
            }
            if effect
                .weapon_range_disallow_weapons
                .iter()
                .any(|name| name.eq_ignore_ascii_case(weapon))
            {
                continue;
            }
            range = safe_multiply(range, effect.weapon_range_multiplier.max(0.0));
            extra_range += effect.weapon_range_extra_range;
        }
        let (range, module_extra) = match module {
            Some((multiplier, extra)) => (safe_multiply(range, multiplier), extra),
            None => (range, 0),
        };
        // `Math::max(range, 0)` (`WeaponType/Body.cpp:467`).
        return range
            .wrapping_add(cpp_i32(extra_range * 256.0))
            .wrapping_add(module_extra)
            .max(0);
    }
    match module {
        None => range,
        Some((multiplier, extra)) => safe_multiply(range, multiplier).wrapping_add(extra).max(0),
    }
}

/// Phobos `EnumFunctions::CanTargetHouse` with a nullable source: the
/// shared port handles a known source; a hit with none follows the enemy
/// bucket, as the AttachEffect branch has always read it.
fn affects_house(
    flags: AffectedHouse,
    source: Option<crate::sim::intern::InternedId>,
    target: crate::sim::intern::InternedId,
    houses: &crate::sim::houses::Houses,
    alliances: &crate::sim::house_diplomacy::HouseAlliances,
    interner: &crate::sim::intern::StringInterner,
) -> bool {
    let Some(source) = source else {
        return flags.contains(AffectedHouse::ENEMIES);
    };
    crate::sim::combat::can_target_house(flags, source, target, houses, alliances, interner)
}

impl Simulation {
    /// Phobos `AttachEffectClass::TransferAttachedEffects`
    /// (`AttachEffectClass.cpp:1222..1333`) at the deploy
    /// (`UnitClass_Deploy_Transfer 0x739956`) and undeploy
    /// (`BuildingClass_Mi_Selling_Transfer 0x44A03C`) hooks: the source techno
    /// is deleted afterwards, so its instances move to the target unless
    /// `AllowTransfer` refuses or the target fails the category/type gates.
    /// An existing instance keeps the longer duration; a cumulative type
    /// appends until its own cap, and only a same-source match is refreshed.
    pub(crate) fn transfer_phobos_attach_effects(
        &mut self,
        source: u64,
        target: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        if source == target {
            return;
        }
        let (Some(source_entity), Some(target_entity)) = (
            self.substrate.entities.get(source),
            self.substrate.entities.get(target),
        ) else {
            return;
        };
        if source_entity.phobos_attach_effects().is_empty() {
            return;
        }
        let Some(target_object) = self.interner.object_type(target_entity.type_ref(), rules) else {
            return;
        };
        let target_category = target_entity.category().into();
        let target_invulnerability = target_entity.invulnerability.clone();
        let effects = rules.attach_effect_types().types();
        let mut list = std::mem::take(
            self.substrate
                .entities
                .get_mut(source)
                .expect("the gated source still exists")
                .phobos_attach_effects_mut(),
        );
        let mut target_list = std::mem::take(
            self.substrate
                .entities
                .get_mut(target)
                .expect("the gated target still exists")
                .phobos_attach_effects_mut(),
        );
        for instance in std::mem::take(&mut list.instances) {
            let Some(effect) = effects.get(instance.type_index) else {
                continue;
            };
            if !effect.allows_transfer(instance.is_self_owned(), false)
                || !effect_target_eligible(target_category, target_object, effect)
            {
                continue;
            }
            let mut type_count = 0;
            let mut first_match: Option<usize> = None;
            let mut shortest_same_source: Option<usize> = None;
            for (index, held) in target_list.instances.iter().enumerate() {
                if held.type_index != instance.type_index {
                    continue;
                }
                type_count += 1;
                first_match.get_or_insert(index);
                if held.source.map(|held| (held.invoker, held.source))
                    == instance
                        .source
                        .map(|incoming| (incoming.invoker, incoming.source))
                    && shortest_same_source
                        .is_none_or(|best| held.remaining < target_list.instances[best].remaining)
                {
                    shortest_same_source = Some(index);
                }
            }
            // `CreateAndAttach`'s match: a non-cumulative type takes the first
            // same-type instance; a cumulative one only a same-source match,
            // and only at its cap.
            let matched = if effect.cumulative {
                let at_cap =
                    effect.cumulative_max_count >= 0 && type_count >= effect.cumulative_max_count;
                at_cap.then_some(shortest_same_source).flatten()
            } else {
                first_match
            };
            if let Some(index) = matched {
                let duration = target_list.instances[index]
                    .remaining
                    .max(instance.remaining);
                target_list.instances[index].remaining = duration;
                continue;
            }
            // A same-source cumulative match below the cap takes native's
            // `if (match)` arm, which writes nothing and discards the source
            // instance.
            if effect.cumulative && shortest_same_source.is_some() {
                continue;
            }
            // Otherwise native calls `CreateAndAttach(..., checkCumulative=false)`:
            // the attach gate runs again, then the first same-type instance
            // refreshes with the attach duration when it is not longer, or a
            // new instance is appended when none exists.
            if !can_attach_effect(
                target_category,
                target_invulnerability.as_ref(),
                target_object,
                effect,
                self.session.binary_frame(),
            ) {
                continue;
            }
            let incoming_duration = if instance.duration_override != 0 {
                instance.duration_override
            } else {
                effect.duration
            };
            if let Some(index) = first_match {
                if incoming_duration >= target_list.instances[index].remaining {
                    target_list.instances[index].remaining = incoming_duration;
                }
                continue;
            }
            target_list.instances.push(instance.transfer_copy());
        }
        let target_entity = self
            .substrate
            .entities
            .get_mut(target)
            .expect("the gated target still exists");
        *target_entity.phobos_attach_effects_mut() = target_list;
        target_entity.recalculate_phobos_stat_multipliers(effects);
    }

    /// Update every active effect's laser trail during techno AI, before the
    /// foot locomotor process rather than after it.
    pub(crate) fn update_phobos_laser_trails(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.phobos_attach_effects().is_empty()
            || entity.lifecycle.in_limbo()
            || entity.passenger_role.inside_transport_id().is_some()
            || entity.chrono_warp_latch()
        {
            return;
        }
        let location = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            self.resolved_terrain.as_ref(),
        );
        let current = [location.x, location.y, location.z];
        let owner = entity.owner();
        let cloak_state = entity.cloak.as_ref().map_or(0, |cloak| cloak.state());
        let effects = rules.attach_effect_types().types();
        let art = rules.art();
        let mut list = std::mem::take(
            self.substrate
                .entities
                .get_mut(id)
                .expect("the gated entity still exists")
                .phobos_attach_effects_mut(),
        );
        for instance in &mut list.instances {
            let Some(effect) = effects.get(instance.type_index) else {
                continue;
            };
            let Some(name) = effect.laser_trail_type.as_deref() else {
                continue;
            };
            let Some(trail) = art.laser_trail(name) else {
                continue;
            };
            // `UpdateLaserTrails` while uncloaking and the type is not
            // cloak-visible only moves `LastLocation` (`Body.Update.cpp:564..566`).
            if cloak_state == 3 && !trail.cloak_visible {
                instance.trail_last_location = Some(current);
                continue;
            }
            // The shared `LaserTrailClass::Update` owner: distance, visibility,
            // `IgnoreVertical` and the one LaserDraw construction live in
            // `sim::laser_trail`, also used by bullets and voxel debris.
            let cloaked = cloak_state == 2 && !trail.cloak_visible;
            crate::sim::laser_trail::update(
                &mut instance.trail_last_location,
                trail,
                current,
                true,
                cloaked,
                Some(owner),
                rules.radiation.color,
                &self.session,
                &mut self.main_rng,
                &mut self.lifecycle_outputs,
            );
        }
        let entity = self
            .substrate
            .entities
            .get_mut(id)
            .expect("the gated entity still exists");
        *entity.phobos_attach_effects_mut() = list;
    }
}

#[cfg(all(test, tests_sim_a_b))]
mod phobos_regression_tests {
    use super::*;
    use tests::ALL_HIT;

    fn lasers(sim: &Simulation) -> Vec<crate::sim::combat::laser::LaserBirth> {
        sim.lifecycle_outputs
            .as_slice()
            .iter()
            .filter_map(|fact| match fact {
                crate::sim::world::LifecycleOutput::LaserCreated(laser) => Some(*laser),
                _ => None,
            })
            .collect()
    }

    /// `LaserTrail.Type` resolves through the ART `[LaserTrailTypes]` catalog:
    /// one segment per `SegmentLength` of movement, with the type's fade,
    /// thickness and color.
    #[test]
    fn a_named_laser_trail_draws_one_segment_after_moving() {
        let art = crate::rules::ini_parser::IniFile::from_str(
            "[LaserTrailTypes]\n0=TRAIL\n[TRAIL]\nSegmentLength=64\nFadeDuration=5\n\
             Thickness=2\nColor=1,2,3\n",
        );
        let mut rules = crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(
            &crate::rules::ini_parser::IniFile::from_str(
                "[VehicleTypes]\n0=TANK\n[TANK]\nStrength=100\nSpeed=5\n\
                 AttachEffect.AttachTypes=AE\n\
                 [AttachEffectTypes]\n0=AE\n[AE]\nDuration=100\nLaserTrail.Type=TRAIL\n\
                 [Clear]\nBuildable=yes\n",
            ),
            &art,
        )
        .unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        let mut sim = Simulation::with_seed(3);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let tank = sim
            .spawn_object_at_height("TANK", "Neutral", 10, 10, 0, 0, &rules)
            .expect("TANK spawns");
        sim.update_phobos_laser_trails(tank, &rules);
        assert!(
            lasers(&sim).is_empty(),
            "the first update only seeds LastLocation"
        );
        sim.substrate
            .entities
            .get_mut(tank)
            .unwrap()
            .position
            .set_rx_for_test(11);
        sim.update_phobos_laser_trails(tank, &rules);
        let births = lasers(&sim);
        assert_eq!(births.len(), 1, "one SegmentLength crossing");
        assert_eq!(births[0].width, 2);
        assert_eq!(births[0].duration, 5);
        assert!(!births[0].supported);
        assert_eq!(
            births[0].color,
            crate::sim::combat::laser::LaserColor::HouseStyle([1, 2, 3]),
            "the trail type's Color is the DrawBeamSpecial RGB"
        );
        assert_eq!(births[0].from.y, births[0].to.y, "moved along X");
        sim.update_phobos_laser_trails(tank, &rules);
        assert_eq!(lasers(&sim).len(), 1, "no movement adds no segment");
    }

    fn transfer_rules(allow_transfer: &str, target_attach: &str) -> crate::rules::ruleset::RuleSet {
        let mut rules = crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(
            &crate::rules::ini_parser::IniFile::from_str(&format!(
                "[VehicleTypes]\n0=SMIN\n[BuildingTypes]\n0=YAREFN\n[InfantryTypes]\n0=SLAV\n\
                 [SMIN]\nStrength=100\nSpeed=3\nDeploysInto=YAREFN\nEnslaves=SLAV\n\
                 SlavesNumber=1\n\
                 [YAREFN]\nStrength=1000\nDeployFacing=0\nUndeploysInto=SMIN\n{target_attach}\
                 [SLAV]\nStrength=125\nSpeed=3\nStorage=4\n\
                 [AttachEffectTypes]\n0=AE\n[AE]\nDuration=100\n{allow_transfer}\n\
                 [Clear]\nBuildable=yes\n"
            )),
            &crate::rules::ini_parser::IniFile::from_str("[YAREFN]\nFoundation=1x1\n"),
        )
        .unwrap();
        // A building without a buildup SHP cannot be sold or undeployed.
        rules.set_buildup_control_for_test("YAREFN", [0, 17, 3]);
        rules
    }

    fn instances(sim: &Simulation, id: u64) -> usize {
        sim.substrate
            .entities
            .get(id)
            .expect("entity")
            .phobos_attach_effects()
            .instances()
            .len()
    }

    fn only_entity(sim: &Simulation, name: &str) -> u64 {
        sim.substrate
            .entities
            .values()
            .find(|entity| {
                entity.lifecycle.object_alive() && sim.interner.resolve(entity.type_ref()) == name
            })
            .expect("converted entity")
            .stable_id()
    }

    /// One externally attached `AE` instance with a distinct source pair.
    fn attach_external(sim: &mut Simulation, rules: &crate::rules::ruleset::RuleSet, id: u64) {
        let effect = rules.attach_effect_types().types()[0].clone();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .phobos_attach_effects_mut()
            .attach(
                0,
                AttachEffectParams::default(),
                &effect,
                AttachPolicy::default(),
                Some(AttachSource {
                    invoker: Some(id),
                    source: Some(2),
                    invoker_house: None,
                }),
            );
    }

    /// A cumulative type already on the target refreshes its first instance
    /// through `CreateAndAttach(checkCumulative=false)` instead of stacking,
    /// matching `TransferAttachedEffects` when no same-source match exists.
    #[test]
    fn cumulative_transfer_refreshes_an_existing_instance() {
        let rules = transfer_rules(
            "AllowTransfer=yes\nCumulative=yes",
            "AttachEffect.AttachTypes=AE\n",
        );
        let mut sim = Simulation::with_seed(7);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let mcv = sim
            .spawn_object_at_height("SMIN", "Neutral", 10, 10, 0, 0, &rules)
            .expect("SMIN spawns");
        attach_external(&mut sim, &rules, mcv);
        assert!(sim.deploy_mcv(
            mcv,
            &rules,
            None,
            &mut crate::sim::world::FrameEffects::default()
        ));
        let yard = only_entity(&sim, "YAREFN");
        assert_eq!(
            instances(&sim, yard),
            1,
            "the building's own instance absorbed the transfer"
        );
    }

    /// `AllowTransfer` at Phobos' deploy (`UnitClass_Deploy_Transfer
    /// 0x739956`) and undeploy (`BuildingClass_Mi_Selling_Transfer 0x44A03C`)
    /// hooks: a self-attached effect with `AllowTransfer=yes` follows the
    /// techno across both conversions, while `no` drops it.
    #[test]
    fn allow_transfer_moves_effects_across_deploy_and_undeploy() {
        for (allow, expected) in [("AllowTransfer=yes", 1), ("AllowTransfer=no", 0)] {
            let rules = transfer_rules(allow, "");
            let mut sim = Simulation::with_seed(7);
            crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
            let mcv = sim
                .spawn_object_at_height("SMIN", "Neutral", 10, 10, 0, 0, &rules)
                .expect("SMIN spawns");
            attach_external(&mut sim, &rules, mcv);
            assert_eq!(instances(&sim, mcv), 1, "one externally attached effect");
            assert!(sim.deploy_mcv(
                mcv,
                &rules,
                None,
                &mut crate::sim::world::FrameEffects::default()
            ));
            let yard = only_entity(&sim, "YAREFN");
            assert_eq!(instances(&sim, yard), expected, "deploy transfer ({allow})");
            sim.substrate
                .entities
                .get_mut(yard)
                .unwrap()
                .finish_building_construction_for_test();
            assert!(sim.undeploy_building(
                yard,
                false,
                &rules,
                None,
                &mut crate::sim::world::FrameEffects::default()
            ));
            sim.substrate
                .entities
                .get_mut(yard)
                .unwrap()
                .finish_pack_up_for_test();
            sim.advance_tick(&[], Some(&rules), None, None, 22);
            let unit = only_entity(&sim, "SMIN");
            assert_eq!(
                instances(&sim, unit),
                expected,
                "undeploy transfer ({allow})"
            );
        }
    }

    /// A transferred effect starts its trail at the target: `CreateAndAttach`
    /// builds a fresh `LaserTrailClass` (`AttachEffectClass.cpp:89`) and the
    /// transfer's sync list (`:1297..1315`) never names `LastLocation`, so the
    /// source's stored endpoint must not draw a segment across the deploy.
    #[test]
    fn a_transferred_effect_seeds_its_trail_at_the_target() {
        let art = crate::rules::ini_parser::IniFile::from_str(
            "[YAREFN]\nFoundation=1x1\n[LaserTrailTypes]\n0=TRAIL\n[TRAIL]\n\
             SegmentLength=64\nFadeDuration=5\nThickness=2\nColor=1,2,3\n",
        );
        let mut rules = crate::rules::ruleset::RuleSet::from_ini_with_fixed_art_for_test(
            &crate::rules::ini_parser::IniFile::from_str(
                "[VehicleTypes]\n0=SMIN\n[BuildingTypes]\n0=YAREFN\n[InfantryTypes]\n0=SLAV\n\
                 [SMIN]\nStrength=100\nSpeed=3\nDeploysInto=YAREFN\nEnslaves=SLAV\n\
                 SlavesNumber=1\n\
                 [YAREFN]\nStrength=1000\nDeployFacing=0\nUndeploysInto=SMIN\n\
                 [SLAV]\nStrength=125\nSpeed=3\nStorage=4\n\
                 [AttachEffectTypes]\n0=AE\n[AE]\nDuration=100\nAllowTransfer=yes\n\
                 LaserTrail.Type=TRAIL\n\
                 [Clear]\nBuildable=yes\n",
            ),
            &art,
        )
        .unwrap();
        rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
        rules.set_buildup_control_for_test("YAREFN", [0, 17, 3]);
        let mut sim = Simulation::with_seed(7);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let mcv = sim
            .spawn_object_at_height("SMIN", "Neutral", 10, 10, 0, 0, &rules)
            .expect("SMIN spawns");
        attach_external(&mut sim, &rules, mcv);
        sim.update_phobos_laser_trails(mcv, &rules);
        assert!(
            lasers(&sim).is_empty(),
            "the first update only seeds LastLocation"
        );
        // Move without a trail update, so the stored endpoint stays two cells
        // behind the cell the building deploys on.
        sim.substrate
            .entities
            .get_mut(mcv)
            .unwrap()
            .position
            .set_rx_for_test(12);
        assert!(sim.deploy_mcv(
            mcv,
            &rules,
            None,
            &mut crate::sim::world::FrameEffects::default()
        ));
        let yard = only_entity(&sim, "YAREFN");
        assert_eq!(instances(&sim, yard), 1, "the effect followed the deploy");
        sim.substrate
            .entities
            .get_mut(yard)
            .unwrap()
            .finish_building_construction_for_test();
        let before = lasers(&sim).len();
        sim.update_phobos_laser_trails(yard, &rules);
        assert_eq!(
            lasers(&sim).len(),
            before,
            "a fresh trail seeds at the building instead of drawing from the unit's old endpoint"
        );
    }

    /// A two-house arena built from the given rules text, with every type id
    /// and rule handle interned, ready for production spawns.
    fn arena_scene(text: &str, seed: u64) -> (Simulation, crate::rules::ruleset::RuleSet) {
        let ini = crate::sim::arena_fixture::clear_ground_ini(text);
        let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("rules parse");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        (sim, rules)
    }

    fn spawn(
        sim: &mut Simulation,
        rules: &crate::rules::ruleset::RuleSet,
        type_name: &str,
        owner: &str,
        rx: u16,
        ry: u16,
    ) -> u64 {
        sim.spawn_object(type_name, owner, rx, ry, 0, rules)
            .expect("arena spawn")
    }

    fn health(sim: &Simulation, id: u64) -> i32 {
        sim.substrate
            .entities
            .get(id)
            .expect("live entity")
            .health
            .current()
    }

    fn attach_count(sim: &Simulation, id: u64) -> usize {
        sim.substrate
            .entities
            .get(id)
            .expect("live entity")
            .phobos_attach_effects()
            .instances()
            .len()
    }

    fn direct_hit(
        target: u64,
        damage: i32,
        attacker: u64,
        warhead: crate::sim::intern::InternedId,
    ) -> crate::sim::combat::EntityDamageEvent {
        crate::sim::combat::EntityDamageEvent::direct_receiver(
            target,
            damage,
            0,
            attacker,
            None,
            warhead,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        )
    }

    /// Phobos `SuppressRevengeWeapons=` / `.Types=` on the killing warhead
    /// (`Ext/WarheadType/Body.cpp:356-357`;
    /// `Ext/Techno/WeaponHelpers.cpp:281-282`): the effect's revenge weapon is
    /// dropped for every weapon without a filter list, dropped for a listed
    /// weapon, and still fired when the list names another weapon.
    #[test]
    fn suppress_revenge_weapons_drops_the_effect_shot() {
        let run = |warhead_keys: &str| {
            let (mut sim, rules) = arena_scene(
                &format!(
                    "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
                     [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
                     [TARGET]\nStrength=50\nSpeed=6\nArmor=none\nAttachEffect.AttachTypes=REV\n\
                     [HIT]\n{ALL_HIT}{warhead_keys}\
                     [KILLW]\nDamage=100\nRange=1\nWarhead=HIT\n\
                     [REVW]\nDamage=7\nRange=1\nWarhead=HIT\n\
                     [OTHERW]\nDamage=11\nRange=1\nWarhead=HIT\n\
                     [AttachEffectTypes]\n0=REV\n[REV]\nDuration=-1\nRevengeWeapon=REVW\n"
                ),
                23,
            );
            let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
            let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
            sim.update_phobos_attach_effects(victim, &rules);
            assert_eq!(attach_count(&sim, victim), 1);
            let warhead = sim.interner.intern("HIT");
            let event = direct_hit(victim, 100, attacker, warhead);
            let _ = sim.commit_direct_damage_receiver(
                &rules,
                None,
                event,
                &mut crate::sim::world::FrameEffects::empty(),
            );
            health(&sim, attacker)
        };
        // The killing warhead's own `HIT` carries no suppression: 1000 - 7.
        assert_eq!(run(""), 993);
        // No filter list: every revenge weapon is suppressed.
        assert_eq!(run("SuppressRevengeWeapons=yes\n"), 1000);
        // The filter names the fired weapon.
        assert_eq!(
            run("SuppressRevengeWeapons=yes\nSuppressRevengeWeapons.Types=REVW\n"),
            1000
        );
        // The filter names another weapon, so the shot still lands.
        assert_eq!(
            run("SuppressRevengeWeapons=yes\nSuppressRevengeWeapons.Types=OTHERW\n"),
            993
        );
    }

    /// Phobos `WeaponTypeExt::GetRangeWithModifiers` veteran arm:
    /// `HasAdditionalAbility(Range)` at veteran/elite multiplies by the type's
    /// `VeteranRange` else the `[General]` value (max with 0); a rookie is
    /// untouched. The explicit-range overload scales even `-512`; only the
    /// two-argument wrapper owns the sentinel.
    #[test]
    fn the_veteran_range_ability_scales_get_range_with_modifiers() {
        use crate::rules::ini_parser::IniFile;
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
                "[Countries]
0=Americans
[VehicleTypes]
0=VET
1=PLAIN
2=ELITEONLY
                 [WeaponTypes]
0=GUN
[GUN]
Range=5
Damage=10
ROF=20
                 [VET]
Strength=1000
Speed=6
Primary=GUN
VeteranAbilities=RANGE
VeteranRange=1.5
                 [PLAIN]
Strength=1000
Speed=6
Primary=GUN
VeteranAbilities=RANGE
                 [ELITEONLY]
Strength=1000
Speed=6
Primary=GUN
EliteAbilities=RANGE
                 [General]
VeteranRange=2.0
",
            ))
            .expect("rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let spawn = |sim: &mut Simulation, kind: &str, rank: u16, rx: u16| {
            let id = sim
                .spawn_object(kind, "Americans", rx, 20, 0, &rules)
                .unwrap_or_else(|| panic!("spawn {kind}"));
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .veterancy
                .set_rank_for_test(rank);
            id
        };
        let range = |sim: &Simulation, id: u64, base: i32| {
            super::get_range_with_modifiers(
                &rules,
                &sim.interner,
                Some(&sim.substrate.entities),
                sim.substrate.entities.get(id).unwrap(),
                "GUN",
                base,
            )
        };
        let vet = spawn(&mut sim, "VET", 0, 20);
        assert_eq!(range(&sim, vet, 1000), 1000, "a rookie has no ability");
        sim.substrate
            .entities
            .get_mut(vet)
            .unwrap()
            .veterancy
            .set_rank_for_test(100);
        assert_eq!(range(&sim, vet, 1000), 1500, "the type's VeteranRange");
        sim.substrate
            .entities
            .get_mut(vet)
            .unwrap()
            .veterancy
            .set_rank_for_test(200);
        assert_eq!(range(&sim, vet, 1000), 1500, "elite keeps the veteran tier");
        let plain = spawn(&mut sim, "PLAIN", 100, 22);
        assert_eq!(range(&sim, plain, 1000), 2000, "the global VeteranRange");
        let elite_only = spawn(&mut sim, "ELITEONLY", 100, 24);
        assert_eq!(
            range(&sim, elite_only, 1000),
            1000,
            "a veteran does not read the elite tier"
        );
        sim.substrate
            .entities
            .get_mut(elite_only)
            .unwrap()
            .veterancy
            .set_rank_for_test(200);
        assert_eq!(range(&sim, elite_only, 1000), 2000);
        assert_eq!(
            range(&sim, plain, -512),
            -1024,
            "the explicit-range overload has no sentinel arm"
        );
        let mut sentinel_weapon = rules.weapon("GUN").expect("GUN").clone();
        sentinel_weapon.range_leptons =
            crate::util::lepton::WEAPON_RANGE_ALWAYS_IN_RANGE_LEPTONS as i32;
        assert_eq!(
            super::weapon_range_with_modifiers(
                &rules,
                &sim.interner,
                Some(&sim.substrate.entities),
                sim.substrate.entities.get(plain).unwrap(),
                &sentinel_weapon,
            ),
            -512,
            "the two-argument wrapper keeps the sentinel"
        );
    }

    /// Phobos `ProjectileRange.ApplyModifiers` gates the fold for the
    /// projectile's initial range: the weapon's nullable key else the
    /// `[CombatDamage]` global (constructor true).
    #[test]
    fn initial_projectile_range_reads_the_global_and_weapon_keys() {
        use crate::rules::ini_parser::IniFile;
        let scene = |global: &str, weapon_key: &str, base: i32| {
            let rules = crate::rules::ruleset::RuleSet::from_ini(
                &crate::sim::arena_fixture::clear_ground_ini(&format!(
                    "[Countries]
0=Americans
[VehicleTypes]
0=VET
                     [WeaponTypes]
0=GUN
                     [GUN]
Range=5
Damage=10
ROF=20
{weapon_key}
                     [VET]
Strength=1000
Speed=6
Primary=GUN
VeteranAbilities=RANGE
                     VeteranRange=1.5
                     [CombatDamage]
{global}
[General]
VeteranRange=2.0
"
                )),
            )
            .expect("rules");
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let id = sim
                .spawn_object("VET", "Americans", 20, 20, 0, &rules)
                .expect("spawn");
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .veterancy
                .set_rank_for_test(100);
            let firer = sim.substrate.entities.get(id).unwrap();
            super::initial_projectile_range(
                &rules,
                &sim.interner,
                Some(&sim.substrate.entities),
                firer,
                rules.weapon("GUN").unwrap(),
                base,
            )
        };
        assert_eq!(
            scene("", "", 1000),
            1500,
            "the constructor's true applies the fold"
        );
        assert_eq!(
            scene("ProjectileRange.ApplyModifiers=no", "", 1000),
            1000,
            "the global off skips the fold"
        );
        assert_eq!(
            scene(
                "ProjectileRange.ApplyModifiers=no",
                "ProjectileRange.ApplyModifiers=yes",
                1000
            ),
            1500,
            "the weapon key overrides the global"
        );
        assert_eq!(
            scene("", "", 0),
            0,
            "the native maxRange > 0 guard precedes the key"
        );
        assert_eq!(scene("", "", -300), -300, "a negative range is untouched");
        assert_eq!(
            scene("", "", -512),
            -512,
            "no sentinel arm on the bullet gate"
        );
    }

    /// Phobos `TechnoExt::InitializeAttachEffects`'s second call
    /// (`Body.Internal.cpp:157`): the owner country's
    /// `AttachEffect.AttachTypes` list attaches at creation, with the country's
    /// own paired duration override.
    #[test]
    fn a_country_attach_types_list_attaches_at_creation() {
        use crate::rules::ini_parser::IniFile;
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[Countries]
0=Americans
             [Americans]
AttachEffect.AttachTypes=CTRY
             AttachEffect.DurationOverrides=7
             [VehicleTypes]
0=TANK
             [TANK]
Strength=100
Speed=5
             [AttachEffectTypes]
0=CTRY
[CTRY]
Duration=100
",
        ))
        .expect("country attach rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 9);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        sim.houses
            .get_mut(&owner)
            .unwrap()
            .set_country_for_test(Some(owner));
        let tank = sim
            .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
            .expect("TANK spawns");
        let effects = sim
            .substrate
            .entities
            .get(tank)
            .unwrap()
            .phobos_attach_effects();
        assert_eq!(effects.instances.len(), 1, "the country list attached");
        assert_eq!(effects.instances[0].remaining(), 7, "the country override");
    }

    /// Phobos `House/Hooks.cpp:342-343`: an owner change attaches the new
    /// country's list when `AttachEffect.AttachOnOwnerChange` allows it.
    #[test]
    fn a_new_country_attaches_its_list_on_owner_change() {
        use crate::rules::ini_parser::IniFile;
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[Countries]
0=Americans
1=Russians
             [Americans]
             [Russians]
AttachEffect.AttachTypes=CTRY
             AttachEffect.AttachOnOwnerChange=yes
             [VehicleTypes]
0=TANK
             [TANK]
Strength=100
Speed=5
             [AttachEffectTypes]
0=CTRY
[CTRY]
Duration=100
",
        ))
        .expect("owner-change attach rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 11);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let russians = sim.interner.intern("Russians");
        sim.houses
            .get_mut(&russians)
            .unwrap()
            .set_country_for_test(Some(russians));
        let tank = sim
            .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
            .expect("TANK spawns");
        assert_eq!(attach_count(&sim, tank), 0, "the old country has no list");
        sim.change_owner_with_rules(
            tank,
            russians,
            &rules,
            None,
            &mut crate::sim::world::FrameEffects::empty(),
        );
        assert_eq!(
            attach_count(&sim, tank),
            1,
            "the new country's list attached"
        );
        assert_eq!(sim.substrate.entities.get(tank).unwrap().owner(), russians);
    }

    /// Phobos `AttachEffectClass::UpdateCumulativeAnim`
    /// (`New/Entity/AttachEffectClass.cpp:520-526`): a layer count that picks
    /// another body retypes the running animation in place through
    /// `AnimExt::ChangeAnimType` (`Ext/Anim/Body.cpp:198-243`) instead of
    /// destroying and re-creating it. With `RestartOnChange=no` the retype
    /// keeps the fraction the old body had travelled.
    #[test]
    fn a_cumulative_layer_change_retypes_the_same_animation_in_place() {
        let (mut sim, rules) = cumulative_anim_scene(
            "CumulativeAnimations=AEONE,AETWO\nCumulativeAnimations.RestartOnChange=no\n",
            21,
        );
        let target = spawn(&mut sim, &rules, "TARGET", "Americans", 20, 20);
        sim.update_phobos_attach_effects(target, &rules);
        let (anim, name, _frame) = cumulative_anim(&sim, target);
        assert_eq!(name, "AEONE", "one layer picks the first body");
        // Half of the eight frames behind us.
        sim.substrate
            .anims
            .get_mut(anim)
            .expect("the running effect animation")
            .runtime_mut_for_test()
            .current_frame = 4;
        add_layer_from_warhead(&mut sim, "ADDCUM", target, &rules);
        assert_eq!(attach_count(&sim, target), 2, "the second layer attached");
        sim.update_phobos_attach_effects(target, &rules);
        let (after, name, frame) = cumulative_anim(&sim, target);
        assert_eq!(after, anim, "the retype keeps the same anim instance");
        assert_eq!(name, "AETWO", "two layers pick the second body");
        assert_eq!(
            frame, 6,
            "int(End=12 * fraction 4/8) keeps the travelled fraction"
        );
    }

    /// The same retype with the key's default (`RestartOnChange=yes`) re-arms
    /// the new body at its first frame.
    #[test]
    fn a_cumulative_layer_change_restarts_when_the_type_asks_for_it() {
        let (mut sim, rules) = cumulative_anim_scene("CumulativeAnimations=AEONE,AETWO\n", 22);
        let target = spawn(&mut sim, &rules, "TARGET", "Americans", 20, 20);
        sim.update_phobos_attach_effects(target, &rules);
        let (anim, name, _frame) = cumulative_anim(&sim, target);
        assert_eq!(name, "AEONE");
        sim.substrate
            .anims
            .get_mut(anim)
            .expect("the running effect animation")
            .runtime_mut_for_test()
            .current_frame = 4;
        add_layer_from_warhead(&mut sim, "ADDCUM", target, &rules);
        sim.update_phobos_attach_effects(target, &rules);
        let (after, name, frame) = cumulative_anim(&sim, target);
        assert_eq!(after, anim, "the default still retypes in place");
        assert_eq!(name, "AETWO");
        assert_eq!(frame, 0, "RestartOnChange=yes re-arms the first frame");
    }

    /// `TARGET` attaches one `CUM` layer; `ADDCUM` adds a second.
    fn cumulative_anim_scene(
        effect_keys: &str,
        seed: u64,
    ) -> (Simulation, crate::rules::ruleset::RuleSet) {
        let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
            "[Countries]\n0=Americans\n1=Russians\n\
             [VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\n\
             AttachEffect.AttachTypes=CUM\n\
             [Warheads]\n0=ADDCUM\n\
             [ADDCUM]\n{ALL_HIT}AttachEffect.AttachTypes=CUM\n\
             AttachEffect.DurationOverrides=50\n\
             [AttachEffectTypes]\n0=CUM\n\
             [CUM]\nDuration=100\nCumulative=yes\n{effect_keys}"
        ));
        let mut rules =
            crate::rules::ruleset::RuleSet::from_ini(&ini).expect("cumulative anim rules");
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(
                "[AEONE]\nEnd=8\nRate=2\n[AETWO]\nEnd=12\nRate=2\n",
            ),
        );
        art.bind_anim_frame_count_for_test("AEONE", 8);
        art.bind_anim_frame_count_for_test("AETWO", 12);
        rules.replace_art_registry_for_test(art);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        (sim, rules)
    }

    /// The designated cumulative instance's animation: its id, type name and
    /// committed frame.
    fn cumulative_anim(
        sim: &Simulation,
        target: u64,
    ) -> (crate::sim::anim_class::AnimId, String, i32) {
        let list = sim
            .substrate
            .entities
            .get(target)
            .expect("live entity")
            .phobos_attach_effects();
        let anim = list
            .instances()
            .iter()
            .find_map(|instance| instance.anim)
            .expect("the cumulative effect shows an animation");
        let live = sim.substrate.anims.get(anim).expect("live animation");
        (
            anim,
            sim.interner.resolve(live.type_id()).to_owned(),
            live.runtime_frame_for_test(),
        )
    }

    /// `AttachEffectClass::CanShowAnim` (`New/Entity/AttachEffectClass.h:43-50`)
    /// with the tunnel hooks (`Ext/Techno/Hooks.Misc.cpp:487-527`): an effect
    /// animation is killed once the tunnel reaches its dug-in state and comes
    /// back when the unit starts to surface.
    #[test]
    fn a_dug_in_tunnel_hides_and_restores_its_effect_animation() {
        let (mut sim, rules) = tunnel_anim_scene("", 51);
        let target = spawn(&mut sim, &rules, "MOLE", "Americans", 20, 20);
        sim.update_phobos_attach_effects(target, &rules);
        let first = effect_anim_id(&sim, target).expect("the effect shows an animation");
        set_tunnel_state(
            &mut sim,
            target,
            crate::sim::movement::TunnelState::DiggingIn,
        );
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(
            effect_anim_id(&sim, target),
            Some(first),
            "digging in still shows the animation"
        );
        set_tunnel_state(&mut sim, target, crate::sim::movement::TunnelState::DugIn);
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(
            effect_anim_id(&sim, target),
            None,
            "the dug-in transition kills the animation"
        );
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(
            effect_anim_id(&sim, target),
            None,
            "CanShowAnim keeps it hidden while travelling"
        );
        set_tunnel_state(
            &mut sim,
            target,
            crate::sim::movement::TunnelState::PreDigOut,
        );
        sim.update_phobos_attach_effects(target, &rules);
        let back = effect_anim_id(&sim, target).expect("surfacing restores the animation");
        assert_ne!(back, first, "the restored animation is a new instance");
    }

    /// A `Tunnel` locomotor target carrying one `Duration=100` effect whose
    /// `Animation=AEONE`.
    fn tunnel_anim_scene(
        art_keys: &str,
        seed: u64,
    ) -> (Simulation, crate::rules::ruleset::RuleSet) {
        let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
            "[Countries]\n0=Americans\n1=Russians\n\
             [VehicleTypes]\n0=MOLE\n\
             [MOLE]\nStrength=1000\nSpeed=6\nArmor=none\nLocomotor=Tunnel\n\
             AttachEffect.AttachTypes=TUNNELE\n\
             [AttachEffectTypes]\n0=TUNNELE\n\
             [TUNNELE]\nDuration=100\nAnimation=AEONE\n"
        ));
        let mut rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("tunnel anim rules");
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(&format!(
                "[AEONE]\nEnd=8\nRate=2\n{art_keys}"
            )),
        );
        art.bind_anim_frame_count_for_test("AEONE", 8);
        rules.replace_art_registry_for_test(art);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        (sim, rules)
    }

    fn set_tunnel_state(sim: &mut Simulation, id: u64, state: crate::sim::movement::TunnelState) {
        sim.substrate
            .entities
            .get_mut(id)
            .expect("live entity")
            .locomotor
            .as_mut()
            .expect("Locomotor=Tunnel carries the runtime")
            .tunnel_runtime_mut_for_test()
            .expect("the Tunnel locomotor payload")
            .set_state_and_timer_for_test(state, 0, 0);
    }

    /// `AttachEffectClass::CloakCheck` (`New/Entity/AttachEffectClass.cpp:446-453`)
    /// with `CreateAnim`'s matching gate (`:476-477`): a cloaked owner kills an
    /// effect animation whose type asks for `DetachOnCloak` and refuses to
    /// create a new one until the object uncloaks.
    #[test]
    fn a_cloaked_owner_hides_a_detach_on_cloak_effect_animation() {
        let (mut sim, rules) = cloak_anim_scene("", 41);
        let target = spawn(&mut sim, &rules, "TARGET", "Americans", 20, 20);
        sim.update_phobos_attach_effects(target, &rules);
        let first = effect_anim_id(&sim, target).expect("the effect shows an animation");
        set_cloak_state(&mut sim, target, 2);
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(
            effect_anim_id(&sim, target),
            None,
            "Cloaked kills the DetachOnCloak animation"
        );
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(
            effect_anim_id(&sim, target),
            None,
            "and CreateAnim keeps refusing while cloaked"
        );
        set_cloak_state(&mut sim, target, 0);
        sim.update_phobos_attach_effects(target, &rules);
        let back = effect_anim_id(&sim, target).expect("uncloaking recreates the animation");
        assert_ne!(back, first, "the recreated animation is a new instance");
    }

    /// `DetachOnCloak=no` (`Ext/AnimType/Body.cpp:112`, default true): the
    /// animation type keeps its effect animation while the owner is cloaked.
    #[test]
    fn a_detach_on_cloak_opt_out_keeps_the_effect_animation_while_cloaked() {
        let (mut sim, rules) = cloak_anim_scene("DetachOnCloak=no\n", 42);
        let target = spawn(&mut sim, &rules, "TARGET", "Americans", 20, 20);
        sim.update_phobos_attach_effects(target, &rules);
        let first = effect_anim_id(&sim, target).expect("the effect shows an animation");
        set_cloak_state(&mut sim, target, 2);
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(
            effect_anim_id(&sim, target),
            Some(first),
            "the opt-out keeps the same animation"
        );
        set_cloak_state(&mut sim, target, 0);
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(effect_anim_id(&sim, target), Some(first));
    }

    /// A `Cloakable=yes` target that carries one `Duration=100` effect whose
    /// `Animation=AEONE`; the art keys decide the animation type's cloak flag.
    fn cloak_anim_scene(art_keys: &str, seed: u64) -> (Simulation, crate::rules::ruleset::RuleSet) {
        let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
            "[General]\nCloakingStages=6\n\
             [Countries]\n0=Americans\n1=Russians\n\
             [VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\n\
             Cloakable=yes\nCloakingSpeed=3\n\
             AttachEffect.AttachTypes=CLOAKAE\n\
             [AttachEffectTypes]\n0=CLOAKAE\n\
             [CLOAKAE]\nDuration=100\nAnimation=AEONE\n"
        ));
        let mut rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("cloak anim rules");
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(&format!(
                "[AEONE]\nEnd=8\nRate=2\n{art_keys}"
            )),
        );
        art.bind_anim_frame_count_for_test("AEONE", 8);
        rules.replace_art_registry_for_test(art);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        (sim, rules)
    }

    fn set_cloak_state(sim: &mut Simulation, id: u64, state: i32) {
        sim.substrate
            .entities
            .get_mut(id)
            .expect("live entity")
            .cloak
            .as_mut()
            .expect("Cloakable=yes carries the cloak runtime")
            .set_state_for_test(state);
    }

    fn effect_anim_id(sim: &Simulation, id: u64) -> Option<crate::sim::anim_class::AnimId> {
        sim.substrate
            .entities
            .get(id)
            .expect("live entity")
            .phobos_attach_effects()
            .instances()
            .iter()
            .find_map(|instance| instance.anim)
    }

    /// One detonation of the `ADDCUM` warhead on `target`.
    fn add_layer_from_warhead(
        sim: &mut Simulation,
        warhead: &str,
        target: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let warhead = sim.interner.intern(warhead);
        let impacted = {
            let entity = sim.substrate.entities.get(target).expect("live entity");
            crate::sim::movement::ground_pose::object_get_coords(
                entity,
                sim.resolved_terrain.as_ref(),
            )
        };
        let detonation = crate::sim::projectile::ProjectileDetonation {
            projectile_id: u64::MAX,
            source_id: target,
            target: crate::sim::projectile::ProjectileTarget::Entity(target),
            impact: crate::sim::projectile::ProjectileCoord::new(
                impacted.x, impacted.y, impacted.z,
            ),
            payload: crate::sim::projectile::ProjectilePayload::new(1, warhead, None, false),
            reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
        };
        sim.commit_attach_effect_detonations(
            &[detonation],
            rules,
            None,
            &mut crate::sim::world::FrameEffects::empty(),
        );
    }

    /// `Animation.ResetOnReapply` (`New/Entity/AttachEffectClass.cpp:549-555`
    /// and `:928-934`): a reapply of a non-cumulative effect kills the live
    /// animation and creates a fresh one.
    #[test]
    fn a_reapply_restarts_the_animation_when_reset_on_reapply_is_set() {
        let (mut sim, rules) = reapply_anim_scene("Animation.ResetOnReapply=yes\n", 31);
        let target = spawn(&mut sim, &rules, "TARGET", "Americans", 20, 20);
        add_layer_from_warhead(&mut sim, "REAPPLY", target, &rules);
        sim.update_phobos_attach_effects(target, &rules);
        let (first, name, _frame) = cumulative_anim(&sim, target);
        assert_eq!(name, "AEONE");
        sim.substrate
            .anims
            .get_mut(first)
            .expect("the running effect animation")
            .runtime_mut_for_test()
            .current_frame = 4;
        // Half of the remaining duration: the refresh arm is skipped, the
        // animation restart is not.
        add_layer_from_warhead(&mut sim, "REAPPLY", target, &rules);
        assert_eq!(attach_count(&sim, target), 1, "the instance absorbed it");
        sim.update_phobos_attach_effects(target, &rules);
        let (second, name, frame) = cumulative_anim(&sim, target);
        assert_eq!(name, "AEONE");
        assert_ne!(second, first, "KillAnim + CreateAnim made a new animation");
        assert_eq!(frame, 0, "the fresh animation starts at its first frame");
    }

    /// The key's default (`Animation.ResetOnReapply` unset) keeps the running
    /// animation across a reapply.
    #[test]
    fn a_reapply_keeps_the_animation_without_reset_on_reapply() {
        let (mut sim, rules) = reapply_anim_scene("", 32);
        let target = spawn(&mut sim, &rules, "TARGET", "Americans", 20, 20);
        add_layer_from_warhead(&mut sim, "REAPPLY", target, &rules);
        sim.update_phobos_attach_effects(target, &rules);
        let (first, name, _frame) = cumulative_anim(&sim, target);
        assert_eq!(name, "AEONE");
        sim.substrate
            .anims
            .get_mut(first)
            .expect("the running effect animation")
            .runtime_mut_for_test()
            .current_frame = 4;
        add_layer_from_warhead(&mut sim, "REAPPLY", target, &rules);
        sim.update_phobos_attach_effects(target, &rules);
        let (after, _, frame) = cumulative_anim(&sim, target);
        assert_eq!(after, first, "the same animation keeps running");
        assert_eq!(frame, 4, "its frame position survives the reapply");
    }

    /// `Animation.ResetOnReapply` on the shorter-duration reapply arm
    /// (`AttachEffectClass.cpp:928-934`): the duration is not refreshed, yet
    /// the animation still restarts.
    #[test]
    fn a_shorter_reapply_still_restarts_the_animation() {
        let (mut sim, rules) = reapply_anim_scene("Animation.ResetOnReapply=yes\n", 33);
        let target = spawn(&mut sim, &rules, "TARGET", "Americans", 20, 20);
        add_layer_from_warhead(&mut sim, "REAPPLY", target, &rules);
        sim.update_phobos_attach_effects(target, &rules);
        let (first, name, _frame) = cumulative_anim(&sim, target);
        assert_eq!(name, "AEONE");
        let remaining = {
            let instances = sim
                .substrate
                .entities
                .get(target)
                .expect("live entity")
                .phobos_attach_effects()
                .instances();
            instances[0].remaining()
        };
        assert!(remaining > 50, "the override is shorter than what is left");
        sim.substrate
            .anims
            .get_mut(first)
            .expect("the running effect animation")
            .runtime_mut_for_test()
            .current_frame = 4;
        add_layer_from_warhead(&mut sim, "SHORTER", target, &rules);
        let remaining_after = {
            let instances = sim
                .substrate
                .entities
                .get(target)
                .expect("live entity")
                .phobos_attach_effects()
                .instances();
            instances[0].remaining()
        };
        assert_eq!(
            remaining_after, remaining,
            "the shorter hit kept the duration"
        );
        sim.update_phobos_attach_effects(target, &rules);
        let (second, _, frame) = cumulative_anim(&sim, target);
        assert_ne!(second, first, "the animation still restarted");
        assert_eq!(frame, 0);
    }

    /// `TARGET` carries no effect; `REAPPLY` attaches one `AE` (duration 100)
    /// and `SHORTER` attaches the same type with `DurationOverrides=50`, so the
    /// second hit takes the reapply arm that does not refresh the duration.
    fn reapply_anim_scene(
        effect_keys: &str,
        seed: u64,
    ) -> (Simulation, crate::rules::ruleset::RuleSet) {
        let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
            "[Countries]\n0=Americans\n1=Russians\n\
             [VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\n\
             [Warheads]\n0=REAPPLY\n1=SHORTER\n\
             [REAPPLY]\n{ALL_HIT}AttachEffect.AttachTypes=AE\n\
             [SHORTER]\n{ALL_HIT}AttachEffect.AttachTypes=AE\n\
             AttachEffect.DurationOverrides=50\n\
             [AttachEffectTypes]\n0=AE\n\
             [AE]\nDuration=100\nAnimation=AEONE\n{effect_keys}"
        ));
        let mut rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("reapply anim rules");
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(
            &crate::rules::ini_parser::IniFile::from_str("[AEONE]\nEnd=8\nRate=2\n"),
        );
        art.bind_anim_frame_count_for_test("AEONE", 8);
        rules.replace_art_registry_for_test(art);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        (sim, rules)
    }
}

#[cfg(all(test, tests_sim_a_b))]
mod tests;
