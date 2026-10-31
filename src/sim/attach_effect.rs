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

use crate::rules::attach_effect::{
    AttachEffectType, AttachedAnimFlag, DiscardOn, ExpireWeaponCondition,
};
#[cfg(feature = "phobos")]
use crate::rules::phobos_shields::AffectedHouse;
#[cfg(feature = "phobos")]
use crate::sim::world::Simulation;

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
    #[cfg(feature = "phobos")]
    #[serde(default)]
    anim: Option<u64>,
    /// `ShouldBeDiscarded`: set by the effect's event conditions (a fired
    /// count, received damage, entry, owner change, invoker death).
    #[cfg(feature = "phobos")]
    #[serde(default)]
    should_be_discarded: bool,
    /// `FiringCount`: shots counted towards `DiscardOn.Firing.Count`.
    #[cfg(feature = "phobos")]
    #[serde(default)]
    firing_count: i32,
    /// `ReceivedDamageCount`: hits counted towards
    /// `DiscardOn.ReceivedDamage.Count`.
    #[cfg(feature = "phobos")]
    #[serde(default)]
    received_damage_count: i32,
    /// `LastSequenceCheck`: the infantry art sequence the previous frame
    /// played, for the deferred `DiscardOn.Sequences` form.
    #[cfg(feature = "phobos")]
    #[serde(default)]
    last_sequence_check: Option<crate::rules::animation_sequence::SequenceKind>,
    /// `ArmorMultiplierTimer`: a real hit that applied a restricted
    /// `ArmorMultiplier` with `.Delay > 0` blocks further rolls until it runs
    /// out (`TechnoHooks.cpp:552`).
    #[cfg(feature = "phobos")]
    #[serde(default)]
    armor_multiplier_timer: crate::sim::timer::CdTimer,
    /// `ReflectDamageTimer`: a reflection under `.Delay > 0` blocks the next
    /// one until it runs out (`Hooks.ReceiveDamage.cpp:429`).
    #[cfg(feature = "phobos")]
    #[serde(default)]
    reflect_damage_timer: crate::sim::timer::CdTimer,
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
            #[cfg(feature = "phobos")]
            anim: None,
            #[cfg(feature = "phobos")]
            should_be_discarded: false,
            #[cfg(feature = "phobos")]
            firing_count: 0,
            #[cfg(feature = "phobos")]
            received_damage_count: 0,
            #[cfg(feature = "phobos")]
            last_sequence_check: None,
            #[cfg(feature = "phobos")]
            armor_multiplier_timer: crate::sim::timer::CdTimer::default(),
            #[cfg(feature = "phobos")]
            reflect_damage_timer: crate::sim::timer::CdTimer::default(),
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
    #[cfg(feature = "phobos")]
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
    }

    /// `RefreshDuration` for a caller that already folded the duration
    /// multipliers into `duration`.
    pub fn refresh_with(&mut self, duration: i32) {
        self.remaining = duration.max(0);
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
    #[cfg(feature = "phobos")]
    pub fn is_flagged_discarded(&self) -> bool {
        self.should_be_discarded
    }

    #[cfg(feature = "phobos")]
    pub(crate) fn mark_discarded(&mut self) {
        self.should_be_discarded = true;
    }

    #[cfg(feature = "phobos")]
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
#[cfg(feature = "phobos")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttachEffectWeaponShot {
    pub weapon: String,
    pub invoker: Option<u64>,
    /// The firing house the shot carries (`InvokerHouse`, or the effect
    /// owner's house for an owner shot).
    pub house: Option<crate::sim::intern::InternedId>,
}

#[cfg(feature = "phobos")]
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
    #[cfg(feature = "phobos")]
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
                if policy.replace_longer_duration || duration >= instance.remaining {
                    self.instances[index].refresh_with(duration);
                }
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
                    }
                } else if let Some(index) = shortest
                    && (policy.replace_longer_duration
                        || duration >= self.instances[index].remaining)
                {
                    self.instances[index].refresh_with(duration);
                }
                return AttachOutcome::Refreshed;
            } else if policy.cumulative_refresh_all && policy.cumulative_refresh_all_on_attach {
                // `CumulativeRefreshAll.OnAttach` refreshes under the cap and
                // still appends the new instance.
                for index in matches {
                    self.instances[index].refresh_with(duration);
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
#[cfg(feature = "phobos")]
fn paired(list: &[i32], index: usize, default: i32) -> i32 {
    list.get(index)
        .or_else(|| list.last())
        .copied()
        .unwrap_or(default)
}

/// `AttachEffectClass::CreateAndAttach` (`AttachEffectClass.cpp:883..898`)
/// gates every attach site before the cumulative decision: an active Iron
/// Curtain or Force Shield needs the type's matching penetration flag,
/// `AffectsTarget` must admit the target's class, and `AffectTypes` (when
/// non-empty) then `IgnoreTypes` filter the target's type id.
#[cfg(feature = "phobos")]
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
    if !effect.affects_target.allows(category) {
        return false;
    }
    if !effect.affect_types.is_empty()
        && !effect
            .affect_types
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&object.id))
    {
        return false;
    }
    !effect
        .ignore_types
        .iter()
        .any(|name| name.eq_ignore_ascii_case(&object.id))
}

/// Phobos attaches a techno type's `AttachEffect.AttachTypes` once when the
/// entity is created, pairing `DurationOverrides`, `Delays`, `InitialDelays`
/// and `RecreationDelays` by index. `source` is the invoking pair when the
/// attachment came from outside (a warhead or weapon); a type's own list has
/// none. Each entry passes [`can_attach_effect`] first, as `CreateAndAttach`
/// does for every attach site.
#[cfg(feature = "phobos")]
pub fn attach_initial_effects(
    list: &mut AttachEffectList,
    category: crate::rules::object_type::ObjectCategory,
    invulnerability: Option<&crate::sim::superweapon::invulnerability::InvulnerabilityState>,
    object: &crate::rules::object_type::ObjectType,
    rules: &crate::rules::ruleset::RuleSet,
    frame: u32,
    source: Option<AttachSource>,
) -> usize {
    let names = object.attach_effect_attach_types();
    if names.is_empty() {
        return 0;
    }
    let overrides = object.attach_effect_duration_overrides();
    let delays = object.attach_effect_delays();
    let initial_delays = object.attach_effect_initial_delays();
    let recreation_delays = object.attach_effect_recreation_delays();
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
/// the `int` range, then MSVC's cast.
#[cfg(feature = "phobos")]
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

/// `WeaponTypeExt::GetRangeWithModifiers` (`WeaponType/Body.cpp:398..443`):
/// the entity whose AttachEffect range fold applies. A passenger inside an
/// `OpenTopped` transport whose type enables
/// `OpenTopped.UseTransportRangeModifiers` (defaulting to `[General]`) folds
/// the transport's active effects instead of its own. `None` entities
/// (mapless probes) keep the firer.
#[cfg(feature = "phobos")]
pub(crate) fn range_modifier_source<'a>(
    entity: &'a crate::sim::game_entity::GameEntity,
    entities: Option<&'a crate::sim::entity_store::EntityStore>,
    rules: &crate::rules::ruleset::RuleSet,
    interner: &crate::sim::intern::StringInterner,
) -> &'a crate::sim::game_entity::GameEntity {
    let Some(entities) = entities else {
        return entity;
    };
    let Some(transport_id) = entity.passenger_role.inside_transport_id() else {
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

#[cfg(not(feature = "phobos"))]
pub(crate) fn range_modifier_source<'a>(
    entity: &'a crate::sim::game_entity::GameEntity,
    _entities: Option<&'a crate::sim::entity_store::EntityStore>,
    _rules: &crate::rules::ruleset::RuleSet,
    _interner: &crate::sim::intern::StringInterner,
) -> &'a crate::sim::game_entity::GameEntity {
    entity
}

/// Phobos `WeaponTypeExt::GetRangeWithModifiers`'s AttachEffect arm: every
/// active effect whose weapon lists admit `weapon` multiplies the range (in
/// leptons, clamped) and sums its `WeaponRange.ExtraRange`; the summed cells
/// are converted to leptons and added after every multiplier, and the result
/// floors at zero. The `-512` always-in-range sentinel returns before the
/// fold, and a type with nothing to add leaves the range untouched.
/// `entity` is the fold source [`range_modifier_source`] resolved.
pub(crate) fn range_with_attach_effects(
    entity: &crate::sim::game_entity::GameEntity,
    weapon: &str,
    range: i32,
    rules: &crate::rules::ruleset::RuleSet,
) -> i32 {
    #[cfg(feature = "phobos")]
    {
        const WEAPON_RANGE_ALWAYS_IN_RANGE_LEPTONS: i32 = -512;
        if range == WEAPON_RANGE_ALWAYS_IN_RANGE_LEPTONS {
            return range;
        }
        if !entity.phobos_stat_multipliers().has_range_modifier {
            return range;
        }
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
        // `Math::max(range, 0)` (`WeaponType/Body.cpp:467`).
        range.wrapping_add(cpp_i32(extra_range * 256.0)).max(0)
    }
    #[cfg(not(feature = "phobos"))]
    {
        let _ = (entity, weapon, rules);
        range
    }
}

/// Phobos `AttachEffectTechnoProperties`'s cached stat fold
/// (`TechnoExt::RecalculateStatMultipliers`, Body.Update.cpp:1124). The
/// instance list stays the source of truth; this is recomputed from it every
/// AI frame, restored from the save like Phobos' own `TechnoExt::AE`, and
/// never enters the state hash.
///
/// RESIDUALS: the shield pre-stage still divides with the unrestricted armor
/// product and the receiver's restricted roll runs before the receiver's rare
/// early returns (see the two methods); `disable_weapons` needs the
/// `GetFireError` hooks at `0x006FC0C5` (REARM, unless the firer's
/// `FootClass+0x6CC IsAttackedByLocomotor` is set) and `0x006FC5C7`
/// (open-topped `CheckTransportDisableWeapons` asks the transport's flag),
/// plus the gattling AI arms, and VERA keeps no `IsAttackedByLocomotor`
/// producer; and the charge-turret half of
/// `ROFMultiplier.ApplyOnCurrentTimer` needs the
/// `LastRearmWasFullDelay`/charge-timer state VERA does not keep.
#[cfg(feature = "phobos")]
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct PhobosStatMultipliers {
    pub firepower: f64,
    pub armor: f64,
    pub speed: f64,
    pub rof: f64,
    pub cloakable: bool,
    pub force_decloak: bool,
    pub disable_weapons: bool,
    pub unkillable: bool,
    pub has_restricted_armor: bool,
    pub has_range_modifier: bool,
    pub has_crit_modifiers: bool,
}

#[cfg(feature = "phobos")]
impl Default for PhobosStatMultipliers {
    fn default() -> Self {
        Self {
            firepower: 1.0,
            armor: 1.0,
            speed: 1.0,
            rof: 1.0,
            cloakable: false,
            force_decloak: false,
            disable_weapons: false,
            unkillable: false,
            has_restricted_armor: false,
            has_range_modifier: false,
            has_crit_modifiers: false,
        }
    }
}

#[cfg(feature = "phobos")]
impl AttachEffectList {
    /// The active effects with their catalog entries, in attach order.
    pub(crate) fn active_effects<'a>(
        &'a self,
        effects: &'a [AttachEffectType],
    ) -> impl Iterator<Item = (&'a AttachEffectInstance, &'a AttachEffectType)> {
        self.instances
            .iter()
            .filter(|instance| instance.is_active(instance.is_self_owned()))
            .filter_map(|instance| {
                effects
                    .get(instance.type_index)
                    .map(|effect| (instance, effect))
            })
    }

    /// `RecalculateStatMultipliers`'s full pass over the active effects: the
    /// four products, the four flags and the three derived "has" flags.
    pub(crate) fn stat_multipliers(&self, effects: &[AttachEffectType]) -> PhobosStatMultipliers {
        let mut stats = PhobosStatMultipliers::default();
        for (_, effect) in self.active_effects(effects) {
            stats.firepower *= effect.firepower_multiplier;
            stats.speed *= effect.speed_multiplier;
            stats.rof *= effect.rof_multiplier;
            if effect.restricted_armor_multiplier() {
                stats.has_restricted_armor = true;
            } else {
                stats.armor *= effect.armor_multiplier;
            }
            stats.cloakable |= effect.cloakable;
            stats.force_decloak |= effect.force_decloak;
            stats.disable_weapons |= effect.disable_weapons;
            stats.unkillable |= effect.unkillable;
            stats.has_range_modifier |=
                effect.weapon_range_multiplier != 1.0 || effect.weapon_range_extra_range != 0.0;
            stats.has_crit_modifiers |= effect.has_crit_modifiers();
        }
        stats
    }

    /// `TechnoExt::ExtData::GetAttachedEffectCumulativeCount` with no source
    /// filtering: how many active instances of `type_index` the techno carries.
    pub(crate) fn active_count(&self, type_index: usize) -> i32 {
        self.instances
            .iter()
            .filter(|instance| instance.type_index == type_index)
            .filter(|instance| instance.is_active(instance.is_self_owned()))
            .count() as i32
    }

    /// `AttachEffectTypeClass::HasGroups(groups, false)` over the catalog: the
    /// type indices of every type carrying at least one listed group, in
    /// catalog order. Phobos' `GetTypesFromGroups` returns a set of pointers;
    /// VERA keeps the catalog order for determinism.
    pub(crate) fn types_with_groups(
        effect_count: usize,
        groups: &[String],
        effects: &[AttachEffectType],
    ) -> Vec<usize> {
        (0..effect_count)
            .filter(|&index| {
                effects.get(index).is_some_and(|effect| {
                    effect.groups.iter().any(|group| {
                        groups
                            .iter()
                            .any(|required| group.eq_ignore_ascii_case(required))
                    })
                })
            })
            .collect()
    }

    /// `TechnoExt::ExtData::HasAttachedEffects` (`Body.cpp:484`): every entry
    /// of `types` needs an active instance when `require_all`, any one when it
    /// does not. A cumulative entry counts through
    /// [`Self::active_count`] and is bounded by the paired counts; a plain
    /// entry matches its first active instance. `ignore_same_source` drops
    /// instances attached by the same invoker and warhead, `same_source_only`
    /// keeps only those, and `house_allowed` filters the instance's invoker
    /// house.
    pub(crate) fn has_attached_effects(
        &self,
        types: &[usize],
        require_all: bool,
        ignore_same_source: bool,
        same_source_only: bool,
        invoker: Option<u64>,
        warhead: Option<&str>,
        effects: &[AttachEffectType],
        interner: &crate::sim::intern::StringInterner,
        min_counts: &[i32],
        max_counts: &[i32],
        house_allowed: impl Fn(bool, Option<crate::sim::intern::InternedId>) -> bool,
    ) -> bool {
        let check_source = ignore_same_source && invoker.is_some() && warhead.is_some();
        let check_same_source = same_source_only && invoker.is_some() && warhead.is_some();
        let min_size = min_counts.len();
        let max_size = max_counts.len();
        let mut found_count = 0usize;
        let mut type_counter = 1usize;

        for &type_index in types {
            let Some(effect) = effects.get(type_index) else {
                continue;
            };
            if effect.cumulative {
                let count: i32 = self
                    .instances
                    .iter()
                    .filter(|instance| instance.type_index == type_index)
                    .filter(|instance| instance.is_active(instance.is_self_owned()))
                    .filter(|instance| {
                        !(check_source
                            && instance
                                .source
                                .is_some_and(|source| source.matches(invoker, warhead, interner)))
                    })
                    .filter(|instance| {
                        !(check_same_source
                            && !instance
                                .source
                                .is_some_and(|source| source.matches(invoker, warhead, interner)))
                    })
                    .filter(|instance| {
                        house_allowed(
                            instance.is_self_owned(),
                            instance.source.and_then(|source| source.invoker_house),
                        )
                    })
                    .count() as i32;
                let mut matched = count > 0;
                if matched && min_size > 0 {
                    if count < paired(min_counts, type_counter - 1, -1) {
                        matched = false;
                    }
                }
                if matched && max_size > 0 {
                    if count > paired(max_counts, type_counter - 1, -1) {
                        matched = false;
                    }
                }
                if matched {
                    if !require_all {
                        return true;
                    }
                    found_count += 1;
                }
            } else {
                for instance in &self.instances {
                    if instance.type_index != type_index
                        || !instance.is_active(instance.is_self_owned())
                    {
                        continue;
                    }
                    if check_source
                        && instance
                            .source
                            .is_some_and(|source| source.matches(invoker, warhead, interner))
                    {
                        continue;
                    }
                    if check_same_source
                        && !instance
                            .source
                            .is_some_and(|source| source.matches(invoker, warhead, interner))
                    {
                        continue;
                    }
                    if !house_allowed(
                        instance.is_self_owned(),
                        instance.source.and_then(|source| source.invoker_house),
                    ) {
                        continue;
                    }
                    if !require_all {
                        return true;
                    }
                    found_count += 1;
                    break;
                }
            }
            if require_all && found_count < type_counter {
                return false;
            }
            type_counter += 1;
        }

        require_all && found_count == types.len()
    }

    /// `TechnoExt::ExtData::HasAttachedEffects` with `requireAll=false`: any
    /// list entry with an active instance matches. `require_anims` adds the
    /// effect's own guard: a type that declares an animation only matches
    /// while that instance holds one.
    pub fn has_active_type(
        &self,
        names: &[String],
        effects: &[AttachEffectType],
        require_anims: bool,
    ) -> bool {
        self.instances.iter().any(|instance| {
            let Some(effect) = effects.get(instance.type_index) else {
                return false;
            };
            instance.is_active(instance.is_self_owned())
                && (!require_anims || !effect.has_animation() || instance.anim.is_some())
                && names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(&effect.name))
        })
    }

    /// `UpdateConditionalAnimDrawingLogic`'s draw offset for the animation of
    /// `instance`: every rule whose `RequiredTypes` is empty or matches at
    /// least one attached effect adds its offset, so the rules accumulate
    /// rather than the last one winning.
    pub fn draw_offset(&self, instance: usize, effects: &[AttachEffectType]) -> [i32; 2] {
        let Some(instance) = self.instances.get(instance) else {
            return [0, 0];
        };
        let Some(effect) = effects.get(instance.type_index) else {
            return [0, 0];
        };
        let mut offset = [0, 0];
        for rule in &effect.animation_draw_offsets {
            if rule.required_types.is_empty()
                || self.has_active_type(&rule.required_types, effects, true)
            {
                offset[0] += rule.offset[0];
                offset[1] += rule.offset[1];
            }
        }
        offset
    }
}

/// The per-frame target facts `AttachEffectClass::ShouldBeDiscardedNow`
/// reads.
#[cfg(feature = "phobos")]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DiscardFacts {
    pub is_foot: bool,
    pub is_building: bool,
    pub is_moving: bool,
    pub is_really_moving: bool,
    pub harvesting: bool,
    pub mission_harvest_on_tiberium: bool,
    pub selling: bool,
    pub undeploying: bool,
    pub draining: bool,
    pub ammo: i32,
    /// The target's current and type-maximum health, read by
    /// [`crate::sim::combat::health_in_threshold`].
    pub health: i32,
    pub strength: i32,
    pub land_type: Option<crate::rules::terrain_rules::LandType>,
    pub mission: Option<crate::rules::mission_data::MissionType>,
    pub sequence: Option<crate::rules::animation_sequence::SequenceKind>,
    /// The held target's 3-D distance and the selected weapon's effective
    /// range in leptons (`-1` without a weapon).
    pub target: Option<(i32, i32)>,
    pub owner_human: bool,
}

#[cfg(feature = "phobos")]
impl AttachEffectInstance {
    /// `ShouldBeDiscardedNow`: the event flag or any continuous condition the
    /// type lists. RESIDUAL: `Drain` waits on a victim-side drain owner.
    pub(crate) fn should_be_discarded_now(
        &self,
        effect: &AttachEffectType,
        facts: &DiscardFacts,
        globals: &crate::rules::attach_effect::AttachEffectGlobals,
    ) -> bool {
        use crate::rules::attach_effect::DiscardOn;
        if self.should_be_discarded {
            return true;
        }
        let flags = effect.discard_on;
        if flags.is_none() {
            return false;
        }
        if facts.is_foot {
            let by_destination = effect
                .discard_move_based_on_destination
                .unwrap_or(globals.discard_move_based_on_destination);
            let moving = if by_destination {
                facts.is_moving
            } else {
                facts.is_really_moving
            };
            if moving {
                if flags.contains(DiscardOn::MOVE) {
                    return true;
                }
            } else if effect
                .discard_consider_harvesting_as_stationary
                .unwrap_or(globals.discard_consider_harvesting_as_stationary)
            {
                if flags.contains(DiscardOn::STATIONARY) {
                    return true;
                }
            } else if facts.harvesting {
                if flags.contains(DiscardOn::HARVESTING) {
                    return true;
                }
            } else if facts.mission_harvest_on_tiberium {
                // The intermediate state that is about to start harvesting is
                // neither harvesting nor stationary this frame.
                return false;
            } else if flags.contains(DiscardOn::STATIONARY) {
                return true;
            }
        }
        if facts.is_building && facts.selling {
            if facts.undeploying {
                if flags.contains(DiscardOn::UNDEPLOYING) {
                    return true;
                }
            } else if flags.contains(DiscardOn::SELLING) {
                return true;
            }
        }
        if facts.draining && flags.contains(DiscardOn::DRAIN) {
            return true;
        }
        if flags.contains(DiscardOn::AMMO) {
            let min = effect.discard_ammo_minimum;
            let max = effect.discard_ammo_maximum;
            if (min < 0 || facts.ammo >= min) && (max < 0 || facts.ammo <= max) {
                return true;
            }
        }
        if flags.contains(DiscardOn::HEALTH)
            && crate::sim::combat::health_in_threshold(
                facts.health,
                facts.strength,
                effect.discard_health_above,
                effect.discard_health_below,
            )
        {
            return true;
        }
        if flags.contains(DiscardOn::LAND_TYPE) && !effect.discard_land_types.is_empty() {
            if let Some(land) = facts.land_type {
                if effect
                    .discard_land_types
                    .iter()
                    .any(|token| token.eq_ignore_ascii_case(land.section_name()))
                {
                    return true;
                }
            }
        }
        if flags.contains(DiscardOn::MISSION) {
            // A human-controlled owner uses `DiscardOn.Missions`; an AI uses
            // `DiscardOn.AIMissions` when it is set.
            let missions = if facts.owner_human {
                &effect.discard_missions
            } else {
                effect
                    .discard_ai_missions
                    .as_ref()
                    .unwrap_or(&effect.discard_missions)
            };
            if let Some(mission) = facts.mission {
                if missions
                    .iter()
                    .any(|name| mission.ini_section().eq_ignore_ascii_case(name))
                {
                    return true;
                }
            }
        }
        if flags.contains(DiscardOn::SEQUENCE) && !effect.discard_sequences.is_empty() {
            if let Some(current) = facts.sequence {
                let matches = |kind| {
                    effect.discard_sequences.iter().any(|token| {
                        crate::rules::animation_sequence::SequenceKind::from_native_name(token)
                            == Some(kind)
                    })
                };
                let immediate = effect
                    .discard_sequences_immediate
                    .unwrap_or(globals.discard_sequences_immediate);
                if immediate {
                    if matches(current) {
                        return true;
                    }
                } else if let Some(last) = self.last_sequence_check
                    && last != current
                    && matches(last)
                {
                    return true;
                }
            }
        }
        if flags.contains(DiscardOn::IN_RANGE) || flags.contains(DiscardOn::OUT_OF_RANGE) {
            if let Some((distance, weapon_range)) = facts.target {
                let range = effect
                    .discard_range_override
                    .map_or(weapon_range, |cells| cpp_i32(cells * 256.0));
                if (flags.contains(DiscardOn::IN_RANGE) && distance <= range)
                    || (flags.contains(DiscardOn::OUT_OF_RANGE) && distance >= range)
                {
                    return true;
                }
            }
        }
        false
    }
}

/// Where an effect's animation lives: attached to `owner` at its world point,
/// drawn under `house`, and paused or running with the effect's state.
#[cfg(feature = "phobos")]
struct AttachEffectAnimTarget {
    owner: u64,
    house: crate::sim::intern::InternedId,
    world: crate::sim::anim_class::AnimWorldCoord,
    paused: bool,
}

#[cfg(feature = "phobos")]
impl Simulation {
    /// Phobos `TechnoExt::UpdateAttachEffects`
    /// (`src/Ext/Techno/Body.Update.cpp:895..975`, reached from the
    /// `TechnoClass_AI_0x6F9E50` early update): every effect runs one `AI()`
    /// phase, expired effects are removed, and each surviving effect
    /// reconciles its `AnimClass` with `CanShowAnim`.
    ///
    /// RESIDUALS: `DiscardOn` belongs to its own slice, so nothing is discarded
    /// here yet; native `IsImmobilized` is approximated by the chrono-warp and
    /// transport-interior gates VERA models; `CloakCheck` and the tunnel
    /// animation state are not ported; a cumulative type's animation is
    /// rebuilt when the layer count changes instead of going through
    /// `AnimExt::ChangeAnimType`, so `CumulativeAnimations.RestartOnChange=no`
    /// still restarts; `Animation.ResetOnReapply` does not restart a refreshed
    /// effect's animation.
    /// `ShouldBeDiscardedNow`'s target facts for one entity, read once per
    /// frame before the AI walk. `needs_range` skips the weapon selection
    /// when no effect on the object reads inrange/outofrange.
    fn phobos_discard_facts(
        &self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        needs_range: bool,
    ) -> DiscardFacts {
        let Some(entity) = self.substrate.entities.get(id) else {
            return DiscardFacts::default();
        };
        use crate::map::entities::EntityCategory;
        let category = entity.category();
        let is_foot = matches!(
            category,
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
        );
        let is_building = category == EntityCategory::Structure;
        let frame = self.session.binary_frame();
        let (is_moving, is_really_moving) = if is_foot {
            (
                crate::sim::movement::motion_query::is_moving(entity).unwrap_or(false),
                crate::sim::movement::motion_query::is_really_moving_now(
                    entity,
                    crate::sim::movement::SpeedRules::new(rules, &self.interner, &self.houses),
                    frame,
                ),
            )
        } else {
            (false, false)
        };
        // `InfantryClass::SequenceAnim` follows the current Doing; VERA maps
        // the stored action id through the shared sequence table.
        let sequence = (category == EntityCategory::Infantry)
            .then(|| {
                entity
                    .mission_leaf
                    .as_infantry()
                    .and_then(|leaf| crate::rules::infantry_sequence::action_kind(leaf.doing()))
            })
            .flatten();
        let target = if needs_range {
            entity.attack_target.target().and_then(|target| {
                let target_coords = match target {
                    crate::sim::combat::TargetKind::Entity(id) => {
                        let target = self.substrate.entities.get(id)?;
                        let coord = crate::sim::movement::ground_pose::position_world_coord(
                            &target.position,
                        );
                        [coord.x, coord.y, coord.z]
                    }
                    crate::sim::combat::TargetKind::Cell(rx, ry) => {
                        let floor_z = self
                            .resolved_terrain
                            .as_ref()
                            .and_then(|terrain| terrain.cell(rx, ry))
                            .map_or(0, |cell| {
                                i32::from(cell.level)
                                    .wrapping_mul(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS)
                            });
                        let centre = crate::sim::cell_kernel::cell_center(
                            crate::sim::cell_kernel::CellCoordinate {
                                x: i32::from(rx),
                                y: i32::from(ry),
                            },
                            floor_z,
                        );
                        [centre.x, centre.y, centre.z]
                    }
                };
                let source =
                    crate::sim::movement::ground_pose::position_world_coord(&entity.position);
                let distance = crate::util::native_x87::distance_3d_leptons(
                    [source.x, source.y, source.z],
                    target_coords,
                );
                let object = self.interner.object_type(entity.type_ref(), rules)?;
                let (_, selected) = crate::sim::combat::combat_weapon::select_weapon_against(
                    rules,
                    entity,
                    object,
                    &crate::sim::combat::combat_weapon::attacker_facts(entity, object),
                    Some(&target),
                    self,
                    &self.interner,
                    self.resolved_terrain.as_ref(),
                    Some(self.house_alliances.as_map()),
                )?;
                let range = selected.map_or(-1, |selected| {
                    let source = range_modifier_source(
                        entity,
                        Some(&self.substrate.entities),
                        rules,
                        &self.interner,
                    );
                    range_with_attach_effects(
                        source,
                        &selected.weapon.id,
                        selected.weapon.range_leptons,
                        rules,
                    )
                });
                Some((distance, range))
            })
        } else {
            None
        };
        let land_type = self.resolved_terrain.as_ref().and_then(|terrain| {
            terrain
                .cell(entity.position.rx(), entity.position.ry())
                .and_then(|cell| crate::rules::terrain_rules::LandType::from_index(cell.land_type))
        });
        let mission = entity.mission.current().known();
        let harvesting = is_foot
            && (entity
                .miner
                .as_ref()
                .is_some_and(|miner| miner.is_harvesting())
                || entity.animation.as_ref().is_some_and(|animation| {
                    animation.sequence() == crate::rules::animation_sequence::SequenceKind::Shovel
                }));
        let mission_harvest_on_tiberium = mission
            == Some(crate::rules::mission_data::MissionType::Harvest)
            && land_type == Some(crate::rules::terrain_rules::LandType::Tiberium);
        let selling =
            is_building && mission == Some(crate::rules::mission_data::MissionType::Selling);
        let strength = self
            .interner
            .object_type(entity.type_ref(), rules)
            .map_or(0, |object| object.strength);
        let health = entity.health.current();
        DiscardFacts {
            is_foot,
            is_building,
            is_moving,
            is_really_moving,
            harvesting,
            mission_harvest_on_tiberium,
            selling,
            undeploying: selling && entity.archive_target().is_some(),
            // `TechnoClass+0x1D0 DrainingMe`, the Floating Disc victim side
            // owned by [`crate::sim::credit_income::Drain`].
            draining: entity.drain.draining_me().is_some(),
            ammo: entity.ammo().map_or(0, |ammo| ammo.current()),
            health,
            strength,
            land_type,
            mission,
            sequence,
            target,
            // `HouseClass::IsControlledByHuman @ 0x0050B730`, the predicate
            // the mission list reads.
            owner_human: self.houses.get(&entity.owner()).is_some_and(|house| {
                house.is_controlled_by_human(self.session.game_mode_nonzero())
            }),
        }
    }

    /// Phobos `TechnoClass_FireAt_BeforeTruelyFire` (`0x6FDDC0`, 6 bytes
    /// replaced): every admitted shot counts towards `DiscardOn.Firing.Count`,
    /// including spawner, drain and suicide weapons that launch no bullet.
    pub(crate) fn record_phobos_attach_firing(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.phobos_attach_effects().is_empty() {
            return;
        }
        let effects = rules.attach_effect_types().types();
        let mut list = std::mem::take(
            self.substrate
                .entities
                .get_mut(id)
                .expect("the gated firer still exists")
                .phobos_attach_effects_mut(),
        );
        for instance in &mut list.instances {
            let Some(effect) = effects.get(instance.type_index) else {
                continue;
            };
            if !effect.discard_on.contains(DiscardOn::FIRING) {
                continue;
            }
            instance.firing_count += 1;
            if instance.firing_count >= effect.discard_firing_count {
                instance.should_be_discarded = true;
            }
        }
        *self
            .substrate
            .entities
            .get_mut(id)
            .expect("the gated firer still exists")
            .phobos_attach_effects_mut() = list;
    }

    /// Phobos' ReceiveDamage wrapper: a hit whose source house passes
    /// `DiscardOn.ReceivedDamage.AffectsHouse` counts towards the type's
    /// `DiscardOn.ReceivedDamage.Count`.
    pub(crate) fn record_phobos_attach_damage(
        &mut self,
        id: u64,
        source_house: Option<crate::sim::intern::InternedId>,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.phobos_attach_effects().is_empty() {
            return;
        }
        let owner = entity.owner();
        let effects = rules.attach_effect_types().types();
        let mut list = std::mem::take(
            self.substrate
                .entities
                .get_mut(id)
                .expect("the gated target still exists")
                .phobos_attach_effects_mut(),
        );
        for instance in &mut list.instances {
            let Some(effect) = effects.get(instance.type_index) else {
                continue;
            };
            if !effect.discard_on.contains(DiscardOn::RECEIVED_DAMAGE) {
                continue;
            }
            // `CanTargetHouse(flags, pThis->Owner, pSourceHouse)`: the target's
            // owner is the "owner" argument and the source house the target.
            let gated = match source_house {
                Some(source) => affects_house(
                    effect.discard_received_damage_affects_house,
                    Some(owner),
                    source,
                    &self.houses,
                    &self.house_alliances,
                    &self.interner,
                ),
                None => effect
                    .discard_received_damage_affects_house
                    .contains(AffectedHouse::ENEMIES),
            };
            if !gated {
                continue;
            }
            instance.received_damage_count += 1;
            if instance.received_damage_count >= effect.discard_received_damage_count {
                instance.should_be_discarded = true;
            }
        }
        *self
            .substrate
            .entities
            .get_mut(id)
            .expect("the gated target still exists")
            .phobos_attach_effects_mut() = list;
    }

    /// Phobos' House owner-change hook: every effect with
    /// `DiscardOn=ownerchange` is flagged when the human/computer direction
    /// and the two per-type switches admit it, except a revert driven by a
    /// transport exit. RESIDUAL: VERA has no `Passengers.SyncOwner`
    /// revert-on-exit caller, so `IgnoreRevertOnExit` is always false here.
    pub(crate) fn phobos_attach_effects_on_owner_change(
        &mut self,
        id: u64,
        old_owner: crate::sim::intern::InternedId,
        new_owner: crate::sim::intern::InternedId,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.phobos_attach_effects().is_empty() {
            return;
        }
        let game_mode_nonzero = self.session.game_mode_nonzero();
        let old_human = self
            .houses
            .get(&old_owner)
            .is_some_and(|house| house.is_controlled_by_human(game_mode_nonzero));
        let new_human = self
            .houses
            .get(&new_owner)
            .is_some_and(|house| house.is_controlled_by_human(game_mode_nonzero));
        let human_and_computer = old_human != new_human;
        let has_transporter = entity.passenger_role.inside_transport_id().is_some();
        let effects = rules.attach_effect_types().types();
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
            if !effect.discard_on.contains(DiscardOn::OWNER_CHANGE) {
                continue;
            }
            let human_to_computer = effect.discard_owner_change_human_to_computer;
            let computer_to_human = effect.discard_owner_change_computer_to_human;
            let ignore_revert = false;
            let mut discard = false;
            if effect.discard_owner_change_ignore_revert_on_exit && ignore_revert {
                discard = false;
            } else if human_to_computer && computer_to_human {
                discard = true;
            } else if human_to_computer || computer_to_human {
                if human_and_computer
                    && ((old_human && human_to_computer) || (!old_human && computer_to_human))
                {
                    discard = true;
                }
            }
            if discard && has_transporter && !ignore_revert {
                discard = false;
            }
            if discard {
                instance.should_be_discarded = true;
            }
        }
        *self
            .substrate
            .entities
            .get_mut(id)
            .expect("the gated entity still exists")
            .phobos_attach_effects_mut() = list;
    }

    /// Phobos `TechnoClass_Limbo_Wrapper` (`Ext/Techno/Hooks.cpp:400..472`,
    /// call/vtable replacements): every effect with `DiscardOn=Entry` is reset
    /// or removed when the object enters limbo, except on an undeploying
    /// building (`UndeploysInto=` while its mission is Selling at status 2).
    /// A removed effect queues its `Discard` expire weapon; the stat
    /// multipliers are recalculated, then the weapons detonate at the
    /// object's coordinates before the original Limbo body. A reset effect
    /// keeps its `ShouldBeDiscarded` flag, as native does.
    pub(crate) fn limbo_phobos_attach_effects(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: crate::sim::world::FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.phobos_attach_effects().is_empty() {
            return;
        }
        if entity.category() == crate::map::entities::EntityCategory::Structure {
            let undeploying = self
                .interner
                .object_type(entity.type_ref(), rules)
                .is_some_and(|object| object.undeploys_into.is_some())
                && entity.mission.current().known()
                    == Some(crate::rules::mission_data::MissionType::Selling)
                && entity.mission.handler_state() == 2;
            if undeploying {
                return;
            }
        }
        let owner_house = entity.owner();
        let effects = rules.attach_effect_types().types();
        let mut list = std::mem::take(
            self.substrate
                .entities
                .get_mut(id)
                .expect("the gated entity still exists")
                .phobos_attach_effects_mut(),
        );
        let mut expire_shots = Vec::new();
        let mut index = 0;
        while index < list.instances.len() {
            let type_index = list.instances[index].type_index;
            let Some(effect) = effects
                .get(type_index)
                .filter(|effect| effect.discard_on.contains(DiscardOn::ENTRY))
            else {
                index += 1;
                continue;
            };
            if let Some(anim) = list.instances[index].anim.take() {
                self.destroy_anim_with_context(anim, rules);
            }
            if list.instances[index].reset_if_recreatable() {
                index += 1;
                continue;
            }
            let active_count = list.active_count(type_index);
            if let Some(shot) = list.instances[index].expire_weapon_shot(
                effect,
                ExpireWeaponCondition::DISCARD,
                id,
                owner_house,
                active_count,
                false,
            ) {
                expire_shots.push(shot);
            }
            list.instances.remove(index);
        }
        let entity = self
            .substrate
            .entities
            .get_mut(id)
            .expect("the gated entity still exists");
        *entity.phobos_attach_effects_mut() = list;
        entity.recalculate_phobos_stat_multipliers(effects);
        if !expire_shots.is_empty() {
            let detonations: Vec<_> = expire_shots
                .iter()
                .filter_map(|shot| self.attach_effect_weapon_detonation(shot, id, rules))
                .collect();
            self.commit_attach_effect_detonations(
                &detonations,
                rules,
                overlay_registry,
                frame_effects,
            );
        }
    }

    /// [`Self::update_phobos_attach_effects_with`] without a caller frame
    /// boundary: fixtures that observe the list directly.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn update_phobos_attach_effects(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        self.update_phobos_attach_effects_with(
            id,
            rules,
            crate::sim::world::FrameEffects::empty(),
            None,
        );
    }

    pub(crate) fn update_phobos_attach_effects_with(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        frame_effects: crate::sim::world::FrameEffects<'_>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        // `InLimbo || IsImmobilized`: an object that cannot act keeps its
        // effects untouched. Chrono warp and transport interior stand in for
        // the native immobilised states VERA models.
        if entity.lifecycle.in_limbo()
            || entity.chrono_warp_latch()
            || entity.passenger_role.inside_transport_id().is_some()
            || entity.phobos_attach_effects().is_empty()
        {
            return;
        }
        // `OnlineCheck`: Deactivated, EMP, and a building's power-online latch.
        let owner_active = !entity.deactivated.is_offline()
            && entity.emp_remaining() <= 0
            && (entity.category() != crate::map::entities::EntityCategory::Structure
                || entity.building_online());
        let owner_house = entity.owner();
        let position = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        let world = crate::sim::anim_class::AnimWorldCoord {
            x: position.x,
            y: position.y,
            z: position.z,
        };
        let mut list = std::mem::take(
            self.substrate
                .entities
                .get_mut(id)
                .expect("the gated entity still exists")
                .phobos_attach_effects_mut(),
        );
        let effects = rules.attach_effect_types().types();
        let globals = *rules.attach_effect_globals();
        let needs_range = list.instances().iter().any(|instance| {
            effects.get(instance.type_index).is_some_and(|effect| {
                effect.discard_on.contains(DiscardOn::IN_RANGE)
                    || effect.discard_on.contains(DiscardOn::OUT_OF_RANGE)
            })
        });
        let facts = self.phobos_discard_facts(id, rules, needs_range);
        list.online_check_all(effects, owner_active);
        // `PointerGotInvalid` flags an `InvokerDie` effect whose invoker is
        // gone. VERA checks the store each frame instead of the native
        // pointer-expiry broadcast, so a target that ran earlier in the
        // invoker's death frame sees it one frame later.
        for instance in &mut list.instances {
            let Some(effect) = effects.get(instance.type_index) else {
                continue;
            };
            if effect.discard_on.contains(DiscardOn::INVOKER_DIE)
                && let Some(invoker) = instance.source.and_then(|source| source.invoker)
                && self.substrate.entities.get(invoker).is_none()
            {
                instance.should_be_discarded = true;
            }
        }
        let initialized_before: Vec<bool> = list
            .instances
            .iter()
            .map(|instance| instance.has_initialized())
            .collect();
        // `AI()` reads `ShouldBeDiscardedNow()` at its delay and expiry
        // branches after initialization, so the tick evaluates it lazily
        // there instead of taking a value cached before the frame.
        list.tick_all(effects, |_, instance, effect| {
            instance.should_be_discarded_now(effect, &facts, &globals)
        });
        // `IsActiveIgnorePowered() && ShouldBeDiscardedNow()`
        // (`Body.Update.cpp:940`) runs after `AI()`; the per-frame native
        // LastDiscardCheckFrame cache makes this value equal to the tick's.
        let active: Vec<bool> = list
            .instances
            .iter()
            .map(|instance| instance.is_active_ignoring_power(instance.is_self_owned()))
            .collect();
        let discarded: Vec<bool> = list
            .instances
            .iter()
            .zip(&active)
            .map(|(instance, &active)| {
                active
                    && effects.get(instance.type_index).is_some_and(|effect| {
                        instance.should_be_discarded_now(effect, &facts, &globals)
                    })
            })
            .collect();
        // `LastSequenceCheck` advances only when the frame reached the
        // sequence block: an earlier condition returned first.
        for (index, instance) in list.instances.iter_mut().enumerate() {
            if !active.get(index).copied().unwrap_or(false)
                || discarded.get(index).copied().unwrap_or(false)
            {
                continue;
            }
            let Some(effect) = effects.get(instance.type_index) else {
                continue;
            };
            if !effect.discard_on.contains(DiscardOn::SEQUENCE) {
                continue;
            }
            match facts.sequence {
                Some(current) => {
                    if !effect.discard_sequences.is_empty()
                        && !effect
                            .discard_sequences_immediate
                            .unwrap_or(globals.discard_sequences_immediate)
                    {
                        instance.last_sequence_check = Some(current);
                    }
                }
                None => instance.last_sequence_check = None,
            }
        }
        // `AI()`'s initialization block: `ROFMultiplier.ApplyOnCurrentTimer`
        // scales the timer already running when the effect first activates.
        let frame = self.session.binary_frame() as i32;
        for (index, instance) in list.instances.iter().enumerate() {
            if initialized_before.get(index).copied().unwrap_or(true) || !instance.has_initialized()
            {
                continue;
            }
            let Some(effect) = effects.get(instance.type_index) else {
                continue;
            };
            if effect.rof_multiplier <= 0.0 || !effect.rof_multiplier_apply_on_current_timer {
                continue;
            }
            let Some(entity) = self.substrate.entities.get_mut(id) else {
                continue;
            };
            let remaining = entity.rearm_timer.remaining(frame);
            if remaining > 0 {
                entity
                    .rearm_timer
                    .start(frame, scale_i32(remaining, effect.rof_multiplier));
            }
        }
        // `HasExpired || (IsActiveIgnorePowered && ShouldBeDiscardedNow)`:
        // a discardable effect behind a recreation delay is reset instead of
        // erased; the destructor unlinks the animation either way.
        // Reverse order keeps the flag vector aligned with the shifting list.
        let mut expire_shots: Vec<AttachEffectWeaponShot> = Vec::new();
        for index in (0..list.instances.len()).rev() {
            let expired = list.instances[index].has_expired();
            let discard = discarded.get(index).copied().unwrap_or(false);
            if !(expired || discard) {
                continue;
            }
            // `AddExpireWeaponParams` runs before the reset/erase, and its
            // cumulative check reads the count at this moment.
            let type_index = list.instances[index].type_index;
            if let Some(effect) = effects.get(type_index) {
                let condition = if expired {
                    ExpireWeaponCondition::EXPIRE
                } else {
                    ExpireWeaponCondition::DISCARD
                };
                let active_count = list.active_count(type_index);
                if let Some(shot) = list.instances[index].expire_weapon_shot(
                    effect,
                    condition,
                    id,
                    owner_house,
                    active_count,
                    false,
                ) {
                    expire_shots.push(shot);
                }
            }
            if discard {
                list.instances[index].clear_discarded();
                if list.instances[index].reset_if_recreatable() {
                    if let Some(anim) = list.instances[index].anim.take() {
                        self.destroy_anim_with_context(anim, rules);
                    }
                    continue;
                }
            }
            if let Some(anim) = list.instances[index].anim.take() {
                self.destroy_anim_with_context(anim, rules);
            }
            list.instances.remove(index);
        }
        // The removal walks backwards; the shots keep native's attach order.
        expire_shots.reverse();
        // `UpdateConditionalAnimDrawingLogic` reacts to other effects on the
        // same object, so every effect re-evaluates it each frame.
        let mut state = Vec::with_capacity(list.instances.len());
        let mut can_show = Vec::with_capacity(list.instances.len());
        for instance in &list.instances {
            let Some(effect) = effects.get(instance.type_index) else {
                state.push(AttachAnimState::default());
                can_show.push(false);
                continue;
            };
            // Temporal state is not tracked yet, so only the power gate applies.
            let anim_state = instance.animation_state(effect, false);
            let hidden =
                list.has_active_type(&effect.animation_hide_if_attached_with, effects, false);
            can_show.push(instance.animation_phase() && anim_state.show && !hidden);
            state.push(anim_state);
        }
        // One cumulative type shows one animation no matter how many layers it
        // has: `UpdateCumulativeAttachEffects` keeps the instance with the
        // longest remaining duration (any negative duration wins, last seen).
        let mut designated = vec![None; effects.len()];
        for (type_index, effect) in effects.iter().enumerate() {
            if !(effect.cumulative && !effect.cumulative_animations.is_empty()) {
                continue;
            }
            let mut best = None;
            let mut duration = 0;
            for (instance_index, instance) in list.instances.iter().enumerate() {
                if instance.type_index != type_index || !can_show[instance_index] {
                    continue;
                }
                let current = instance.remaining();
                if current < 0 || current > duration {
                    best = Some(instance_index);
                    duration = current;
                }
            }
            designated[type_index] = best;
        }
        for instance_index in 0..list.instances.len() {
            let type_index = list.instances[instance_index].type_index;
            let Some(effect) = effects.get(type_index) else {
                continue;
            };
            let desired = if can_show[instance_index] {
                if effect.cumulative && !effect.cumulative_animations.is_empty() {
                    let count = list
                        .instances
                        .iter()
                        .filter(|other| other.type_index == type_index)
                        .filter(|other| other.is_active(other.is_self_owned()))
                        .count() as i32;
                    if designated[type_index] == Some(instance_index) {
                        effect.cumulative_animation(count)
                    } else {
                        None
                    }
                } else {
                    effect.animation.as_deref()
                }
            } else {
                None
            };
            let house = if effect.animation_use_invoker_as_owner {
                list.instances[instance_index]
                    .source
                    .and_then(|source| source.invoker)
                    .and_then(|invoker| self.substrate.entities.get(invoker).map(|e| e.owner()))
                    .unwrap_or(owner_house)
            } else {
                owner_house
            };
            self.maintain_phobos_attach_effect_anim(
                &mut list.instances[instance_index],
                desired,
                AttachEffectAnimTarget {
                    owner: id,
                    house,
                    world,
                    paused: state[instance_index].paused,
                },
                rules,
            );
        }
        let entity = self
            .substrate
            .entities
            .get_mut(id)
            .expect("the gated entity still exists");
        *entity.phobos_attach_effects_mut() = list;
        entity.recalculate_phobos_stat_multipliers(effects);
        // `UpdateAttachEffects` fires one weapon per expired/discarded effect
        // at the object's own coordinates, after stats and animations.
        if !expire_shots.is_empty() {
            let detonations: Vec<_> = expire_shots
                .iter()
                .filter_map(|shot| self.attach_effect_weapon_detonation(shot, id, rules))
                .collect();
            self.commit_attach_effect_detonations(
                &detonations,
                rules,
                overlay_registry,
                frame_effects,
            );
        }
    }

    /// `CreateAnim` / `KillAnim`: keep exactly `desired` alive on the effect.
    /// Phobos creates the animation at the owner's location, attaches it with
    /// `SetOwnerObject`, and holds its loops; a changed cumulative type is
    /// rebuilt here because `AnimExt::ChangeAnimType` has no VERA owner yet.
    fn maintain_phobos_attach_effect_anim(
        &mut self,
        instance: &mut AttachEffectInstance,
        desired: Option<&str>,
        target: AttachEffectAnimTarget,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let AttachEffectAnimTarget {
            owner,
            house,
            world,
            paused,
        } = target;
        if let Some(anim) = instance.anim {
            let keeps = desired.is_some_and(|name| {
                self.substrate.anims.get(anim).is_some_and(|live| {
                    self.interner
                        .resolve(live.type_id())
                        .eq_ignore_ascii_case(name)
                })
            });
            if !keeps {
                self.destroy_anim_with_context(anim, rules);
                instance.anim = None;
            }
        }
        if instance.anim.is_none()
            && let Some(name) = desired
        {
            let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
            let type_name = self.interner.intern(name);
            let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
                draw_flags: 0x600,
                owner: Some(owner),
                ..crate::sim::components::AnimClassSpawnDescriptor::new(
                    type_name, rx, ry, sub_x, sub_y, z,
                )
            };
            match self.spawn_anim_at_world(rules, descriptor, world) {
                Ok(anim) => {
                    self.substrate
                        .anims
                        .get_mut(anim)
                        .expect("the new effect animation")
                        .hold_loops();
                    self.substrate.set_anim_owner_house(anim, house);
                    instance.anim = Some(anim);
                }
                Err(error) => log::debug!(
                    "attach effect animation [{name}] on {owner} did not construct: {error}"
                ),
            }
        }
        if let Some(anim) = instance.anim
            && let Some(live) = self.substrate.anims.get_mut(anim)
        {
            if paused {
                live.pause();
            } else {
                live.resume();
            }
        }
    }

    /// Phobos `TechnoExt::CalculateArmorMultipliers` at the receiver hook
    /// (`0x00701966`, `hitAnim = true`): the cached unrestricted product, then
    /// one `RandomDouble` per active restricted effect with the chance, house
    /// and warhead gates; every applied effect multiplies and, when it lists a
    /// `HitAnim`, picks one and spawns it on the target.
    ///
    /// Native draws each chance before its gates and the pick before the anim
    /// constructor, so a HitAnim's own constructor draws stay between the
    /// following effects' chance draws; this loop keeps that order.
    ///
    /// RESIDUAL: the shield pre-stage still divides with the unrestricted
    /// product only, and this roll runs before the receiver's LaserFence,
    /// bridge-hut, zero-health and null-warhead early returns where native
    /// would not reach the armor step.
    pub(crate) fn attach_effect_armor_multiplier(
        &mut self,
        target_id: u64,
        warhead: Option<crate::sim::intern::InternedId>,
        source_house: Option<crate::sim::intern::InternedId>,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> f64 {
        let warhead_name = warhead.map(|id| self.interner.resolve(id).to_owned());
        let warhead_name = warhead_name.as_deref();
        let Some(target) = self.substrate.entities.get(target_id) else {
            return 1.0;
        };
        let stats = *target.phobos_stat_multipliers();
        if !stats.has_restricted_armor {
            return stats.armor;
        }
        let target_owner = target.owner();
        let position = crate::sim::movement::ground_pose::position_world_coord(&target.position);
        let world = crate::sim::anim_class::AnimWorldCoord {
            x: position.x,
            y: position.y,
            z: position.z,
        };
        let effects = rules.attach_effect_types().types();
        // Instance positions keep the attach order without holding the entity
        // borrow, so the per-instance `ArmorMultiplierTimer` can be read and
        // restarted between the other state and RNG writes.
        let restricted: Vec<(usize, usize)> = target
            .phobos_attach_effects()
            .instances()
            .iter()
            .enumerate()
            .filter(|(_, instance)| instance.is_active(instance.is_self_owned()))
            .filter(|(_, instance)| {
                effects
                    .get(instance.type_index)
                    .is_some_and(|effect| effect.restricted_armor_multiplier())
            })
            .map(|(index, instance)| (index, instance.type_index))
            .collect();
        if restricted.is_empty() {
            return stats.armor;
        }
        let frame = self.session.binary_frame() as i32;
        let mut multiplier = stats.armor;
        for (instance_index, type_index) in restricted {
            let Some(effect) = effects.get(type_index) else {
                continue;
            };
            // `attachEffect->ArmorMultiplierTimer.InProgress()` skips the
            // effect before its chance draw (`TechnoHooks.cpp:552`).
            let timer = self
                .substrate
                .entities
                .get(target_id)
                .and_then(|target| {
                    target
                        .phobos_attach_effects()
                        .instances()
                        .get(instance_index)
                })
                .map_or_else(crate::sim::timer::CdTimer::default, |instance| {
                    instance.armor_multiplier_timer
                });
            if timer.remaining(frame) > 0 {
                continue;
            }
            // `type->ArmorMultiplier_Chance < random.RandomDouble()`: the draw
            // happens for every restricted effect, whatever the chance value.
            let chance = self.scenario_rng.random_double();
            if effect.armor_multiplier_chance < chance {
                continue;
            }
            if !affects_house(
                effect.armor_multiplier_affects_house,
                source_house,
                target_owner,
                &self.houses,
                &self.house_alliances,
                &self.interner,
            ) {
                continue;
            }
            if let Some(warhead) = warhead_name {
                if effect
                    .armor_multiplier_disallow_warheads
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(warhead))
                {
                    continue;
                }
                if !effect.armor_multiplier_allow_warheads.is_empty()
                    && !effect
                        .armor_multiplier_allow_warheads
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case(warhead))
                {
                    continue;
                }
            }
            multiplier *= effect.armor_multiplier;
            // The real-hit arm starts the per-instance delay before the hit
            // animation (`TechnoHooks.cpp:573..579`).
            if effect.armor_multiplier_delay > 0
                && let Some(instance) =
                    self.substrate
                        .entities
                        .get_mut(target_id)
                        .and_then(|target| {
                            target
                                .phobos_attach_effects_mut()
                                .instances
                                .get_mut(instance_index)
                        })
            {
                instance
                    .armor_multiplier_timer
                    .start(frame, effect.armor_multiplier_delay);
            }
            if !effect.armor_multiplier_hit_anim.is_empty() {
                let pick = self
                    .scenario_rng
                    .next_range_i32_inclusive(0, effect.armor_multiplier_hit_anim.len() as i32 - 1);
                let name = effect.armor_multiplier_hit_anim[pick as usize].clone();
                self.spawn_attach_effect_hit_anim(target_id, &name, world, target_owner, rules);
            }
        }
        multiplier
    }

    /// Phobos' `Unkillable` arm of the ReceiveDamage wrapper:
    /// `MapClass::GetTotalDamage(input, WH, target armor, distance)` reaching
    /// the target's health leaves `Health` and `EstimatedHealth` at 1 and the
    /// receiver packet at 0. `WarheadTypeExt::CanKill` is not ported, so only
    /// the effect's flag opens the arm. Returns whether it applied.
    pub(crate) fn apply_unkillable_clamp(
        &mut self,
        target_id: u64,
        event: &crate::sim::combat::EntityDamageEvent,
        input: i32,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> bool {
        let Some(target) = self.substrate.entities.get(target_id) else {
            return false;
        };
        if !target.phobos_unkillable() || target.health.current() <= 0 || input == 0 {
            return false;
        }
        let Some(warhead) = rules.warhead(self.interner.resolve(event.warhead_ref)) else {
            return false;
        };
        let Some(object) = self.interner.object_type(target.type_ref(), rules) else {
            return false;
        };
        let source_house = event.source_house.or_else(|| {
            self.substrate
                .entities
                .get(event.attacker_id)
                .map(|attacker| attacker.owner())
        });
        if !crate::sim::shield::can_target_house(warhead, source_house, target.owner(), self) {
            return false;
        }
        let total = crate::sim::combat::damage::kernel::apply_warhead_damage(
            input,
            warhead.cell_spread_f64,
            warhead.percent_at_max_f64,
            warhead.interaction(object.armor_id()).multiplier(),
            event.distance_leptons,
            self.session.no_damage(),
            rules.combat_damage.max_damage,
        );
        if total < target.health.current() {
            return false;
        }
        let target = self
            .substrate
            .entities
            .get_mut(target_id)
            .expect("the gated target still exists");
        target.health.set(1);
        target.estimated_health.reset(1);
        true
    }

    /// `AnimExt::CreateRandomAnim`'s creation arm: one anim at the target's
    /// coordinates, owned by the target and attached to it.
    fn spawn_attach_effect_hit_anim(
        &mut self,
        target: u64,
        name: &str,
        world: crate::sim::anim_class::AnimWorldCoord,
        house: crate::sim::intern::InternedId,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
        let type_name = self.interner.intern(name);
        let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
            draw_flags: 0x600,
            owner: Some(target),
            ..crate::sim::components::AnimClassSpawnDescriptor::new(
                type_name, rx, ry, sub_x, sub_y, z,
            )
        };
        match self.spawn_anim_at_world(rules, descriptor, world) {
            Ok(anim) => {
                self.substrate.set_anim_owner_house(anim, house);
            }
            Err(error) => log::debug!(
                "attach effect hit animation [{name}] on {target} did not construct: {error}"
            ),
        }
    }
}

#[cfg(feature = "phobos")]
impl Simulation {
    /// `WeaponTypeExt::HasRequiredAttachedEffects` (`src/Ext/WeaponType/Body.cpp:11`):
    /// a weapon carrying no AE list always allows the shot. Otherwise the
    /// effects of the target (or, with `AttachEffect.CheckOnFirer`, of the
    /// firer) must satisfy the disallowed and required lists in native order,
    /// skipping effects attached by the same invoker and warhead when
    /// `AttachEffect.IgnoreFromSameSource` is set.
    pub(crate) fn weapon_attach_effects_allow(
        &self,
        weapon: &crate::rules::weapon_type::WeaponType,
        target: Option<&crate::sim::game_entity::GameEntity>,
        firer: Option<&crate::sim::game_entity::GameEntity>,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> bool {
        if !weapon.has_attach_effect_gates() {
            return true;
        }
        let techno = if weapon.attach_effect_check_on_firer {
            firer
        } else {
            target
        };
        let Some(techno) = techno else {
            return true;
        };
        let catalog = rules.attach_effect_types();
        let effects = catalog.types();
        let warhead = weapon.warhead.as_deref();
        let invoker = firer.map(|entity| entity.stable_id());
        let ignore = weapon.attach_effect_ignore_from_same_source;
        let list = techno.phobos_attach_effects();

        let resolve = |names: &[String]| -> Vec<usize> {
            names
                .iter()
                .filter_map(|name| catalog.index_of(name))
                .collect()
        };
        let refused = |types: &[usize],
                       require_all: bool,
                       min: &[i32],
                       max: &[i32],
                       houses: AffectedHouse| {
            // Native checks the matched instance's invoker house only when a
            // firer is known and the list narrows houses.
            let firer_owner = firer.map(|entity| entity.owner());
            let techno_owner = techno.owner();
            let need_house = firer.is_some() && houses != AffectedHouse::ALL;
            list.has_attached_effects(
                types,
                require_all,
                ignore,
                weapon.attach_effect_same_source_only,
                invoker,
                warhead,
                effects,
                &self.interner,
                min,
                max,
                |self_owned, instance_house| {
                    if !need_house {
                        return true;
                    }
                    // A self-owned effect's invoker is the techno itself.
                    let instance_house = if self_owned {
                        Some(techno_owner)
                    } else {
                        instance_house
                    };
                    match (firer_owner, instance_house) {
                        (Some(source), Some(target)) => crate::sim::combat::can_target_house(
                            houses,
                            source,
                            target,
                            &self.houses,
                            self.house_alliances.as_map(),
                            &self.interner,
                        ),
                        _ => false,
                    }
                },
            )
        };
        let disallowed = resolve(&weapon.attach_effect_disallowed_types);
        if !disallowed.is_empty()
            && refused(
                &disallowed,
                !weapon.attach_effect_disallowed_any,
                &weapon.attach_effect_disallowed_min_counts,
                &weapon.attach_effect_disallowed_max_counts,
                weapon.attach_effect_disallowed_houses,
            )
        {
            return false;
        }
        let disallowed_groups = AttachEffectList::types_with_groups(
            effects.len(),
            &weapon.attach_effect_disallowed_groups,
            effects,
        );
        if !disallowed_groups.is_empty()
            && refused(
                &disallowed_groups,
                !weapon.attach_effect_disallowed_any,
                &weapon.attach_effect_disallowed_min_counts,
                &weapon.attach_effect_disallowed_max_counts,
                weapon.attach_effect_disallowed_houses,
            )
        {
            return false;
        }
        let required = resolve(&weapon.attach_effect_required_types);
        if !required.is_empty()
            && !refused(
                &required,
                !weapon.attach_effect_required_any,
                &weapon.attach_effect_required_min_counts,
                &weapon.attach_effect_required_max_counts,
                weapon.attach_effect_required_houses,
            )
        {
            return false;
        }
        let required_groups = AttachEffectList::types_with_groups(
            effects.len(),
            &weapon.attach_effect_required_groups,
            effects,
        );
        if !required_groups.is_empty()
            && !refused(
                &required_groups,
                !weapon.attach_effect_required_any,
                &weapon.attach_effect_required_min_counts,
                &weapon.attach_effect_required_max_counts,
                weapon.attach_effect_required_houses,
            )
        {
            return false;
        }
        true
    }

    /// One `WeaponTypeExt::DetonateAt`-style shot carried by an effect: a
    /// bullet of the weapon, created at the target and detonated there.
    fn attach_effect_weapon_detonation(
        &mut self,
        shot: &AttachEffectWeaponShot,
        target_id: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Option<crate::sim::projectile::ProjectileDetonation> {
        let weapon = rules.weapon(&shot.weapon)?;
        let warhead_name = weapon.warhead.as_deref()?;
        let impact = {
            let target = self.substrate.entities.get(target_id)?;
            crate::sim::movement::ground_pose::object_get_coords(
                target,
                self.resolved_terrain.as_ref(),
            )
        };
        let warhead_ref = self.interner.intern(warhead_name);
        let weapon_ref = self.interner.intern(&weapon.id);
        let projectile = weapon
            .projectile
            .as_deref()
            .map(|name| self.interner.intern(name));
        Some(crate::sim::projectile::ProjectileDetonation {
            projectile_id: target_id,
            source_id: shot.invoker.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
            target: crate::sim::projectile::ProjectileTarget::Entity(target_id),
            impact: crate::sim::projectile::ProjectileCoord::new(impact.x, impact.y, impact.z),
            payload: crate::sim::projectile::ProjectilePayload::new(
                weapon.damage,
                warhead_ref,
                projectile,
                weapon.bright,
            )
            .with_installed_weapon(Some(weapon_ref))
            .with_source_house(shot.house),
            reason: crate::sim::projectile::ProjectileDetonationReason::AttachEffectShot,
        })
    }

    /// `WarheadTypeExt::DetonateAt` (`src/Ext/WarheadType/Body.cpp:107`): a
    /// bullet with no weapon type, carrying `warhead` and `damage`.
    fn attach_effect_warhead_detonation(
        &mut self,
        warhead_name: &str,
        damage: i32,
        target_id: u64,
        invoker: Option<u64>,
        house: Option<crate::sim::intern::InternedId>,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Option<(
        crate::sim::projectile::ProjectileDetonation,
        crate::sim::intern::InternedId,
    )> {
        let warhead = rules.warhead(warhead_name)?;
        let impact = {
            let target = self.substrate.entities.get(target_id)?;
            crate::sim::movement::ground_pose::object_get_coords(
                target,
                self.resolved_terrain.as_ref(),
            )
        };
        let warhead_ref = self.interner.intern(warhead_name);
        Some((
            crate::sim::projectile::ProjectileDetonation {
                projectile_id: target_id,
                source_id: invoker.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
                target: crate::sim::projectile::ProjectileTarget::Entity(target_id),
                impact: crate::sim::projectile::ProjectileCoord::new(impact.x, impact.y, impact.z),
                payload: crate::sim::projectile::ProjectilePayload::new(
                    damage,
                    warhead_ref,
                    None,
                    warhead.bright,
                )
                .with_source_house(house),
                reason: crate::sim::projectile::ProjectileDetonationReason::AttachEffectShot,
            },
            warhead_ref,
        ))
    }

    /// Detonate the given shots through the ordinary bullet receiver chain.
    pub(crate) fn commit_attach_effect_detonations(
        &mut self,
        detonations: &[crate::sim::projectile::ProjectileDetonation],
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: crate::sim::world::FrameEffects<'_>,
    ) {
        if detonations.is_empty() {
            return;
        }
        let mut run =
            crate::sim::combat::world_receiver::ReceiverRun::default().with_effects(effects);
        let commit = crate::sim::combat::world_receiver::commit_projectiles(
            self,
            &mut run,
            detonations,
            rules,
            overlay_registry,
        );
        let navigation = run.finish();
        let _ = self.absorb_noncombat_damage_effects(
            rules,
            overlay_registry,
            commit.effects,
            commit.under_attack_events,
            navigation,
            effects,
        );
    }

    /// Phobos `TechnoClass_ReceiveDamage_ReflectDamage` (`0x701E18`): every
    /// active effect with `ReflectDamage` may deal the received, defense-
    /// divided damage back to the attacker, under its chance, warhead,
    /// multiplier/override and house gates. A reflected hit whose warhead is
    /// already marked `Reflected` never reflects again. `attack` is the
    /// post-divide value the native hook reads from `args->Damage`.
    pub(crate) fn apply_attach_effect_reflect_damage(
        &mut self,
        target_id: u64,
        attacker_id: u64,
        source_house: Option<crate::sim::intern::InternedId>,
        warhead_ref: crate::sim::intern::InternedId,
        damage: i32,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: crate::sim::world::FrameEffects<'_>,
    ) {
        if damage <= 0 || self.reflected_warhead == Some(warhead_ref) {
            return;
        }
        let Some(warhead) = rules.warhead(self.interner.resolve(warhead_ref)) else {
            return;
        };
        let info = warhead.attach_effect.clone();
        // Without a source object the native hook has nothing to hit.
        let Some(list) = self
            .substrate
            .entities
            .get(target_id)
            .map(|target| target.phobos_attach_effects().clone())
        else {
            return;
        };
        let victim_owner = self
            .substrate
            .entities
            .get(target_id)
            .map(|target| target.owner())
            .expect("the gated target still exists");
        let Some(attacker_owner) = source_house.or_else(|| {
            self.substrate
                .entities
                .get(attacker_id)
                .map(|attacker| attacker.owner())
        }) else {
            return;
        };
        let suppress = info.suppress_reflect_damage;
        let suppress_by_type = !info.suppress_reflect_damage_types.is_empty();
        let suppress_by_group = !info.suppress_reflect_damage_groups.is_empty();
        // `(!suppress || suppressByType || suppressByGroup)` gates the loop,
        // so a bare `SuppressReflectDamage` suppresses every effect before any
        // chance draw.
        if suppress && !suppress_by_type && !suppress_by_group {
            return;
        }
        let effects_table = rules.attach_effect_types().types();
        let mut actions: Vec<(
            bool,
            i32,
            Option<u64>,
            Option<crate::sim::intern::InternedId>,
            String,
            usize,
            i32,
        )> = Vec::new();
        let frame = self.session.binary_frame() as i32;
        for (instance_index, instance) in list.instances().iter().enumerate() {
            if !instance.is_active(instance.is_self_owned()) {
                continue;
            }
            let Some(effect) = effects_table.get(instance.type_index) else {
                continue;
            };
            if !effect.reflect_damage {
                continue;
            }
            // `attachEffect->ReflectDamageTimer.InProgress()` skips before the
            // chance draw (`Hooks.ReceiveDamage.cpp:429`).
            if effect.reflect_damage_delay > 0 && instance.reflect_damage_timer.remaining(frame) > 0
            {
                continue;
            }
            if effect.reflect_damage_chance < self.scenario_rng.random_double() {
                continue;
            }
            if suppress {
                if suppress_by_type
                    && info
                        .suppress_reflect_damage_types
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case(&effect.name))
                {
                    continue;
                }
                if suppress_by_group
                    && effect.groups.iter().any(|group| {
                        info.suppress_reflect_damage_groups
                            .iter()
                            .any(|suppressed| suppressed.eq_ignore_ascii_case(group))
                    })
                {
                    continue;
                }
            }
            let reflected = effect
                .reflect_damage_override
                .unwrap_or_else(|| scale_i32(damage, effect.reflect_damage_multiplier));
            let warhead_name = effect
                .reflect_damage_warhead
                .clone()
                .unwrap_or_else(|| rules.bridge_warheads.c4_name.clone());
            if warhead_name.is_empty() || rules.warhead(&warhead_name).is_none() {
                continue;
            }
            let affects = effect.reflect_damage_affects_house;
            let shot = if effect.reflect_damage_use_invoker_as_owner {
                let invoker = instance.source.and_then(|source| source.invoker);
                let house = instance.source.and_then(|source| source.invoker_house);
                let invoker_house = invoker.and_then(|entity| {
                    self.substrate
                        .entities
                        .get(entity)
                        .map(|entity| entity.owner())
                });
                if invoker.is_some()
                    && affects_house(
                        affects,
                        invoker_house,
                        attacker_owner,
                        &self.houses,
                        &self.house_alliances,
                        &self.interner,
                    )
                {
                    Some((invoker, invoker_house, true))
                } else if affects_house(
                    affects,
                    house,
                    attacker_owner,
                    &self.houses,
                    &self.house_alliances,
                    &self.interner,
                ) {
                    Some((None, house, true))
                } else {
                    None
                }
            } else if affects_house(
                affects,
                Some(victim_owner),
                attacker_owner,
                &self.houses,
                &self.house_alliances,
                &self.interner,
            ) {
                Some((Some(target_id), Some(victim_owner), false))
            } else {
                None
            };
            if let Some((invoker, house, detonate)) = shot {
                actions.push((
                    detonate,
                    reflected,
                    invoker,
                    house,
                    warhead_name,
                    instance_index,
                    effect.reflect_damage_delay,
                ));
            }
        }

        for (detonate, reflected, invoker, house, warhead_name, instance_index, delay) in actions {
            let previous = self
                .reflected_warhead
                .replace(self.interner.intern(&warhead_name));
            if detonate {
                if let Some((detonation, _)) = self.attach_effect_warhead_detonation(
                    &warhead_name,
                    reflected,
                    attacker_id,
                    invoker,
                    house,
                    rules,
                ) {
                    self.commit_attach_effect_detonations(
                        &[detonation],
                        rules,
                        overlay_registry,
                        effects,
                    );
                }
            } else {
                let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                    attacker_id,
                    reflected,
                    0,
                    invoker.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
                    house,
                    self.interner.intern(&warhead_name),
                    crate::sim::combat::ReceiverCallFlags {
                        ignore_defenses: false,
                        arg6: false,
                    },
                );
                self.commit_direct_damage_receiver(rules, overlay_registry, event, effects);
            }
            // The reflection starts the per-instance delay before the
            // reflected flag clears (`Hooks.ReceiveDamage.cpp:461..464`).
            if delay > 0
                && let Some(instance) =
                    self.substrate
                        .entities
                        .get_mut(target_id)
                        .and_then(|target| {
                            target
                                .phobos_attach_effects_mut()
                                .instances
                                .get_mut(instance_index)
                        })
            {
                instance.reflect_damage_timer.start(frame, delay);
            }
            self.reflected_warhead = previous;
        }
    }

    /// Phobos `TechnoExt::ApplyRevengeWeapon` (`src/Ext/Techno/WeaponHelpers.cpp:254`):
    /// once a victim dies from a sourced hit, every active effect carrying
    /// `RevengeWeapon` fires that weapon at the attacker, under the effect's
    /// house gate and invoker ownership.
    pub(crate) fn apply_attach_effect_revenge_weapons(
        &mut self,
        victim_id: u64,
        attacker_id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: crate::sim::world::FrameEffects<'_>,
    ) {
        let Some(victim) = self.substrate.entities.get(victim_id) else {
            return;
        };
        let Some(attacker) = self.substrate.entities.get(attacker_id) else {
            return;
        };
        let victim_owner = victim.owner();
        let attacker_owner = attacker.owner();
        let table = rules.attach_effect_types().types();
        let mut shots = Vec::new();
        for (instance, effect) in victim.phobos_attach_effects().active_effects(table) {
            if effect.revenge_weapon.is_none() {
                continue;
            }
            let Some(shot) = instance.revenge_weapon_shot(effect, victim_id, victim_owner) else {
                continue;
            };
            if !affects_house(
                effect.revenge_weapon_affects_house,
                shot.house,
                attacker_owner,
                &self.houses,
                &self.house_alliances,
                &self.interner,
            ) {
                continue;
            }
            shots.push(shot);
        }
        let detonations: Vec<_> = shots
            .iter()
            .filter_map(|shot| self.attach_effect_weapon_detonation(shot, attacker_id, rules))
            .collect();
        self.commit_attach_effect_detonations(&detonations, rules, overlay_registry, effects);
    }

    /// Phobos `TechnoClass_ReceiveDamage_AttachEffectExpireWeapon`
    /// (`0x702050`, `src/Ext/Techno/Hooks.ReceiveDamage.cpp:350`): when a techno
    /// dies, every attached instance whose type has `ExpireWeapon` and
    /// `TriggerOn=death` fires that weapon at the dying object's coordinates,
    /// before the native death weapon. A cumulative type with
    /// `CumulativeOnlyOnce` fires once; the scan follows attach order.
    pub(crate) fn apply_attach_effect_death_weapons(
        &mut self,
        dead_id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: crate::sim::world::FrameEffects<'_>,
    ) {
        let Some(victim) = self.substrate.entities.get(dead_id) else {
            return;
        };
        let owner_house = victim.owner();
        let table = rules.attach_effect_types().types();
        let mut once = std::collections::HashSet::new();
        let mut shots = Vec::new();
        // The native hook walks every instance, not just active ones.
        for instance in victim.phobos_attach_effects().instances() {
            let Some(effect) = table.get(instance.type_index) else {
                continue;
            };
            if effect.expire_weapon.is_none()
                || !effect
                    .expire_weapon_trigger_on
                    .contains(ExpireWeaponCondition::DEATH)
            {
                continue;
            }
            if effect.cumulative
                && effect.expire_weapon_cumulative_only_once
                && !once.insert(instance.type_index)
            {
                continue;
            }
            if let Some(shot) = instance.expire_weapon_shot_identity(
                effect,
                ExpireWeaponCondition::DEATH,
                dead_id,
                owner_house,
            ) {
                shots.push(shot);
            }
        }
        let detonations: Vec<_> = shots
            .iter()
            .filter_map(|shot| self.attach_effect_weapon_detonation(shot, dead_id, rules))
            .collect();
        self.commit_attach_effect_detonations(&detonations, rules, overlay_registry, effects);
    }

    /// `AttachEffectClass::RemoveAllOfType` (`AttachEffectClass.cpp:1131`): the
    /// list-position bounded removal pass. A cumulative type below
    /// `min_count` is kept entirely; at most `max_count` instances are
    /// removed; a recreatable instance resets instead and stays. Every removed
    /// instance contributes its `ExpireWeapon.TriggerOn=remove` shot.
    fn remove_attach_effect_type(
        &mut self,
        list: &mut AttachEffectList,
        type_index: usize,
        min_count: i32,
        max_count: i32,
        owner: u64,
        owner_house: crate::sim::intern::InternedId,
        effects: &[AttachEffectType],
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Vec<AttachEffectWeaponShot> {
        let Some(effect) = effects.get(type_index) else {
            return Vec::new();
        };
        let mut shots = Vec::new();
        // `stackCount` keeps the active count read before any removal and is
        // -1 for a non-cumulative type.
        let stack = if effect.cumulative {
            list.active_count(type_index)
        } else {
            -1
        };
        if min_count > 0 && stack > -1 && min_count > stack {
            return shots;
        }
        let ignore_cumulative_count_check = stack != 1;
        let mut detached = 0;
        let mut index = 0;
        while index < list.instances.len() {
            if max_count > 0 && detached >= max_count {
                break;
            }
            if list.instances[index].type_index != type_index {
                index += 1;
                continue;
            }
            detached += 1;
            let active_count = list.active_count(type_index);
            if let Some(shot) = list.instances[index].expire_weapon_shot(
                effect,
                ExpireWeaponCondition::REMOVE,
                owner,
                owner_house,
                active_count,
                ignore_cumulative_count_check,
            ) {
                shots.push(shot);
            }
            if list.instances[index].reset_if_recreatable() {
                index += 1;
                continue;
            }
            if let Some(anim) = list.instances[index].anim.take() {
                self.destroy_anim_with_context(anim, rules);
            }
            list.instances.remove(index);
            if !effect.cumulative {
                break;
            }
        }
        shots
    }

    /// Phobos `WarheadTypeExt::ApplyAttachEffects` (`src/Ext/WarheadType/Detonate.cpp:726`),
    /// reached per target before its damage: attach every `AttachTypes` entry,
    /// then remove `RemoveTypes` and every type carrying a `RemoveGroups`
    /// group. `invoker` and `source_house` are the detonation's attacker and
    /// decided house; the caller, `crate::sim::combat::crit::detonate`, owns
    /// `DetonateOnOneUnit`'s target gate.
    pub(crate) fn apply_warhead_attach_effects(
        &mut self,
        target_id: u64,
        warhead_ref: crate::sim::intern::InternedId,
        invoker: Option<u64>,
        source_house: Option<crate::sim::intern::InternedId>,
        rules: &crate::rules::ruleset::RuleSet,
        effects: crate::sim::world::FrameEffects<'_>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let Some(warhead) = rules.warhead(self.interner.resolve(warhead_ref)) else {
            return;
        };
        let info = warhead.attach_effect.clone();
        if info.is_empty() {
            return;
        }
        let Some(target) = self.substrate.entities.get(target_id) else {
            return;
        };
        let target_owner = target.owner();
        // `CreateAndAttach` gating reads the live target; snapshot the pair
        // before the attach list mutably borrows the entity.
        let target_category = target.category();
        let target_invulnerability = target.invulnerability.clone();
        let target_type_ref = target.type_ref();
        let Some(target_type) = self.interner.object_type(target_type_ref, rules) else {
            return;
        };
        let catalog = rules.attach_effect_types();
        let table = catalog.types();
        let policy = AttachPolicy {
            replace_longer_duration: info
                .replace_longer_duration
                .unwrap_or_else(|| rules.attach_effect_globals().replace_longer_duration),
            cumulative_source_max_count: info.cumulative_source_max_count,
            cumulative_refresh_all: info.cumulative_refresh_all,
            cumulative_refresh_all_on_attach: info.cumulative_refresh_all_on_attach,
            cumulative_refresh_same_source_only: info.cumulative_refresh_same_source_only,
        };
        let source = Some(AttachSource {
            invoker,
            source: Some(u64::from(warhead_ref.index())),
            invoker_house: source_house,
        });
        let Some(target) = self.substrate.entities.get_mut(target_id) else {
            return;
        };
        let mut list = std::mem::take(target.phobos_attach_effects_mut());
        let frame = self.session.binary_frame();
        for (index, name) in info.attach_types.iter().enumerate() {
            let Some(type_index) = catalog.index_of(name) else {
                continue;
            };
            let effect = &table[type_index];
            if !can_attach_effect(
                target_category.into(),
                target_invulnerability.as_ref(),
                target_type,
                effect,
                frame,
            ) {
                continue;
            }
            let params = AttachEffectParams {
                // `GetAttachParams(index, false)`: the warhead carries the
                // duration override but no delay lists, and its delay is -1.
                duration_override: paired(&info.duration_overrides, index, 0),
                delay: -1,
                initial_delay: 0,
                recreation_delay: -1,
            };
            list.attach(type_index, params, effect, policy, source);
        }
        let mut shots = Vec::new();
        for (position, name) in info.remove_types.iter().enumerate() {
            let Some(type_index) = catalog.index_of(name) else {
                continue;
            };
            let min_count = paired(&info.cumulative_remove_min_counts, position, -1);
            let max_count = paired(&info.cumulative_remove_max_counts, position, -1);
            shots.extend(self.remove_attach_effect_type(
                &mut list,
                type_index,
                min_count,
                max_count,
                target_id,
                target_owner,
                table,
                rules,
            ));
        }
        let group_types =
            AttachEffectList::types_with_groups(table.len(), &info.remove_groups, table);
        for (position, type_index) in group_types.into_iter().enumerate() {
            let min_count = paired(&info.cumulative_remove_min_counts, position, -1);
            let max_count = paired(&info.cumulative_remove_max_counts, position, -1);
            shots.extend(self.remove_attach_effect_type(
                &mut list,
                type_index,
                min_count,
                max_count,
                target_id,
                target_owner,
                table,
                rules,
            ));
        }
        if let Some(target) = self.substrate.entities.get_mut(target_id) {
            *target.phobos_attach_effects_mut() = list;
            target.recalculate_phobos_stat_multipliers(table);
        }
        let detonations: Vec<_> = shots
            .iter()
            .filter_map(|shot| self.attach_effect_weapon_detonation(shot, target_id, rules))
            .collect();
        self.commit_attach_effect_detonations(&detonations, rules, overlay_registry, effects);
    }
}

/// Phobos `EnumFunctions::CanTargetHouse` with a nullable source: the
/// shared port handles a known source; a hit with none follows the enemy
/// bucket, as the AttachEffect branch has always read it.
#[cfg(feature = "phobos")]
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
    crate::sim::combat::can_target_house(
        flags,
        source,
        target,
        houses,
        alliances.as_map(),
        interner,
    )
}

#[cfg(all(test, feature = "phobos"))]
mod tests {
    use super::*;

    fn effect(duration: i32) -> AttachEffectType {
        let mut effect = AttachEffectType::default();
        effect.duration = duration;
        effect
    }

    /// The initial delay gates everything, then the duration counts down and a
    /// terminal (`Delay < 0`) effect expires at zero.
    #[test]
    fn initial_delay_gates_the_countdown_and_a_terminal_effect_expires() {
        let effect = effect(3);
        let mut instance = AttachEffectInstance::new(
            0,
            AttachEffectParams {
                initial_delay: 2,
                delay: -1,
                ..AttachEffectParams::default()
            },
            &effect,
        );
        assert_eq!(
            instance.remaining(),
            3,
            "constructor takes the type duration"
        );
        instance.tick(&effect, false);
        instance.tick(&effect, false);
        assert_eq!(instance.remaining(), 3, "initial delay holds the duration");
        for _ in 0..3 {
            instance.tick(&effect, false);
        }
        assert_eq!(instance.remaining(), 0);
        assert!(instance.has_expired(), "Delay < 0 ends the effect");
        instance.tick(&effect, false);
        assert_eq!(instance.remaining(), 0, "a terminal effect stays ended");
    }

    /// `Delay = 0` refills the duration in the same frame, and a re-creating
    /// effect never reports expired.
    #[test]
    fn a_zero_delay_recreates_in_the_same_frame() {
        let effect = effect(2);
        let mut instance = AttachEffectInstance::new(
            0,
            AttachEffectParams {
                delay: 0,
                ..AttachEffectParams::default()
            },
            &effect,
        );
        instance.tick(&effect, false);
        assert_eq!(instance.remaining(), 1);
        instance.tick(&effect, false);
        assert_eq!(instance.remaining(), 2, "zero delay refreshes on the spot");
        assert!(!instance.should_refresh(), "no deferred refresh was queued");
        assert!(
            !instance.has_expired(),
            "a re-creating effect never expires"
        );
    }

    /// Behind a discard check the same zero delay is deferred instead.
    #[test]
    fn a_zero_delay_under_discard_refreshes_on_a_later_frame() {
        let effect = effect(1);
        let mut instance = AttachEffectInstance::new(
            0,
            AttachEffectParams {
                delay: 0,
                ..AttachEffectParams::default()
            },
            &effect,
        );
        instance.tick(&effect, true);
        assert_eq!(instance.remaining(), 0);
        assert!(
            instance.should_refresh(),
            "queued, not refilled while discarded"
        );
        instance.tick(&effect, false);
        assert_eq!(instance.remaining(), 1);
    }

    /// A pending re-creation delay counts down and then refreshes; discarding
    /// freezes it, as `ShouldBeDiscardedNow` gates the native branch.
    #[test]
    fn a_recreation_delay_counts_down_and_discarding_freezes_it() {
        let effect = effect(1);
        let mut instance = AttachEffectInstance::new(
            0,
            AttachEffectParams {
                delay: 2,
                ..AttachEffectParams::default()
            },
            &effect,
        );
        instance.tick(&effect, false);
        assert_eq!(
            instance.current_delay(),
            2,
            "duration 1 -> 0 arms the delay"
        );
        instance.tick(&effect, true);
        assert_eq!(instance.current_delay(), 2, "discarded delays do not tick");
        instance.tick(&effect, false);
        assert_eq!(instance.current_delay(), 1);
        instance.tick(&effect, false);
        assert_eq!(instance.current_delay(), 0);
        assert!(instance.should_refresh());
    }

    /// The override wins over the type duration, and a negative recreation
    /// delay refuses `ResetIfRecreatable`.
    #[test]
    fn override_wins_and_a_negative_recreation_delay_refuses_the_reset() {
        let effect = effect(5);
        let mut instance = AttachEffectInstance::new(
            0,
            AttachEffectParams {
                duration_override: 7,
                recreation_delay: -1,
                ..AttachEffectParams::default()
            },
            &effect,
        );
        assert_eq!(instance.remaining(), 7);
        assert!(!instance.reset_if_recreatable());
        assert_eq!(instance.remaining(), 7, "a refused reset changes nothing");
    }

    fn cumulative_effect(duration: i32, max_count: i32) -> AttachEffectType {
        let mut effect = effect(duration);
        effect.cumulative = true;
        effect.cumulative_max_count = max_count;
        effect
    }

    fn attach_with(
        list: &mut AttachEffectList,
        effect: &AttachEffectType,
        override_duration: i32,
    ) -> AttachOutcome {
        list.attach(
            0,
            AttachEffectParams {
                duration_override: override_duration,
                delay: -1,
                ..AttachEffectParams::default()
            },
            effect,
            AttachPolicy::default(),
            None,
        )
    }

    /// The first AI frame creates the effect's animation through the animation
    /// owner, attached to the techno with its loops held; the frame that ends
    /// the duration removes the effect and retires the animation.
    #[test]
    fn the_effect_animation_is_created_attached_and_retired() {
        use crate::rules::art_data::ArtRegistry;
        use crate::rules::ini_parser::IniFile;
        let ini = crate::sim::arena_fixture::clear_ground_ini(
            "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=1000\nSpeed=6\nAttachEffect.AttachTypes=PULSEAE\n\
             AttachEffect.Delays=-1\n\
             [AttachEffectTypes]\n0=PULSEAE\n\
             [PULSEAE]\nDuration=3\nAnimation=PULSE\n",
        );
        let art = IniFile::from_str("[PULSE]\nRate=900\nEnd=4\n");
        let processed = crate::rules::native_processing::RulesLayerStack::new(ini)
            .process_with_fixed_art(&art)
            .expect("rules process");
        let mut rules =
            crate::rules::ruleset::RuleSet::from_processed_rules(&processed).expect("rules");
        let mut registry = ArtRegistry::from_ini(&art);
        registry.bind_anim_frame_count_for_test("PULSE", 4);
        rules.install_art_data(registry);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let target = sim
            .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
            .expect("spawn");

        let instances = sim
            .entities()
            .get(target)
            .unwrap()
            .phobos_attach_effects()
            .instances();
        assert_eq!(instances.len(), 1);
        assert!(
            instances[0].anim().is_none(),
            "the constructor only attaches; AI creates the animation"
        );

        sim.update_phobos_attach_effects(target, &rules);
        let anim = sim
            .entities()
            .get(target)
            .unwrap()
            .phobos_attach_effects()
            .instances()[0]
            .anim()
            .expect("the first AI frame creates the animation");
        assert_eq!(
            sim.anim(anim).unwrap().owner_entity(),
            Some(target),
            "SetOwnerObject binds the animation to its techno"
        );
        assert_eq!(
            sim.anim(anim).unwrap().runtime().loop_remaining,
            u8::MAX,
            "the effect holds its animation loops"
        );
        assert!(
            sim.interner
                .resolve(sim.anim(anim).unwrap().type_id())
                .eq_ignore_ascii_case("PULSE")
        );
        let position = crate::sim::movement::ground_pose::position_world_coord(
            &sim.entities().get(target).unwrap().position,
        );
        assert_eq!(
            sim.anim_absolute_coord(anim),
            Some(crate::sim::anim_class::AnimWorldCoord {
                x: position.x,
                y: position.y,
                z: position.z,
            }),
            "an attached animation follows its owner's coordinate"
        );

        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(
            sim.entities()
                .get(target)
                .unwrap()
                .phobos_attach_effects()
                .instances()[0]
                .remaining(),
            1
        );
        sim.update_phobos_attach_effects(target, &rules);
        assert!(
            sim.entities()
                .get(target)
                .unwrap()
                .phobos_attach_effects()
                .is_empty(),
            "the frame that ends the duration removes the effect"
        );
        assert!(
            sim.substrate.pending_delete.contains_object(&anim),
            "the removed effect retires its animation"
        );
    }

    /// `Animation.HideIfAttachedWith`: a listed type being attached keeps the
    /// effect's animation away; when that type expires the next frame
    /// recreates it.
    #[test]
    fn hide_if_attached_with_hides_and_restores_the_animation() {
        use crate::rules::art_data::ArtRegistry;
        use crate::rules::ini_parser::IniFile;
        let ini = crate::sim::arena_fixture::clear_ground_ini(
            "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=1000\nSpeed=6\nAttachEffect.AttachTypes=SHOOTER,HIDER\n\
             AttachEffect.Delays=-1\n\
             [AttachEffectTypes]\n0=SHOOTER\n1=HIDER\n\
             [SHOOTER]\nDuration=100\nAnimation=PULSE\n\
             Animation.HideIfAttachedWith=HIDER\n\
             [HIDER]\nDuration=3\n",
        );
        let art = IniFile::from_str("[PULSE]\nRate=900\nEnd=4\n");
        let processed = crate::rules::native_processing::RulesLayerStack::new(ini)
            .process_with_fixed_art(&art)
            .expect("rules process");
        let mut rules =
            crate::rules::ruleset::RuleSet::from_processed_rules(&processed).expect("rules");
        let mut registry = ArtRegistry::from_ini(&art);
        registry.bind_anim_frame_count_for_test("PULSE", 4);
        rules.install_art_data(registry);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let target = sim
            .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
            .expect("spawn");
        let shooter_anim = |sim: &Simulation| {
            sim.entities()
                .get(target)
                .unwrap()
                .phobos_attach_effects()
                .instances()
                .iter()
                .find(|instance| instance.type_index == 0)
                .and_then(|instance| instance.anim())
        };

        sim.update_phobos_attach_effects(target, &rules);
        assert!(
            shooter_anim(&sim).is_none(),
            "HIDER is attached, so the animation stays hidden"
        );
        sim.update_phobos_attach_effects(target, &rules);
        assert!(shooter_anim(&sim).is_none());
        sim.update_phobos_attach_effects(target, &rules);
        assert!(
            shooter_anim(&sim).is_some(),
            "the expired HIDER no longer hides the animation"
        );
    }

    /// Phobos' C++ int casts: truncation toward zero, `i32::MIN` for a NaN or
    /// out-of-range product.
    #[test]
    fn scale_i32_matches_the_cpp_cast() {
        assert_eq!(scale_i32(7, 1.0), 7);
        assert_eq!(scale_i32(7, 1.5), 10);
        assert_eq!(scale_i32(7, -0.5), -3);
        assert_eq!(scale_i32(7, f64::INFINITY), i32::MIN);
        assert_eq!(scale_i32(7, f64::NAN), i32::MIN);
        assert_eq!(scale_i32(i32::MAX, 2.0), i32::MIN);
        assert_eq!(scale_i32(i32::MIN, 1.0), i32::MIN);
    }

    /// `ROFMultiplier.ApplyOnCurrentTimer`: the rearm timer already running
    /// when the effect first initializes is scaled; the key can turn it off.
    #[test]
    fn rof_multiplier_scales_the_running_rearm_timer_on_initialization() {
        use crate::rules::ini_parser::IniFile;

        for (apply, expected) in [(false, 30), (true, 60)] {
            let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(&format!(
                "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
                 [TARGET]\nStrength=1000\nSpeed=6\nAttachEffect.AttachTypes=ROFAE\n\
                 AttachEffect.InitialDelays=1\n\
                 [AttachEffectTypes]\n0=ROFAE\n\
                 [ROFAE]\nDuration=10\nROFMultiplier=2\n\
                 ROFMultiplier.ApplyOnCurrentTimer={}\n",
                if apply { "yes" } else { "no" }
            )))
            .expect("rules");
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let target = sim
                .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
                .expect("spawn");
            let frame = sim.session().binary_frame() as i32;
            sim.substrate
                .entities
                .get_mut(target)
                .unwrap()
                .rearm_timer
                .start(frame, 30);

            sim.update_phobos_attach_effects(target, &rules);
            assert_eq!(
                sim.entities()
                    .get(target)
                    .unwrap()
                    .rearm_timer
                    .remaining(frame),
                30,
                "the initial-delay frame never initializes the effect"
            );
            sim.update_phobos_attach_effects(target, &rules);
            assert_eq!(
                sim.entities()
                    .get(target)
                    .unwrap()
                    .rearm_timer
                    .remaining(frame),
                expected,
                "ApplyOnCurrentTimer={apply}"
            );
        }
    }

    /// Phobos `CalculateArmorMultipliers`'s restricted arm: one chance draw
    /// per active restricted effect (even when it can never pass), the
    /// warhead gates, and the picked HitAnim.
    #[test]
    fn restricted_armor_rolls_chance_gates_and_hit_animations() {
        use crate::rules::art_data::ArtRegistry;
        use crate::rules::ini_parser::IniFile;

        let scene = |roll: &str| {
            let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
                "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
                 [TARGET]\nStrength=1000\nSpeed=6\nAttachEffect.AttachTypes=BASE,ROLL\n\
                 [Warheads]\n0=HIT\n1=OTHER\n[HIT]\n[OTHER]\n\
                 [AttachEffectTypes]\n0=BASE\n1=ROLL\n\
                 [BASE]\nDuration=10\nArmorMultiplier=0.5\n\
                 [ROLL]\nDuration=10\nArmorMultiplier=0.5\n{roll}"
            ));
            let art = IniFile::from_str("[PULSE]\nRate=900\nEnd=4\n");
            let processed = crate::rules::native_processing::RulesLayerStack::new(ini)
                .process_with_fixed_art(&art)
                .expect("rules process");
            let mut rules =
                crate::rules::ruleset::RuleSet::from_processed_rules(&processed).expect("rules");
            let mut registry = ArtRegistry::from_ini(&art);
            registry.bind_anim_frame_count_for_test("PULSE", 4);
            rules.install_art_data(registry);
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let target = sim
                .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
                .expect("spawn");
            sim.update_phobos_attach_effects(target, &rules);
            (sim, rules, target)
        };

        // A passing roll multiplies the unrestricted base and spawns the pick.
        let (mut sim, rules, target) =
            scene("ArmorMultiplier.Chance=1.0\nArmorMultiplier.HitAnim=PULSE\n");
        let hit = sim.interner.intern("HIT");
        let owner = sim.entities().get(target).unwrap().owner();
        let before = sim.scenario_rng.native_state_hex();
        assert_eq!(
            sim.attach_effect_armor_multiplier(target, Some(hit), Some(owner), &rules),
            0.25,
            "base 0.5 times the applied 0.5"
        );
        assert_ne!(
            before,
            sim.scenario_rng.native_state_hex(),
            "the chance and the pick both drew"
        );
        assert!(
            sim.substrate.anims.iter().any(|(_, anim)| {
                anim.owner_entity() == Some(target)
                    && sim
                        .interner()
                        .resolve(anim.type_id())
                        .eq_ignore_ascii_case("PULSE")
            }),
            "the picked HitAnim is attached to the target"
        );

        // Chance 0 still draws, then rejects.
        let (mut sim, rules, target) = scene("ArmorMultiplier.Chance=0.0\n");
        let hit = sim.interner.intern("HIT");
        let before = sim.scenario_rng.native_state_hex();
        assert_eq!(
            sim.attach_effect_armor_multiplier(target, Some(hit), None, &rules),
            0.5,
            "the base product survives the failed roll"
        );
        assert_ne!(
            before,
            sim.scenario_rng.native_state_hex(),
            "the chance draw happens even at zero"
        );

        // The warhead lists gate the same multiplier.
        let (mut sim, rules, target) = scene("ArmorMultiplier.DisallowWarheads=HIT\n");
        let hit = sim.interner.intern("HIT");
        let other = sim.interner.intern("OTHER");
        assert_eq!(
            sim.attach_effect_armor_multiplier(target, Some(hit), None, &rules),
            0.5,
            "a disallowed warhead skips the effect"
        );
        assert_eq!(
            sim.attach_effect_armor_multiplier(target, Some(other), None, &rules),
            0.25,
            "an unlisted warhead passes"
        );

        // `ArmorMultiplier.Delay`: a real hit starts the per-instance timer,
        // which skips later rolls until it runs out.
        let (mut sim, rules, target) = scene("ArmorMultiplier.Delay=5\n");
        let hit = sim.interner.intern("HIT");
        assert_eq!(
            sim.attach_effect_armor_multiplier(target, Some(hit), None, &rules),
            0.25,
            "the first hit applies the multiplier"
        );
        let before = sim.scenario_rng.native_state_hex();
        assert_eq!(
            sim.attach_effect_armor_multiplier(target, Some(hit), None, &rules),
            0.5,
            "the running delay skips the effect"
        );
        assert_eq!(
            before,
            sim.scenario_rng.native_state_hex(),
            "a skipped effect draws nothing"
        );
        sim.session.set_binary_frame_for_test(6);
        assert_eq!(
            sim.attach_effect_armor_multiplier(target, Some(hit), None, &rules),
            0.25,
            "once the delay runs out the multiplier applies again"
        );
    }

    /// Phobos' Unkillable arm: a hit whose warhead total damage reaches the
    /// target's health leaves Health and EstimatedHealth at 1; a nonlethal
    /// packet or an object without the effect is untouched.
    #[test]
    fn unkillable_clamps_a_lethal_receive() {
        use crate::rules::ini_parser::IniFile;
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n1=PLAIN\n\
             [Warheads]\n0=HIT\n\
             [HIT]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [TARGET]\nStrength=500\nSpeed=6\nArmor=light\n\
             AttachEffect.AttachTypes=AE_UK\n\
             [PLAIN]\nStrength=500\nSpeed=6\nArmor=light\n\
             [AttachEffectTypes]\n0=AE_UK\n\
             [AE_UK]\nDuration=-1\nUnkillable=yes\n",
        ))
        .expect("rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let target = sim
            .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
            .expect("spawn");
        sim.update_phobos_attach_effects(target, &rules);
        let warhead = sim.interner.intern("HIT");
        let event = crate::sim::combat::EntityDamageEvent::area(
            target,
            100,
            0,
            crate::sim::combat::RAD_NO_ATTACKER,
            None,
            warhead,
        );
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .health
            .set(50);
        assert!(sim.apply_unkillable_clamp(target, &event, 100, &rules));
        assert_eq!(sim.entities().get(target).unwrap().health.current(), 1);
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .health
            .set(500);
        assert!(
            !sim.apply_unkillable_clamp(target, &event, 100, &rules),
            "100 point of damage does not reach 500 health"
        );
        assert_eq!(sim.entities().get(target).unwrap().health.current(), 500);

        let plain = sim
            .spawn_object("PLAIN", "Americans", 22, 20, 0, &rules)
            .expect("spawn");
        sim.substrate
            .entities
            .get_mut(plain)
            .unwrap()
            .health
            .set(50);
        let event = crate::sim::combat::EntityDamageEvent::area(
            plain,
            100,
            0,
            crate::sim::combat::RAD_NO_ATTACKER,
            None,
            warhead,
        );
        assert!(
            !sim.apply_unkillable_clamp(plain, &event, 100, &rules),
            "an object without the effect takes the lethal hit"
        );
    }

    /// `GetRangeWithModifiers`: multipliers apply in attach order, summed
    /// `ExtraRange` cells are added once after every multiplier, and the
    /// weapon lists gate both. The `-512` sentinel returns before the fold.
    #[test]
    fn attach_effect_range_multiplies_then_adds_extra_cells() {
        use crate::rules::ini_parser::IniFile;
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
             [WeaponTypes]\n0=GUN\n[GUN]\nRange=5\nDamage=10\nROF=20\n\
             [TARGET]\nStrength=1000\nSpeed=6\nPrimary=GUN\n\
             AttachEffect.AttachTypes=A,B\n\
             [AttachEffectTypes]\n0=A\n1=B\n\
             [A]\nDuration=10\nWeaponRange.Multiplier=1.5\n\
             [B]\nDuration=10\nWeaponRange.Multiplier=2\n\
             WeaponRange.ExtraRange=1\nWeaponRange.AllowWeapons=GUN\n",
        ))
        .expect("rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let target = sim
            .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
            .expect("spawn");
        sim.update_phobos_attach_effects(target, &rules);
        let entity = sim.entities().get(target).unwrap();
        let obj = rules.object("TARGET").expect("target type");
        assert_eq!(
            crate::sim::combat::combat_weapon::weapon_range(
                entity,
                obj,
                0,
                sim.entities(),
                &rules,
                sim.interner(),
            ),
            5 * 256 * 3 + 256,
            "5 cells times 1.5 times 2, then one extra cell"
        );
        let fold = |weapon: &str, range: i32| {
            crate::sim::attach_effect::range_with_attach_effects(entity, weapon, range, &rules)
        };
        assert_eq!(
            fold("OTHER", 1000),
            1500,
            "B's allow-list skips it entirely"
        );
        assert_eq!(fold("gun", 1000), 3256, "the weapon match ignores case");
        assert_eq!(
            fold("GUN", -512),
            -512,
            "the always-in-range sentinel precedes the fold"
        );
        assert_eq!(
            fold("OTHER", i32::MAX),
            i32::MAX,
            "SafeMultiply clamps instead of overflowing"
        );
        assert_eq!(
            fold("GUN", -100),
            0,
            "-100 * 1.5 * 2 + 256 = -44 floors at zero"
        );
    }

    /// `OpenTopped.UseTransportRangeModifiers`: inside an `OpenTopped`
    /// transport that enables the key, a rider folds the transport's
    /// `WeaponRange` effects instead of its own.
    #[test]
    fn an_open_topped_transport_lends_its_range_modifiers_to_riders() {
        use crate::rules::ini_parser::IniFile;
        let scene = |enabled: bool| {
            let key = if enabled {
                "OpenTopped.UseTransportRangeModifiers=yes\n"
            } else {
                ""
            };
            let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(&format!(
                "[Countries]\n0=Americans\n[VehicleTypes]\n0=TRUCK\n1=RIDER\n\
                 [WeaponTypes]\n0=GUN\n[GUN]\nRange=5\nDamage=10\nROF=20\n\
                 [TRUCK]\nStrength=500\nSpeed=6\nOpenTopped=yes\nPassengers=1\n{key}\
                 AttachEffect.AttachTypes=TRANSPORT_RANGE\n\
                 [RIDER]\nStrength=100\nSpeed=4\nPrimary=GUN\n\
                 AttachEffect.AttachTypes=RIDER_RANGE\n\
                 [AttachEffectTypes]\n0=TRANSPORT_RANGE\n1=RIDER_RANGE\n\
                 [TRANSPORT_RANGE]\nDuration=100\nWeaponRange.Multiplier=2\n\
                 [RIDER_RANGE]\nDuration=100\nWeaponRange.Multiplier=3\n"
            )))
            .expect("rules");
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let truck = sim
                .spawn_object("TRUCK", "Americans", 20, 20, 0, &rules)
                .expect("truck");
            let rider = sim
                .spawn_object("RIDER", "Americans", 21, 20, 0, &rules)
                .expect("rider");
            sim.update_phobos_attach_effects(truck, &rules);
            sim.update_phobos_attach_effects(rider, &rules);
            let range = move |sim: &crate::sim::world::Simulation| {
                crate::sim::combat::combat_weapon::weapon_range(
                    sim.entities().get(rider).unwrap(),
                    rules.object("RIDER").expect("rider type"),
                    0,
                    sim.entities(),
                    &rules,
                    sim.interner(),
                )
            };
            (sim, truck, rider, range)
        };

        let (mut sim, truck, rider, range) = scene(true);
        assert_eq!(
            range(&sim),
            5 * 256 * 3,
            "outside a transport the rider folds its own effects"
        );
        sim.substrate
            .entities
            .get_mut(rider)
            .unwrap()
            .passenger_role = crate::sim::passenger::PassengerRole::inside_for_test(truck, true);
        assert_eq!(
            range(&sim),
            5 * 256 * 2,
            "the transport's fold applies while aboard"
        );

        let (mut sim, truck, rider, range) = scene(false);
        sim.substrate
            .entities
            .get_mut(rider)
            .unwrap()
            .passenger_role = crate::sim::passenger::PassengerRole::inside_for_test(truck, true);
        assert_eq!(
            range(&sim),
            5 * 256 * 3,
            "the default keeps the rider's own fold"
        );
    }

    /// `ShouldBeDiscardedNow`'s continuous conditions over injected facts.
    #[test]
    fn discard_conditions_follow_their_facts() {
        use crate::rules::attach_effect::{AttachEffectType, DiscardOn};
        let mut effect = AttachEffectType::default();
        effect.duration = 10;
        let mut list = AttachEffectList::default();
        list.attach(
            0,
            AttachEffectParams {
                delay: -1,
                ..AttachEffectParams::default()
            },
            &effect,
            AttachPolicy::default(),
            None,
        );
        let instance = list.instances()[0].clone();
        let globals = crate::rules::attach_effect::AttachEffectGlobals::default();

        // Ammo `[min, max]`, inclusive, `-1` unbounded.
        effect.discard_on = DiscardOn::AMMO;
        effect.discard_ammo_minimum = 2;
        effect.discard_ammo_maximum = 4;
        let mut facts = DiscardFacts::default();
        for (ammo, expected) in [(1, false), (2, true), (4, true), (5, false)] {
            facts.ammo = ammo;
            assert_eq!(
                instance.should_be_discarded_now(&effect, &facts, &globals),
                expected,
                "ammo {ammo}"
            );
        }
        effect.discard_ammo_maximum = -1;
        facts.ammo = 9;
        assert!(instance.should_be_discarded_now(&effect, &facts, &globals));

        // Health `(above, below]`, above inclusive at zero health.
        effect.discard_on = DiscardOn::HEALTH;
        effect.discard_health_above = 0.2;
        effect.discard_health_below = 0.5;
        facts = DiscardFacts::default();
        for (health, strength, expected) in [
            (50, 100, true),
            (20, 100, false),
            (60, 100, false),
            (0, 0, true),
            (0, 100, false),
        ] {
            facts.health = health;
            facts.strength = strength;
            assert_eq!(
                instance.should_be_discarded_now(&effect, &facts, &globals),
                expected,
                "health {health}/{strength}"
            );
        }

        // Move / stationary / harvesting.
        effect.discard_on = DiscardOn::MOVE;
        facts = DiscardFacts {
            is_foot: true,
            is_really_moving: true,
            ..DiscardFacts::default()
        };
        assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
        facts.is_really_moving = false;
        assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));
        effect.discard_move_based_on_destination = Some(true);
        facts.is_moving = true;
        assert!(
            instance.should_be_discarded_now(&effect, &facts, &globals),
            "the destination query replaces the locomotor motion query"
        );
        facts.is_moving = false;

        effect.discard_on = DiscardOn::STATIONARY;
        effect.discard_consider_harvesting_as_stationary = Some(true);
        assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
        facts.harvesting = true;
        assert!(
            instance.should_be_discarded_now(&effect, &facts, &globals),
            "harvesting counts as stationary on this switch"
        );
        effect.discard_consider_harvesting_as_stationary = Some(false);
        effect.discard_on = DiscardOn::HARVESTING;
        assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
        facts.harvesting = false;
        assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));
        effect.discard_on = DiscardOn::STATIONARY;
        facts.harvesting = true;
        assert!(
            !instance.should_be_discarded_now(&effect, &facts, &globals),
            "harvesting is not stationary on this switch"
        );
        facts.harvesting = false;
        facts.mission_harvest_on_tiberium = true;
        assert!(
            !instance.should_be_discarded_now(&effect, &facts, &globals),
            "the intermediate harvest state is neither"
        );

        // Selling vs undeploying split on the archive target.
        effect.discard_on = DiscardOn::SELLING;
        facts = DiscardFacts {
            is_building: true,
            selling: true,
            ..DiscardFacts::default()
        };
        assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
        effect.discard_on = DiscardOn::UNDEPLOYING;
        assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));
        facts.undeploying = true;
        assert!(instance.should_be_discarded_now(&effect, &facts, &globals));

        // Land type names.
        effect.discard_on = DiscardOn::LAND_TYPE;
        effect.discard_land_types = vec!["Clear".to_owned()];
        facts = DiscardFacts {
            land_type: Some(crate::rules::terrain_rules::LandType::Clear),
            ..DiscardFacts::default()
        };
        assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
        facts.land_type = Some(crate::rules::terrain_rules::LandType::Road);
        assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));

        // Sequence: immediate matches the current action; deferred matches
        // the previous one once it changes.
        use crate::rules::animation_sequence::SequenceKind;
        effect.discard_on = DiscardOn::SEQUENCE;
        effect.discard_sequences = vec!["walk".to_owned()];
        effect.discard_sequences_immediate = Some(true);
        facts = DiscardFacts {
            sequence: Some(SequenceKind::Walk),
            ..DiscardFacts::default()
        };
        assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
        facts.sequence = Some(SequenceKind::Stand);
        assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));
        effect.discard_sequences_immediate = Some(false);
        let mut deferred = instance.clone();
        deferred.last_sequence_check = Some(SequenceKind::Walk);
        assert!(
            deferred.should_be_discarded_now(&effect, &facts, &globals),
            "the previous matching sequence discards on the change"
        );
        deferred.last_sequence_check = Some(SequenceKind::Stand);
        assert!(!deferred.should_be_discarded_now(&effect, &facts, &globals));

        // InRange/OutOfRange use the held target's distance and the selected
        // weapon's range; RangeOverride replaces the range in cells.
        effect.discard_on = DiscardOn::IN_RANGE;
        effect.discard_range_override = None;
        facts = DiscardFacts {
            target: Some((100, 200)),
            ..DiscardFacts::default()
        };
        assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
        facts.target = Some((300, 200));
        assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));
        effect.discard_on = DiscardOn::OUT_OF_RANGE;
        assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
        facts.target = Some((200, 200));
        assert!(
            instance.should_be_discarded_now(&effect, &facts, &globals),
            "the exact range boundary discards either way"
        );
        effect.discard_on = DiscardOn::IN_RANGE;
        assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
        effect.discard_range_override = Some(0.5);
        facts.target = Some((100, 9_999));
        assert!(
            instance.should_be_discarded_now(&effect, &facts, &globals),
            "the override replaces the weapon range"
        );
        facts.target = Some((200, 9_999));
        assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));
        facts.target = None;
        assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));

        // Mission list: a human owner reads DiscardOn.Missions, an AI reads
        // DiscardOn.AIMissions when it is set.
        use crate::rules::mission_data::MissionType;
        effect.discard_on = DiscardOn::MISSION;
        effect.discard_missions = vec![MissionType::Guard.ini_section().to_owned()];
        effect.discard_ai_missions = Some(vec![MissionType::Hunt.ini_section().to_owned()]);
        facts = DiscardFacts {
            mission: Some(MissionType::Guard),
            owner_human: true,
            ..DiscardFacts::default()
        };
        assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
        facts.mission = Some(MissionType::Hunt);
        assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));
        facts.owner_human = false;
        assert!(instance.should_be_discarded_now(&effect, &facts, &globals));
        facts.mission = Some(MissionType::Guard);
        assert!(!instance.should_be_discarded_now(&effect, &facts, &globals));
        effect.discard_ai_missions = None;
        assert!(
            instance.should_be_discarded_now(&effect, &facts, &globals),
            "an absent AI list inherits DiscardOn.Missions"
        );

        // The event flag wins even with no condition list.
        let mut flagged = instance.clone();
        flagged.mark_discarded();
        effect.discard_on = DiscardOn::NONE;
        assert!(flagged.should_be_discarded_now(&effect, &DiscardFacts::default(), &globals));
    }

    /// The event conditions flag through their owners (a fired count, a
    /// received hit, limbo entry), and the AI walk removes the effect.
    #[test]
    fn event_discards_remove_after_their_counts() {
        use crate::rules::ini_parser::IniFile;
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=500\nSpeed=6\n\
             AttachEffect.AttachTypes=FIRE,RECEIVE,ENTRY\n\
             [AttachEffectTypes]\n0=FIRE\n1=RECEIVE\n2=ENTRY\n\
             [FIRE]\nDuration=10\nDiscardOn=firing\nDiscardOn.Firing.Count=2\n\
             [RECEIVE]\nDuration=10\nDiscardOn=receiveddamage\n\
             DiscardOn.ReceivedDamage.Count=1\n\
             [ENTRY]\nDuration=10\nDiscardOn=entry\n",
        ))
        .expect("rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let target = sim
            .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
            .expect("spawn");
        let count = |sim: &crate::sim::world::Simulation| {
            sim.entities()
                .get(target)
                .unwrap()
                .phobos_attach_effects()
                .instances()
                .len()
        };
        assert_eq!(count(&sim), 3);

        sim.record_phobos_attach_firing(target, &rules);
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(count(&sim), 3, "one shot is below Firing.Count=2");
        sim.record_phobos_attach_firing(target, &rules);
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(count(&sim), 2, "the second shot discards FIRE");

        let owner = sim.entities().get(target).unwrap().owner();
        sim.record_phobos_attach_damage(target, Some(owner), &rules);
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(count(&sim), 1, "one hit discards RECEIVE");

        sim.limbo_phobos_attach_effects(
            target,
            &rules,
            None,
            crate::sim::world::FrameEffects::empty(),
        );
        assert_eq!(count(&sim), 0, "limbo entry discards ENTRY");
    }

    /// An owner change and a dead external invoker flag through their owners.
    #[test]
    fn owner_change_and_invoker_death_flag_the_effect() {
        use crate::rules::ini_parser::IniFile;
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=500\nSpeed=6\nAttachEffect.AttachTypes=OWNER\n\
             [AttachEffectTypes]\n0=OWNER\n1=DEADINV\n\
             [OWNER]\nDuration=10\nDiscardOn=ownerchange\n\
             [DEADINV]\nDuration=10\nDiscardOn=invokerdie\n",
        ))
        .expect("rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let target = sim
            .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
            .expect("spawn");
        // A second effect attached from outside names an invoker that is gone.
        let catalog = rules.attach_effect_types();
        let effect = &catalog.types()[1];
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .phobos_attach_effects_mut()
            .attach(
                1,
                AttachEffectParams {
                    delay: -1,
                    recreation_delay: -1,
                    ..AttachEffectParams::default()
                },
                effect,
                AttachPolicy::default(),
                Some(AttachSource {
                    invoker: Some(999_999),
                    source: None,
                    invoker_house: None,
                }),
            );
        assert_eq!(
            sim.entities()
                .get(target)
                .unwrap()
                .phobos_attach_effects()
                .instances()
                .len(),
            2
        );
        sim.update_phobos_attach_effects(target, &rules);
        let instances = sim
            .entities()
            .get(target)
            .unwrap()
            .phobos_attach_effects()
            .instances();
        assert_eq!(instances.len(), 1, "the dead invoker discards DEADINV");
        assert_eq!(instances[0].type_index, 0);

        let russians = sim.interner().get("Russians").expect("rival house");
        sim.change_owner_with_rules(
            target,
            russians,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        sim.update_phobos_attach_effects(target, &rules);
        assert!(
            sim.entities()
                .get(target)
                .unwrap()
                .phobos_attach_effects()
                .is_empty(),
            "the human-to-computer owner change discards OWNER"
        );
    }

    /// The sequence condition reads the infantry's current Doing; the
    /// immediate list form matches at once and the deferred form matches the
    /// previous sequence after the next one starts.
    #[test]
    fn sequence_discard_reads_the_infantry_doing() {
        use crate::rules::ini_parser::IniFile;
        let scene = |immediate: &str| {
            let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(&format!(
                "[Countries]\n0=Americans\n[InfantryTypes]\n0=E1\n\
                 [E1]\nStrength=100\nSpeed=4\nAttachEffect.AttachTypes=SEQ\n\
                 [AttachEffectTypes]\n0=SEQ\n\
                 [SEQ]\nDuration=10\nDiscardOn=sequence\n\
                 DiscardOn.Sequences=walk\nDiscardOn.Sequences.Immediate={immediate}\n"
            )))
            .expect("rules");
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let target = sim
                .spawn_object("E1", "Americans", 20, 20, 0, &rules)
                .expect("spawn");
            (sim, rules, target)
        };
        let count = |sim: &crate::sim::world::Simulation, id: u64| {
            sim.entities()
                .get(id)
                .unwrap()
                .phobos_attach_effects()
                .instances()
                .len()
        };

        let (mut sim, rules, target) = scene("yes");
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .mission_leaf
            .set_infantry_doing_verified(3)
            .expect("Walk action");
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(count(&sim, target), 0, "the immediate form matches Walk");

        let (mut sim, rules, target) = scene("no");
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .mission_leaf
            .set_infantry_doing_verified(3)
            .expect("Walk action");
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(count(&sim, target), 1, "the deferred form waits");
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .mission_leaf
            .set_infantry_doing_verified(0)
            .expect("Ready action");
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(
            count(&sim, target),
            0,
            "leaving the matching sequence discards"
        );
    }

    /// `DiscardOn=outofrange` reads the live held target; a zero-cell override
    /// makes any positive distance discard.
    #[test]
    fn out_of_range_discard_reads_the_held_target() {
        use crate::rules::ini_parser::IniFile;
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
             [WeaponTypes]\n0=GUN\n[GUN]\nRange=5\nDamage=10\nROF=20\n\
             [TARGET]\nStrength=500\nSpeed=6\nPrimary=GUN\n\
             AttachEffect.AttachTypes=OUT\n\
             [AttachEffectTypes]\n0=OUT\n\
             [OUT]\nDuration=10\nDiscardOn=outofrange\nDiscardOn.RangeOverride=0\n",
        ))
        .expect("rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let target = sim
            .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
            .expect("spawn");
        assert!(
            sim.assign_target_represented(
                target,
                Some(crate::sim::combat::TargetKind::Cell(30, 30)),
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            )
            .is_ok()
        );
        sim.update_phobos_attach_effects(target, &rules);
        assert!(
            sim.entities()
                .get(target)
                .unwrap()
                .phobos_attach_effects()
                .is_empty(),
            "a positive distance is outside the zero range"
        );
    }

    /// `DiscardOn=ammo` erases the effect on the AI frame that observes the
    /// target's rounds inside the interval.
    #[test]
    fn an_ammo_discard_removes_the_effect() {
        use crate::rules::ini_parser::IniFile;
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=500\nSpeed=6\nAmmo=0\n\
             AttachEffect.AttachTypes=AE_AMMO\n\
             [AttachEffectTypes]\n0=AE_AMMO\n\
             [AE_AMMO]\nDuration=10\nDiscardOn=ammo\n\
             DiscardOn.Ammo.MinimumAmount=0\nDiscardOn.Ammo.MaximumAmount=0\n",
        ))
        .expect("rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let target = sim
            .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
            .expect("spawn");
        assert_eq!(
            sim.entities()
                .get(target)
                .unwrap()
                .phobos_attach_effects()
                .instances()
                .len(),
            1
        );
        sim.update_phobos_attach_effects(target, &rules);
        assert!(
            sim.entities()
                .get(target)
                .unwrap()
                .phobos_attach_effects()
                .is_empty(),
            "the empty magazine discards the effect"
        );
    }

    /// `DiscardOn=drain` reads the victim side of the Floating Disc link
    /// (`TechnoClass+0x1D0 DrainingMe`).
    #[test]
    fn a_drain_link_discards_the_effect() {
        use crate::rules::ini_parser::IniFile;
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=500\nSpeed=6\n\
             AttachEffect.AttachTypes=AE_DRAIN\n\
             [AttachEffectTypes]\n0=AE_DRAIN\n\
             [AE_DRAIN]\nDuration=10\nDiscardOn=drain\n",
        ))
        .expect("rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let target = sim
            .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
            .expect("spawn");
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(
            sim.entities()
                .get(target)
                .unwrap()
                .phobos_attach_effects()
                .instances()
                .len(),
            1,
            "no drainer keeps the effect"
        );
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .drain
            .set_draining_me_for_test(Some(999));
        sim.update_phobos_attach_effects(target, &rules);
        assert!(
            sim.entities()
                .get(target)
                .unwrap()
                .phobos_attach_effects()
                .is_empty(),
            "the drained victim discards the effect"
        );
    }

    /// `RecalculateStatMultipliers` folds every active armor multiplier into
    /// the cached factor, and the receiver's extra divide consumes it after
    /// the first AI frame.
    #[test]
    fn active_effects_fold_into_the_receiver_armor_divider() {
        use crate::rules::ini_parser::IniFile;
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
             [TARGET]\nStrength=1000\nSpeed=6\nAttachEffect.AttachTypes=ARMORA,ARMORB\n\
             [AttachEffectTypes]\n0=ARMORA\n1=ARMORB\n\
             [ARMORA]\nDuration=10\nArmorMultiplier=0.5\n\
             [ARMORB]\nDuration=10\nArmorMultiplier=0.5\n",
        ))
        .expect("rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let target = sim
            .spawn_object("TARGET", "Americans", 20, 20, 0, &rules)
            .expect("spawn");
        assert_eq!(
            sim.entities()
                .get(target)
                .unwrap()
                .phobos_armor_multiplier(),
            1.0,
            "the fold starts at identity until the first AI frame"
        );
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(
            sim.entities()
                .get(target)
                .unwrap()
                .phobos_armor_multiplier(),
            0.25,
            "two active effects stack"
        );
        let divisors = crate::sim::combat::receiver_divisors(
            sim.entities().get(target).unwrap(),
            rules.object("TARGET"),
            &rules,
            sim.interner(),
            sim.houses(),
            sim.entities()
                .get(target)
                .unwrap()
                .phobos_armor_multiplier(),
        );
        assert_eq!(divisors.ae_armor, 0.25);
        assert_eq!(
            crate::sim::combat::damage::receive::defence_divides(100, &divisors),
            400,
            "the stock divide then the effect divide"
        );
    }

    /// `TechnoClass::EstimateDamage`'s Phobos divide (`0x006FDC87`) reads
    /// `CalculateArmorMultipliers(pTarget, warhead, pThis->Owner)`, so the
    /// target's effect armor applies, not the firer's.
    #[test]
    fn the_damage_estimate_divides_by_the_targets_effect_armor() {
        use crate::rules::ini_parser::IniFile;
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[Countries]\n0=Americans\n[VehicleTypes]\n0=FIRER\n1=TARGET\n\
             [WeaponTypes]\n0=GUN\n[GUN]\nDamage=100\nRange=5\nROF=20\nWarhead=WH\n\
             [WarheadTypes]\n0=WH\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [FIRER]\nStrength=500\nSpeed=6\nPrimary=GUN\n\
             AttachEffect.AttachTypes=FIRER_ARMOR\n\
             [TARGET]\nStrength=500\nSpeed=6\n\
             AttachEffect.AttachTypes=TARGET_ARMOR\n\
             [AttachEffectTypes]\n0=FIRER_ARMOR\n1=TARGET_ARMOR\n\
             [FIRER_ARMOR]\nDuration=100\nArmorMultiplier=4\n\
             [TARGET_ARMOR]\nDuration=100\nArmorMultiplier=2\n",
        ))
        .expect("rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let firer = sim
            .spawn_object("FIRER", "Americans", 20, 20, 0, &rules)
            .expect("firer");
        let target = sim
            .spawn_object("TARGET", "Americans", 22, 20, 0, &rules)
            .expect("target");
        sim.update_phobos_attach_effects(firer, &rules);
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(
            crate::sim::combat::estimated_damage_on(
                &sim,
                &rules,
                firer,
                target,
                rules.weapon("GUN").expect("weapon"),
            ),
            50,
            "100 damage divided by the target's ArmorMultiplier=2"
        );
    }

    /// `TechnoExt::ExtData::HasAttachedEffects(..., requireAll=false,
    /// requireAnims=true)`: a required set matches on any active attached
    /// type, and a type that declares an animation only counts while its
    /// instance holds one.
    #[test]
    fn draw_offsets_match_any_active_required_type_and_need_their_animation() {
        use crate::rules::attach_effect::AnimationDrawOffset;

        let mut host = AttachEffectType::default();
        host.name = "HOST".to_owned();
        host.duration = 10;
        host.animation_draw_offsets = vec![
            AnimationDrawOffset {
                offset: [2, -1],
                required_types: Vec::new(),
            },
            AnimationDrawOffset {
                offset: [5, 5],
                required_types: vec!["NEEDED".to_owned(), "OTHER".to_owned()],
            },
        ];
        let mut needed = AttachEffectType::default();
        needed.name = "NEEDED".to_owned();
        needed.duration = 10;
        needed.animation = Some("LOOP".to_owned());
        let mut other = AttachEffectType::default();
        other.name = "OTHER".to_owned();
        other.duration = 10;
        let effects = vec![host, needed, other];
        let attach = |list: &mut AttachEffectList, type_index| {
            list.attach(
                type_index,
                AttachEffectParams {
                    delay: -1,
                    ..AttachEffectParams::default()
                },
                &effects[type_index],
                AttachPolicy::default(),
                None,
            )
        };

        let mut list = AttachEffectList::default();
        attach(&mut list, 0);
        attach(&mut list, 1);
        list.tick_all(&effects, |_, _, _| false);
        assert_eq!(
            list.draw_offset(0, &effects),
            [2, -1],
            "the unconditional rule adds on its own; a type that declares an \
             animation does not match without one"
        );
        list.instances[1].anim = Some(9);
        assert_eq!(
            list.draw_offset(0, &effects),
            [7, 4],
            "the animated instance satisfies the rule"
        );
        // `RequiredTypes` is any-of, and a second match must not add twice.
        list.instances[1].anim = None;
        attach(&mut list, 2);
        list.tick_all(&effects, |_, _, _| false);
        assert_eq!(
            list.draw_offset(0, &effects),
            [7, 4],
            "OTHER declares no animation, so it matches on its own"
        );
        list.instances[1].anim = Some(9);
        assert_eq!(
            list.draw_offset(0, &effects),
            [7, 4],
            "a rule adds once however many listed types match"
        );

        // An ended terminal effect no longer satisfies a required type.
        let mut host = AttachEffectType::default();
        host.name = "HOST".to_owned();
        host.duration = 10;
        host.animation_draw_offsets = vec![AnimationDrawOffset {
            offset: [7, 4],
            required_types: vec!["OTHER".to_owned()],
        }];
        let mut short = AttachEffectType::default();
        short.name = "OTHER".to_owned();
        short.duration = 2;
        let short_effects = vec![host, short];
        let mut list = AttachEffectList::default();
        list.attach(
            0,
            AttachEffectParams {
                delay: -1,
                ..AttachEffectParams::default()
            },
            &short_effects[0],
            AttachPolicy::default(),
            None,
        );
        // Attached from outside: activity follows the remaining duration, so a
        // run-out effect leaves the required set even before removal.
        list.attach(
            1,
            AttachEffectParams {
                delay: -1,
                ..AttachEffectParams::default()
            },
            &short_effects[1],
            AttachPolicy::default(),
            Some(AttachSource::default()),
        );
        list.tick_all(&short_effects, |_, _, _| false);
        assert_eq!(list.draw_offset(0, &short_effects), [7, 4]);
        list.tick_all(&short_effects, |_, _, _| false);
        assert_eq!(
            list.draw_offset(0, &short_effects),
            [0, 0],
            "an expired effect is not active"
        );
    }

    /// A creation attach reads the type's list with each paired value and
    /// keeps an unknown catalog name out.
    #[test]
    fn creation_attaches_the_type_list_with_paired_values() {
        use crate::rules::ruleset::RuleSet;
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[AttachEffectTypes]
0=FIRST
[FIRST]
Duration=40
             [VehicleTypes]
0=MTNK
[MTNK]
Name=Apocalypse
             AttachEffect.AttachTypes=FIRST,MISSING
AttachEffect.InitialDelays=3
",
        ))
        .expect("rules");
        let object = rules.object("MTNK").expect("techno");
        let mut list = AttachEffectList::default();
        assert_eq!(
            attach_initial_effects(
                &mut list,
                crate::rules::object_type::ObjectCategory::Vehicle,
                None,
                object,
                &rules,
                0,
                None,
            ),
            1,
            "only the catalog name attaches"
        );
        assert_eq!(list.instances().len(), 1);
        assert_eq!(list.instances()[0].remaining(), 40);
    }

    /// `CreateAndAttach` (`AttachEffectClass.cpp:883..898`) refuses an active
    /// curtain without the matching penetration flag, then applies the class
    /// whitelist and the type lists. A Force Shield reads its own flag, which
    /// inherits the Iron Curtain one when absent.
    #[test]
    fn attach_gate_follows_curtain_class_and_type_filters() {
        use crate::rules::attach_effect::AttachEffectTargets;
        use crate::rules::object_type::ObjectCategory;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};
        use crate::sim::timer::CdTimer;

        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nName=Apocalypse\n",
        ))
        .expect("rules");
        let object = rules.object("MTNK").expect("techno");
        let mut effect = effect(40);
        assert!(can_attach_effect(
            ObjectCategory::Vehicle,
            None,
            object,
            &effect,
            0
        ));

        effect.affects_target = AttachEffectTargets {
            infantry: false,
            units: false,
            aircraft: false,
            buildings: true,
        };
        assert!(
            !can_attach_effect(ObjectCategory::Vehicle, None, object, &effect, 0),
            "a building-only effect rejects a vehicle"
        );
        effect.affects_target = AttachEffectTargets::ALL;

        effect.affect_types = vec!["OTHER".to_owned()];
        assert!(
            !can_attach_effect(ObjectCategory::Vehicle, None, object, &effect, 0),
            "the whitelist rejects an unlisted type"
        );
        effect.affect_types = vec!["mtnk".to_owned()];
        assert!(
            can_attach_effect(ObjectCategory::Vehicle, None, object, &effect, 0),
            "the whitelist matches its id case-insensitively"
        );
        effect.affect_types.clear();
        effect.ignore_types = vec!["MTNK".to_owned()];
        assert!(
            !can_attach_effect(ObjectCategory::Vehicle, None, object, &effect, 0),
            "the blacklist wins after the whitelist"
        );
        effect.ignore_types.clear();

        let curtain = InvulnerabilityState::new(CdTimer::started(0, 100), InvulnKind::IronCurtain);
        assert!(
            !can_attach_effect(ObjectCategory::Vehicle, Some(&curtain), object, &effect, 50),
            "an active curtain blocks the attach"
        );
        effect.penetrates_iron_curtain = true;
        assert!(
            can_attach_effect(ObjectCategory::Vehicle, Some(&curtain), object, &effect, 50),
            "PenetratesIronCurtain admits the curtained target"
        );
        assert!(
            can_attach_effect(
                ObjectCategory::Vehicle,
                Some(&curtain),
                object,
                &effect,
                100
            ),
            "an expired curtain no longer blocks"
        );

        let shield = InvulnerabilityState::new(CdTimer::started(0, 100), InvulnKind::ForceShield);
        effect.penetrates_force_shield = Some(false);
        assert!(
            !can_attach_effect(ObjectCategory::Vehicle, Some(&shield), object, &effect, 50),
            "a Force Shield reads PenetratesForceShield"
        );
        effect.penetrates_force_shield = None;
        assert!(
            can_attach_effect(ObjectCategory::Vehicle, Some(&shield), object, &effect, 50),
            "an absent PenetratesForceShield inherits the iron curtain flag"
        );
    }

    /// A paired list uses its own index, else the last value, else the key's
    /// documented default.
    #[test]
    fn paired_lists_take_the_last_value_for_the_remaining_types() {
        assert_eq!(paired(&[5, 7], 0, 0), 5);
        assert_eq!(paired(&[5, 7], 1, 0), 7);
        assert_eq!(paired(&[5, 7], 4, 0), 7, "the last covers the rest");
        assert_eq!(paired(&[], 0, -1), -1, "an absent list keeps the default");
    }

    /// A re-creating effect is inactive until its initial delay clears, and a
    /// `Powered` type follows its owner's activity; a plain type does not.
    #[test]
    fn active_state_follows_delays_and_the_powered_owner() {
        let plain = effect(3);
        let mut instance = AttachEffectInstance::new(
            0,
            AttachEffectParams {
                initial_delay: 1,
                delay: 0,
                ..AttachEffectParams::default()
            },
            &plain,
        );
        instance.tick(&plain, false);
        assert!(
            !instance.is_active(false),
            "the delay frame does not initialize"
        );
        instance.tick(&plain, false);
        assert!(instance.is_active(false), "initialized, no pending delay");
        instance.online_check(&plain, false);
        assert!(instance.is_online(), "a plain type ignores its owner");

        let mut powered = effect(3);
        powered.powered = true;
        let mut instance = AttachEffectInstance::new(
            0,
            AttachEffectParams {
                delay: 0,
                ..AttachEffectParams::default()
            },
            &powered,
        );
        instance.tick(&powered, false);
        instance.online_check(&powered, false);
        assert!(
            !instance.is_active(false),
            "a powered type follows its owner"
        );
        instance.online_check(&powered, true);
        assert!(instance.is_active(false));
    }

    /// A terminal effect is active only while its duration runs; a re-creating
    /// one is inactive while its re-creation delay is pending.
    #[test]
    fn a_terminal_effect_follows_its_duration_and_a_recreating_one_its_delay() {
        let terminal = effect(1);
        let mut instance = AttachEffectInstance::new(
            0,
            AttachEffectParams {
                delay: -1,
                ..AttachEffectParams::default()
            },
            &terminal,
        );
        assert!(
            instance.is_active(false),
            "a terminal effect reads its duration"
        );
        instance.tick(&terminal, false);
        assert!(!instance.is_active(false), "duration ran out");

        let cycling = effect(1);
        let mut instance = AttachEffectInstance::new(
            0,
            AttachEffectParams {
                delay: 2,
                ..AttachEffectParams::default()
            },
            &cycling,
        );
        instance.tick(&cycling, false);
        assert_eq!(instance.current_delay(), 2);
        assert!(
            !instance.is_active(false),
            "the pending re-creation delay keeps it inactive"
        );
        instance.tick(&cycling, false);
        instance.tick(&cycling, false);
        assert!(instance.should_refresh());
        instance.tick(&cycling, false);
        assert!(instance.is_active(false), "refreshed and counting again");
    }

    /// The offline and temporal actions hide, pause or freeze the animation.
    #[test]
    fn animation_state_maps_the_offline_and_temporal_actions() {
        let mut powered = effect(3);
        powered.powered = true;
        let mut instance = AttachEffectInstance::new(
            0,
            AttachEffectParams {
                delay: -1,
                ..AttachEffectParams::default()
            },
            &powered,
        );
        instance.tick(&powered, false);
        let online = instance.animation_state(&powered, false);
        assert!(online.show && !online.paused && !online.temporal);

        instance.online_check(&powered, false);
        let offline = instance.animation_state(&powered, false);
        assert!(!offline.show, "the default OfflineAction hides");

        powered.animation_offline_action = AttachedAnimFlag::Paused;
        let offline = instance.animation_state(&powered, false);
        assert!(offline.show && offline.paused && !offline.temporal);

        powered.animation_temporal_action = AttachedAnimFlag::PausedTemporal;
        let frozen = instance.animation_state(&powered, true);
        assert!(frozen.show && frozen.paused && frozen.temporal);

        let plain = effect(3);
        let instance = AttachEffectInstance::new(
            0,
            AttachEffectParams {
                delay: -1,
                ..AttachEffectParams::default()
            },
            &plain,
        );
        let untouched = instance.animation_state(&plain, true);
        assert!(untouched.show && !untouched.paused && !untouched.temporal);
    }

    /// The list round-trips through serde with its instance state intact.
    #[test]
    fn the_list_round_trips_through_serde() {
        let effect = cumulative_effect(4, 2);
        let mut list = AttachEffectList::default();
        attach_with(&mut list, &effect, 0);
        list.tick_all(std::slice::from_ref(&effect), |_, _, _| false);
        let json = serde_json::to_string(&list).expect("serialize");
        let back: AttachEffectList = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, list);
    }

    /// A non-cumulative type never stacks: the existing instance takes the
    /// refresh, but only when the new duration wins or the site replaces
    /// longer ones.
    #[test]
    fn reapplying_a_non_cumulative_type_refreshes_by_the_longer_rule() {
        let effect = effect(5);
        let mut list = AttachEffectList::default();
        assert_eq!(
            attach_with(&mut list, &effect, 0),
            AttachOutcome::Attached(0)
        );
        list.tick_all(std::slice::from_ref(&effect), |_, _, _| false);
        list.tick_all(std::slice::from_ref(&effect), |_, _, _| false);
        assert_eq!(list.instances()[0].remaining(), 3);

        assert_eq!(attach_with(&mut list, &effect, 2), AttachOutcome::Refreshed);
        assert_eq!(list.instances().len(), 1, "no second instance");
        assert_eq!(
            list.instances()[0].remaining(),
            3,
            "a shorter reapply is ignored"
        );

        let mut list = AttachEffectList::default();
        attach_with(&mut list, &effect, 0);
        let replace = AttachPolicy {
            replace_longer_duration: true,
            ..AttachPolicy::default()
        };
        let outcome = list.attach(
            0,
            AttachEffectParams {
                duration_override: 2,
                delay: -1,
                ..AttachEffectParams::default()
            },
            &effect,
            replace,
            None,
        );
        assert_eq!(outcome, AttachOutcome::Refreshed);
        assert_eq!(
            list.instances()[0].remaining(),
            2,
            "replaced even though shorter"
        );
    }

    /// Below `Cumulative.MaxCount` the list stacks; at the cap it refreshes
    /// the shortest match instead of appending.
    #[test]
    fn cumulative_stacks_below_the_cap_and_refreshes_the_shortest_at_it() {
        let effect = cumulative_effect(6, 2);
        let mut list = AttachEffectList::default();
        attach_with(&mut list, &effect, 6);
        attach_with(&mut list, &effect, 2);
        assert_eq!(list.instances().len(), 2, "below the cap both attach");

        assert_eq!(attach_with(&mut list, &effect, 3), AttachOutcome::Refreshed);
        assert_eq!(list.instances().len(), 2, "the cap refuses a third");
        assert_eq!(
            list.instances()[0].remaining(),
            6,
            "the longer match is untouched"
        );
        assert_eq!(
            list.instances()[1].remaining(),
            3,
            "the shortest match refreshed"
        );
    }

    /// `CumulativeRefreshAll` refreshes every match at the cap.
    #[test]
    fn refresh_all_at_the_cap_refreshes_every_match() {
        let effect = cumulative_effect(6, 2);
        let mut list = AttachEffectList::default();
        attach_with(&mut list, &effect, 6);
        attach_with(&mut list, &effect, 2);
        let policy = AttachPolicy {
            cumulative_refresh_all: true,
            ..AttachPolicy::default()
        };
        let outcome = list.attach(
            0,
            AttachEffectParams {
                duration_override: 3,
                delay: -1,
                ..AttachEffectParams::default()
            },
            &effect,
            policy,
            None,
        );
        assert_eq!(outcome, AttachOutcome::Refreshed);
        assert_eq!(list.instances()[0].remaining(), 3);
        assert_eq!(list.instances()[1].remaining(), 3);
    }

    /// `CumulativeRefreshAll.OnAttach` refreshes under the cap and still adds
    /// the new instance.
    #[test]
    fn refresh_all_on_attach_below_the_cap_still_appends() {
        let effect = cumulative_effect(5, 3);
        let mut list = AttachEffectList::default();
        attach_with(&mut list, &effect, 5);
        let policy = AttachPolicy {
            cumulative_refresh_all: true,
            cumulative_refresh_all_on_attach: true,
            ..AttachPolicy::default()
        };
        let outcome = list.attach(
            0,
            AttachEffectParams {
                duration_override: 2,
                delay: -1,
                ..AttachEffectParams::default()
            },
            &effect,
            policy,
            None,
        );
        assert_eq!(outcome, AttachOutcome::Attached(1));
        assert_eq!(list.instances()[0].remaining(), 2, "the match refreshed");
        assert_eq!(
            list.instances()[1].remaining(),
            2,
            "the new instance starts at its own"
        );
    }

    /// `CumulativeRefreshSameSourceOnly` keeps another source's instance out of
    /// the match set, so the cap is judged separately per source.
    #[test]
    fn same_source_only_skips_other_sources() {
        let effect = cumulative_effect(4, 2);
        let mut list = AttachEffectList::default();
        let first = Some(AttachSource {
            invoker: Some(1),
            source: Some(10),
            invoker_house: None,
        });
        let second = Some(AttachSource {
            invoker: Some(2),
            source: Some(20),
            invoker_house: None,
        });
        let policy = AttachPolicy::default();
        list.attach(
            0,
            AttachEffectParams {
                delay: -1,
                ..AttachEffectParams::default()
            },
            &effect,
            policy,
            first,
        );
        list.attach(
            0,
            AttachEffectParams {
                delay: -1,
                ..AttachEffectParams::default()
            },
            &effect,
            policy,
            second,
        );
        assert_eq!(
            list.instances().len(),
            2,
            "a different source still attaches"
        );
        assert_eq!(
            list.instances()[1].source,
            second,
            "the instance keeps who attached it"
        );
    }

    /// A source cap refuses the new instance even when the type cap allows it;
    /// a different retained house is still the same source.
    #[test]
    fn the_source_cap_refreshes_instead_of_appending() {
        let effect = cumulative_effect(4, -1);
        let mut list = AttachEffectList::default();
        let source = Some(AttachSource {
            invoker: Some(1),
            source: Some(10),
            invoker_house: None,
        });
        let policy = AttachPolicy {
            cumulative_source_max_count: 1,
            ..AttachPolicy::default()
        };
        list.attach(
            0,
            AttachEffectParams {
                delay: -1,
                ..AttachEffectParams::default()
            },
            &effect,
            policy,
            source,
        );
        let outcome = list.attach(
            0,
            AttachEffectParams {
                delay: -1,
                ..AttachEffectParams::default()
            },
            &effect,
            policy,
            source.map(|source| AttachSource {
                invoker_house: Some(crate::sim::intern::InternedId::from_index(3)),
                ..source
            }),
        );
        assert_eq!(outcome, AttachOutcome::Refreshed);
        assert_eq!(list.instances().len(), 1, "the source cap blocks the copy");
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

    const ALL_HIT: &str = "Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";

    /// Phobos `ExpireWeapon.TriggerOn=expire`: a terminal effect fires its
    /// weapon at its own owner when the duration runs out. The weapon is named
    /// only by the AttachEffect key, so this also covers the catalog's type
    /// factory keeping it.
    #[test]
    fn an_expiring_effect_fires_its_expire_weapon_at_its_owner() {
        let (mut sim, rules) = arena_scene(
            &format!(
                "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
                 [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nDamageSelf=yes\n\
                 AttachEffect.AttachTypes=EXPIRES\nAttachEffect.Delays=-1\n\
                 [HIT]\n{ALL_HIT}\
                 [EXPW]\nDamage=5\nRange=1\nWarhead=HIT\n\
                 [AttachEffectTypes]\n0=EXPIRES\n\
                 [EXPIRES]\nDuration=1\nExpireWeapon=EXPW\n"
            ),
            17,
        );
        assert!(rules.weapon("EXPW").is_some(), "the AE key allocated EXPW");
        assert_eq!(
            rules.weapon("EXPW").unwrap().warhead.as_deref(),
            Some("HIT")
        );
        let target = spawn(&mut sim, &rules, "TARGET", "Americans", 20, 20);
        assert_eq!(health(&sim, target), 1000);

        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(attach_count(&sim, target), 0, "the effect ended");
        assert_eq!(
            health(&sim, target),
            995,
            "the expire weapon detonated at the owner"
        );
    }

    /// Phobos `BulletExt::Detonate`'s `pFiringHouse`: an expire shot whose
    /// invoker Techno is gone keeps the House that attached the effect, so
    /// the detonation's `AffectsAllies` gate still sees it.
    #[test]
    fn an_expired_invokers_shot_keeps_the_attaching_house() {
        let run = |attacker_house: &str, remove_attacker: bool| -> i32 {
            let (mut sim, rules) = arena_scene(
                &format!(
                    "[Countries]\n0=Americans\n1=Russians\n\
                     [VehicleTypes]\n0=SOURCE\n1=TARGET\n\
                     [SOURCE]\nStrength=1000\nSpeed=6\nArmor=none\n\
                     [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\n\
                     [Warheads]\n0=DROP\n1=GATE\n\
                     [DROP]\nAttachEffect.AttachTypes=EXPIRES\n{ALL_HIT}\
                     [GATE]\nAffectsAllies=no\n{ALL_HIT}\
                     [EXPW]\nDamage=5\nRange=1\nWarhead=GATE\n\
                     [AttachEffectTypes]\n0=EXPIRES\n\
                     [EXPIRES]\nDuration=1\nExpireWeapon=EXPW\n\
                     ExpireWeapon.UseInvokerAsOwner=yes\n"
                ),
                41,
            );
            let target = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
            let attacker = spawn(&mut sim, &rules, "SOURCE", attacker_house, 20, 20);
            let house = sim.interner.intern(attacker_house);
            let warhead = sim.interner.intern("DROP");
            sim.apply_warhead_attach_effects(
                target,
                warhead,
                Some(attacker),
                Some(house),
                &rules,
                crate::sim::world::FrameEffects::empty(),
                None,
            );
            assert_eq!(attach_count(&sim, target), 1, "{attacker_house}: attached");
            if remove_attacker {
                sim.substrate.entities.remove(attacker);
            }
            sim.update_phobos_attach_effects(target, &rules);
            assert_eq!(attach_count(&sim, target), 0, "{attacker_house}: expired");
            health(&sim, target)
        };

        assert_eq!(
            run("Russians", false),
            995,
            "a live enemy invoker's house passes the gate"
        );
        assert_eq!(
            run("Americans", true),
            1000,
            "the house retained for the expired invoker still blocks the shot"
        );
    }

    /// Phobos `RevengeWeapon`: a killed effect owner fires its weapon at the
    /// killing source, through the ordinary receiver and detonation paths.
    #[test]
    fn a_dying_effect_owner_fires_its_revenge_weapon_at_the_killer() {
        let (mut sim, rules) = arena_scene(
            &format!(
                "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
                 [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
                 [TARGET]\nStrength=50\nSpeed=6\nArmor=none\nAttachEffect.AttachTypes=REV\n\
                 [HIT]\n{ALL_HIT}\
                 [KILLW]\nDamage=100\nRange=1\nWarhead=HIT\n\
                 [REVW]\nDamage=7\nRange=1\nWarhead=HIT\n\
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
            crate::sim::world::FrameEffects::empty(),
        );
        assert!(
            sim.substrate.entities.get(victim).is_none() || health(&sim, victim) == 0,
            "the victim died"
        );
        assert_eq!(
            health(&sim, attacker),
            993,
            "the revenge weapon hit the killer for 7"
        );
    }

    /// Phobos `TechnoClass_ReceiveDamage_AttachEffectExpireWeapon` (`0x702050`):
    /// a dying effect owner fires `ExpireWeapon.TriggerOn=death` at itself
    /// before the death weapon; two cumulative instances with
    /// `CumulativeOnlyOnce` fire once, and a type without the death trigger
    /// does not fire at all.
    #[test]
    fn a_dying_effect_owner_fires_its_death_expire_weapon_once() {
        let (mut sim, rules) = arena_scene(
            &format!(
                "[Countries]\n0=Americans\n1=Russians\n\
                 [VehicleTypes]\n0=ATTACKER\n1=TARGET\n2=BYSTANDER\n\
                 [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
                 [TARGET]\nStrength=20\nSpeed=6\nArmor=none\n\
                 AttachEffect.AttachTypes=DEATH,DEATH,OTHER\n\
                 [BYSTANDER]\nStrength=1000\nSpeed=6\nArmor=none\n\
                 [KHIT]\n{ALL_HIT}\
                 [DHIT]\nCellSpread=1\nAffectsAllies=yes\n{ALL_HIT}\
                 [KILLW]\nDamage=100\nRange=1\nWarhead=KHIT\n\
                 [DEATHW]\nDamage=7\nRange=1\nWarhead=DHIT\n\
                 [AttachEffectTypes]\n0=DEATH\n1=OTHER\n\
                 [DEATH]\nDuration=-1\nExpireWeapon=DEATHW\nExpireWeapon.TriggerOn=death\n\
                 Cumulative=yes\nExpireWeapon.CumulativeOnlyOnce=yes\n\
                 [OTHER]\nDuration=-1\nExpireWeapon=DEATHW\n"
            ),
            31,
        );
        let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
        let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
        let bystander = spawn(&mut sim, &rules, "BYSTANDER", "Americans", 21, 21);
        sim.update_phobos_attach_effects(victim, &rules);
        assert_eq!(attach_count(&sim, victim), 3);
        assert_eq!(health(&sim, bystander), 1000);

        let warhead = sim.interner.intern("KHIT");
        let event = direct_hit(victim, 100, attacker, warhead);
        let _ = sim.commit_direct_damage_receiver(
            &rules,
            None,
            event,
            crate::sim::world::FrameEffects::empty(),
        );
        assert_eq!(
            health(&sim, bystander),
            993,
            "only the once-per-type death shot reached the bystander"
        );
    }

    /// Phobos `ReflectDamage`: the `.Override` fixed value and the
    /// `.Multiplier` form of the defense-divided received damage.
    #[test]
    fn reflect_damage_returns_the_received_packet_by_override_or_multiplier() {
        for (keys, expected) in [
            ("ReflectDamage.Override=9\n", 991),
            ("ReflectDamage.Multiplier=0.5\n", 995),
        ] {
            let (mut sim, rules) = arena_scene(
                &format!(
                    "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
                     [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
                     [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nAttachEffect.AttachTypes=REF\n\
                     [HIT]\n{ALL_HIT}\
                     [KILLW]\nDamage=1\nRange=1\nWarhead=HIT\n\
                     [AttachEffectTypes]\n0=REF\n\
                     [REF]\nDuration=-1\nReflectDamage=yes\nReflectDamage.Warhead=HIT\n{keys}"
                ),
                29,
            );
            let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
            let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
            sim.update_phobos_attach_effects(victim, &rules);

            let warhead = sim.interner.intern("HIT");
            let event = direct_hit(victim, 10, attacker, warhead);
            let _ = sim.commit_direct_damage_receiver(
                &rules,
                None,
                event,
                crate::sim::world::FrameEffects::empty(),
            );
            assert_eq!(health(&sim, victim), 990, "{keys}");
            assert_eq!(health(&sim, attacker), expected, "{keys}");
        }
    }

    /// Phobos `ReflectDamage.Delay` (`Hooks.ReceiveDamage.cpp:429,461`): a
    /// successful reflection starts the per-instance timer; a hit inside it is
    /// not reflected, and one after it is.
    #[test]
    fn reflect_damage_delay_blocks_the_next_hit_until_it_runs_out() {
        let (mut sim, rules) = arena_scene(
            &format!(
                "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
                 [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
                 [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nAttachEffect.AttachTypes=REF\n\
                 [HIT]\n{ALL_HIT}\
                 [KILLW]\nDamage=1\nRange=1\nWarhead=HIT\n\
                 [AttachEffectTypes]\n0=REF\n\
                 [REF]\nDuration=-1\nReflectDamage=yes\nReflectDamage.Warhead=HIT\n\
                 ReflectDamage.Override=7\nReflectDamage.Delay=5\n"
            ),
            37,
        );
        let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
        let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
        sim.update_phobos_attach_effects(victim, &rules);
        let warhead = sim.interner.intern("HIT");
        let hit = |sim: &mut Simulation| {
            let event = direct_hit(victim, 1, attacker, warhead);
            let _ = sim.commit_direct_damage_receiver(
                &rules,
                None,
                event,
                crate::sim::world::FrameEffects::empty(),
            );
        };
        hit(&mut sim);
        assert_eq!(health(&sim, attacker), 993, "the first hit reflects");
        hit(&mut sim);
        assert_eq!(
            health(&sim, attacker),
            993,
            "the running delay blocks the second"
        );
        sim.session.set_binary_frame_for_test(6);
        hit(&mut sim);
        assert_eq!(
            health(&sim, attacker),
            986,
            "after the delay it reflects again"
        );
    }

    /// Phobos `SuppressReflectDamage`: a bare key suppresses every reflecting
    /// effect, so the hit deals no reflected damage.
    #[test]
    fn suppress_reflect_damage_stops_the_return_hit() {
        let (mut sim, rules) = arena_scene(
            &format!(
                "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
                 [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
                 [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nAttachEffect.AttachTypes=REF\n\
                 [HIT]\n{ALL_HIT}SuppressReflectDamage=yes\n\
                 [KILLW]\nDamage=1\nRange=1\nWarhead=HIT\n\
                 [AttachEffectTypes]\n0=REF\n\
                 [REF]\nDuration=-1\nReflectDamage=yes\nReflectDamage.Warhead=HIT\n\
                 ReflectDamage.Override=9\n"
            ),
            31,
        );
        let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
        let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
        sim.update_phobos_attach_effects(victim, &rules);

        let warhead = sim.interner.intern("HIT");
        let event = direct_hit(victim, 10, attacker, warhead);
        let _ = sim.commit_direct_damage_receiver(
            &rules,
            None,
            event,
            crate::sim::world::FrameEffects::empty(),
        );
        assert_eq!(health(&sim, victim), 990);
        assert_eq!(health(&sim, attacker), 1000, "the suppressor refused it");
    }

    /// The reflected hit's own warhead is marked `Reflected` while it lands,
    /// so two reflectors carrying the same warhead cannot ping-pong.
    #[test]
    fn a_reflected_hit_never_reflects_again() {
        let (mut sim, rules) = arena_scene(
            &format!(
                "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
                 [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
                 AttachEffect.AttachTypes=REF\n\
                 [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nAttachEffect.AttachTypes=REF\n\
                 [HIT]\n{ALL_HIT}\
                 [ECHO]\n{ALL_HIT}\
                 [KILLW]\nDamage=1\nRange=1\nWarhead=HIT\n\
                 [AttachEffectTypes]\n0=REF\n\
                 [REF]\nDuration=-1\nReflectDamage=yes\nReflectDamage.Warhead=ECHO\n"
            ),
            37,
        );
        let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
        let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
        sim.update_phobos_attach_effects(attacker, &rules);
        sim.update_phobos_attach_effects(victim, &rules);

        let warhead = sim.interner.intern("HIT");
        let event = direct_hit(victim, 10, attacker, warhead);
        let _ = sim.commit_direct_damage_receiver(
            &rules,
            None,
            event,
            crate::sim::world::FrameEffects::empty(),
        );
        assert_eq!(health(&sim, victim), 990, "the incoming hit only");
        assert_eq!(
            health(&sim, attacker),
            990,
            "one reflected ECHO hit, and its own reflection was refused"
        );
    }

    /// Phobos `0x701E18` runs after Object's body (`0x00701DF8`): the packet
    /// it reflects is the post-Verses `*pDamage`, not the divided input.
    #[test]
    fn reflect_damage_returns_the_post_verses_packet() {
        let (mut sim, rules) = arena_scene(
            &format!(
                "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
                 [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
                 [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nAttachEffect.AttachTypes=REF\n\
                 [HIT]\n{ALL_HIT}\
                 [HALF]\nVerses=50%,50%,50%,50%,50%,50%,50%,50%,50%,50%,50%\n\
                 [KILLW]\nDamage=1\nRange=1\nWarhead=HALF\n\
                 [AttachEffectTypes]\n0=REF\n\
                 [REF]\nDuration=-1\nReflectDamage=yes\nReflectDamage.Warhead=HIT\n\
                 ReflectDamage.Multiplier=1.0\n"
            ),
            29,
        );
        let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
        let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
        sim.update_phobos_attach_effects(victim, &rules);
        let event = direct_hit(victim, 10, attacker, sim.interner.intern("HALF"));
        let _ = sim.commit_direct_damage_receiver(
            &rules,
            None,
            event,
            crate::sim::world::FrameEffects::empty(),
        );
        assert_eq!(health(&sim, victim), 995, "50% of 10");
        assert_eq!(
            health(&sim, attacker),
            995,
            "the reflected 5, not the 10 sent"
        );
    }

    /// The same hook counts `DiscardOn=ReceivedDamage`: a healing packet does
    /// not count, an IgnoreDefenses hit does.
    #[test]
    fn received_damage_discard_counts_positive_hits_with_ignore_defenses() {
        let (mut sim, rules) = arena_scene(
            &format!(
                "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
                 [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=KILLW\n\
                 [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nAttachEffect.AttachTypes=RECEIVE\n\
                 [HIT]\n{ALL_HIT}\
                 [KILLW]\nDamage=1\nRange=1\nWarhead=HIT\n\
                 [AttachEffectTypes]\n0=RECEIVE\n\
                 [RECEIVE]\nDuration=-1\nDiscardOn=receiveddamage\n\
                 DiscardOn.ReceivedDamage.Count=1\n"
            ),
            29,
        );
        let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
        let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 21, 20);
        sim.update_phobos_attach_effects(victim, &rules);
        let warhead = sim.interner.intern("HIT");
        let hit = |sim: &mut Simulation, damage: i32, ignore_defenses: bool| {
            let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                victim,
                damage,
                0,
                attacker,
                None,
                warhead,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses,
                    arg6: false,
                },
            );
            let _ = sim.commit_direct_damage_receiver(
                &rules,
                None,
                event,
                crate::sim::world::FrameEffects::empty(),
            );
            sim.update_phobos_attach_effects(victim, &rules);
            attach_count(sim, victim)
        };
        assert_eq!(hit(&mut sim, -10, false), 1, "a heal is not damage");
        assert_eq!(hit(&mut sim, 10, true), 0, "IgnoreDefenses still counts");
    }

    /// Phobos `TechnoClass_Limbo_Wrapper`: a removed `DiscardOn=Entry` effect
    /// leaves the cached stats and fires its `Discard` expire weapon at the
    /// object before the Limbo body.
    #[test]
    fn limbo_entry_discard_recalculates_and_fires_the_discard_weapon() {
        let (mut sim, rules) = arena_scene(
            &format!(
                "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
                 [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nDamageSelf=yes\n\
                 AttachEffect.AttachTypes=ENTRY\n\
                 [HIT]\n{ALL_HIT}\
                 [EXPW]\nDamage=5\nRange=1\nWarhead=HIT\n\
                 [AttachEffectTypes]\n0=ENTRY\n\
                 [ENTRY]\nDuration=-1\nDiscardOn=entry\nFirepowerMultiplier=2\n\
                 ExpireWeapon=EXPW\nExpireWeapon.TriggerOn=discard\n"
            ),
            17,
        );
        let target = spawn(&mut sim, &rules, "TARGET", "Americans", 20, 20);
        sim.update_phobos_attach_effects(target, &rules);
        let firepower = |sim: &Simulation| {
            sim.substrate
                .entities
                .get(target)
                .expect("live entity")
                .phobos_stat_multipliers()
                .firepower
        };
        assert_eq!(firepower(&sim), 2.0);
        sim.limbo_phobos_attach_effects(
            target,
            &rules,
            None,
            crate::sim::world::FrameEffects::empty(),
        );
        assert_eq!(attach_count(&sim, target), 0);
        assert_eq!(firepower(&sim), 1.0, "the effect's factor is gone");
        assert_eq!(health(&sim, target), 995, "the discard weapon detonated");
    }

    /// Phobos `AttachEffect.RequiredTypes` on a weapon: without the effect on
    /// the target GetFireError returns `Cant`; with it the same shot is legal.
    #[test]
    fn a_weapon_attach_effect_requirement_gates_the_fire_error() {
        use crate::sim::combat::fire_error::FireError;

        let build = |attach: bool, weapon_extra: &str| {
            let (sim, rules) = arena_scene(
                &format!(
                    "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=ATTACKER\n1=TARGET\n\
                     [ATTACKER]\nStrength=1000\nSpeed=6\nArmor=none\nPrimary=GATEW\n\
                     [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\n{}\
                     [HIT]\n{ALL_HIT}\
                     [GATEW]\nDamage=10\nRange=-2\nWarhead=HIT\nOmniFire=yes\n{weapon_extra}\
                     [AttachEffectTypes]\n0=REQ\n1=OTHER\n[REQ]\nDuration=-1\n[OTHER]\nDuration=-1\n",
                    if attach {
                        "AttachEffect.AttachTypes=REQ\n"
                    } else {
                        ""
                    }
                ),
                41,
            );
            (sim, rules)
        };
        let run = |attach: bool, weapon_extra: &str| {
            let (mut sim, rules) = build(attach, weapon_extra);
            let attacker = spawn(&mut sim, &rules, "ATTACKER", "Russians", 20, 20);
            let victim = spawn(&mut sim, &rules, "TARGET", "Americans", 23, 20);
            if attach {
                sim.update_phobos_attach_effects(victim, &rules);
            }
            crate::sim::combat::fire_error_world::FireSubject {
                world: &sim,
                rules: &rules,
                overlay_registry: None,
                fog: None,
                firer: sim.substrate.entities.get(attacker).unwrap(),
                obj: rules.object("ATTACKER").unwrap(),
                target: Some(crate::sim::combat::TargetKind::Entity(victim)),
                weapon_index: 0,
            }
            .fire_error(false)
        };

        assert_eq!(
            run(false, "AttachEffect.RequiredTypes=REQ\n"),
            FireError::Cant,
            "the target lacks REQ"
        );
        assert_eq!(
            run(true, "AttachEffect.RequiredTypes=REQ\n"),
            FireError::Ok,
            "the target carries REQ, so the shot is legal"
        );

        // `Required.Any` needs only one listed entry; the default needs all.
        assert_eq!(
            run(
                true,
                "AttachEffect.RequiredTypes=REQ,OTHER\nAttachEffect.Required.Any=yes\n"
            ),
            FireError::Ok,
            "one of the two required entries is present"
        );
        assert_eq!(
            run(true, "AttachEffect.RequiredTypes=REQ,OTHER\n"),
            FireError::Cant,
            "the default needs every required entry"
        );

        // `Disallowed.Any` defaults true; clearing it needs every entry.
        assert_eq!(
            run(true, "AttachEffect.DisallowedTypes=REQ,OTHER\n"),
            FireError::Cant,
            "any disallowed entry refuses the shot"
        );
        assert_eq!(
            run(
                true,
                "AttachEffect.DisallowedTypes=REQ,OTHER\nAttachEffect.Disallowed.Any=no\n"
            ),
            FireError::Ok,
            "requireAll disallowed needs the whole list"
        );

        // A self-owned effect does not come from the shot's source.
        assert_eq!(
            run(
                true,
                "AttachEffect.RequiredTypes=REQ\nAttachEffect.SameSourceOnly=yes\n"
            ),
            FireError::Cant,
            "the target's own effect is not from the firer"
        );
        // The instance's house is the target's own house for a self effect.
        assert_eq!(
            run(
                true,
                "AttachEffect.RequiredTypes=REQ\nAttachEffect.Required.Houses=enemies\n"
            ),
            FireError::Ok,
            "the target's house is the firer's enemy"
        );
        assert_eq!(
            run(
                true,
                "AttachEffect.RequiredTypes=REQ\nAttachEffect.Required.Houses=owner\n"
            ),
            FireError::Cant,
            "the target's house is not the firer's"
        );
    }

    /// Phobos `AttachEffect.CumulativeRemoveMinCounts` / `MaxCounts` and
    /// `RemoveGroups`: the min keeps the whole stack, the max removes only
    /// that many, a later pass below the min keeps the rest, and a group
    /// removes what is left.
    #[test]
    fn warhead_removal_boundaries_follow_min_max_counts_and_groups() {
        let (mut sim, rules) = arena_scene(
            &format!(
                "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=TARGET\n\
                 [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nDamageSelf=yes\n\
                 AttachEffect.AttachTypes=STACK,STACK,STACK\nAttachEffect.Delays=-1\n\
                 [Warheads]\n0=KEEPMIN\n1=LIMITMAX\n2=LATERMIN\n3=GROUPOFF\n\
                 [KEEPMIN]\n{ALL_HIT}AttachEffect.RemoveTypes=STACK\n\
                 AttachEffect.CumulativeRemoveMinCounts=4\n\
                 [LIMITMAX]\n{ALL_HIT}AttachEffect.RemoveTypes=STACK\n\
                 AttachEffect.CumulativeRemoveMaxCounts=2\n\
                 [LATERMIN]\n{ALL_HIT}AttachEffect.RemoveTypes=STACK\n\
                 AttachEffect.CumulativeRemoveMinCounts=3\n\
                 [GROUPOFF]\n{ALL_HIT}AttachEffect.RemoveGroups=tag\n\
                 [AttachEffectTypes]\n0=STACK\n\
                 [STACK]\nDuration=100\nCumulative=yes\nGroups=tag\n"
            ),
            43,
        );
        let target = spawn(&mut sim, &rules, "TARGET", "Americans", 20, 20);
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(attach_count(&sim, target), 3, "three stacked instances");

        let detonate = |sim: &mut Simulation, warhead: &str| {
            let warhead_ref = sim.interner.intern(warhead);
            let impact = {
                let entity = sim.substrate.entities.get(target).unwrap();
                crate::sim::movement::ground_pose::object_get_coords(
                    entity,
                    sim.resolved_terrain.as_ref(),
                )
            };
            let detonation = crate::sim::projectile::ProjectileDetonation {
                projectile_id: u64::MAX,
                source_id: target,
                target: crate::sim::projectile::ProjectileTarget::Entity(target),
                impact: crate::sim::projectile::ProjectileCoord::new(impact.x, impact.y, impact.z),
                payload: crate::sim::projectile::ProjectilePayload::new(
                    1,
                    warhead_ref,
                    None,
                    false,
                ),
                reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
            };
            sim.commit_attach_effect_detonations(
                &[detonation],
                &rules,
                None,
                crate::sim::world::FrameEffects::empty(),
            );
        };

        detonate(&mut sim, "KEEPMIN");
        assert_eq!(attach_count(&sim, target), 3, "min 4 keeps the stack of 3");

        detonate(&mut sim, "LIMITMAX");
        assert_eq!(attach_count(&sim, target), 1, "max 2 removed two");

        detonate(&mut sim, "LATERMIN");
        assert_eq!(attach_count(&sim, target), 1, "min 3 keeps the last one");

        detonate(&mut sim, "GROUPOFF");
        assert_eq!(attach_count(&sim, target), 0, "the group removed the last");
    }

    /// Phobos `ExpireWeapon.TriggerOn=remove`: the weapon fires when a
    /// warhead's `AttachEffect.RemoveTypes` takes the effect off its owner.
    #[test]
    fn a_removed_effect_fires_its_remove_weapon() {
        let (mut sim, rules) = arena_scene(
            &format!(
                "[Countries]\n0=Americans\n[VehicleTypes]\n0=TARGET\n\
                 [TARGET]\nStrength=1000\nSpeed=6\nArmor=none\nDamageSelf=yes\n\
                 AttachEffect.AttachTypes=REMOVABLE\nAttachEffect.Delays=-1\n\
                 [Warheads]\n0=REMWH\n\
                 [REMWH]\n{ALL_HIT}AttachEffect.RemoveTypes=REMOVABLE\n\
                 [HIT]\n{ALL_HIT}\
                 [EXPW]\nDamage=5\nRange=1\nWarhead=HIT\n\
                 [AttachEffectTypes]\n0=REMOVABLE\n\
                 [REMOVABLE]\nDuration=100\nExpireWeapon=EXPW\n\
                 ExpireWeapon.TriggerOn=remove\n"
            ),
            47,
        );
        let target = spawn(&mut sim, &rules, "TARGET", "Americans", 20, 20);
        sim.update_phobos_attach_effects(target, &rules);
        assert_eq!(attach_count(&sim, target), 1);

        let warhead_ref = sim.interner.intern("REMWH");
        let impact = {
            let entity = sim.substrate.entities.get(target).unwrap();
            crate::sim::movement::ground_pose::object_get_coords(
                entity,
                sim.resolved_terrain.as_ref(),
            )
        };
        let detonation = crate::sim::projectile::ProjectileDetonation {
            projectile_id: u64::MAX,
            source_id: target,
            target: crate::sim::projectile::ProjectileTarget::Entity(target),
            impact: crate::sim::projectile::ProjectileCoord::new(impact.x, impact.y, impact.z),
            payload: crate::sim::projectile::ProjectilePayload::new(1, warhead_ref, None, false),
            reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
        };
        sim.commit_attach_effect_detonations(
            &[detonation],
            &rules,
            None,
            crate::sim::world::FrameEffects::empty(),
        );
        assert_eq!(attach_count(&sim, target), 0, "the warhead removed it");
        assert_eq!(
            health(&sim, target),
            994,
            "the remove weapon fired for 5 on top of the warhead's 1"
        );
    }

    /// A positive re-creation delay clears the duration and arms the delay.
    #[test]
    fn reset_arms_the_recreation_delay() {
        let effect = effect(4);
        let mut instance = AttachEffectInstance::new(
            0,
            AttachEffectParams {
                recreation_delay: 3,
                ..AttachEffectParams::default()
            },
            &effect,
        );
        assert!(instance.reset_if_recreatable());
        assert_eq!(instance.remaining(), 0);
        assert_eq!(instance.current_delay(), 3);
        assert!(instance.should_refresh(), "the queued refresh refills it");
    }

    /// `GetCritChance`'s AttachEffect fold: a multiplier scales the warhead's
    /// chance, extra chances add after it, and the Allow/Disallow lists gate
    /// the effect's contribution.
    #[test]
    fn attach_effect_crit_modifiers_fold_into_the_warhead_chance() {
        use crate::rules::ini_parser::IniFile;
        let crit_damage = |firer_ini: &str, effect_ini: &str| {
            let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(&format!(
                "[Countries]\n0=Americans\n1=Russians\n\
                 [VehicleTypes]\n0=FIRER\n1=TARGET\n\
                 [WeaponTypes]\n0=GUN\n[GUN]\nDamage=0\nRange=5\nROF=20\nWarhead=WH\n\
                 [Warheads]\n0=WH\n\
                 [WH]\nCellSpread=0\nCrit.Chance=0.05\nCrit.ExtraDamage=50\n\
                 [FIRER]\nStrength=500\nArmor=none\nSpeed=6\nPrimary=GUN\n{firer_ini}\
                 [TARGET]\nStrength=500\nArmor=none\nSpeed=6\n\
                 [AttachEffectTypes]\n0=CRIT\n\
                 [CRIT]\nDuration=100\n{effect_ini}"
            )))
            .expect("rules");
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x5A_AA_1E);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let firer = sim
                .spawn_object("FIRER", "Americans", 20, 20, 0, &rules)
                .expect("firer");
            let target = sim
                .spawn_object("TARGET", "Russians", 22, 20, 0, &rules)
                .expect("target");
            sim.update_phobos_attach_effects(firer, &rules);
            // The shot's shared dice must miss the bare 0.05 chance, so only a
            // folded chance can crit.
            let mut probe = sim.scenario_rng.clone();
            let dice = probe.random_double();
            assert!(dice > 0.05, "the seed's dice must miss the base chance");
            let coords = {
                let target = sim.entities().get(target).expect("target");
                crate::sim::movement::ground_pose::object_get_coords(
                    target,
                    sim.resolved_terrain.as_ref(),
                )
            };
            let detonation = crate::sim::projectile::ProjectileDetonation {
                projectile_id: target,
                source_id: firer,
                target: crate::sim::projectile::ProjectileTarget::Entity(target),
                impact: crate::sim::projectile::ProjectileCoord::new(coords.x, coords.y, coords.z),
                payload: crate::sim::projectile::ProjectilePayload::new(
                    0,
                    sim.interner.intern("WH"),
                    None,
                    false,
                ),
                reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
            };
            let mut run = crate::sim::combat::world_receiver::ReceiverRun::default();
            crate::sim::combat::world_receiver::commit_projectiles(
                &mut sim,
                &mut run,
                &[detonation],
                &rules,
                None,
            );
            run.finish();
            500 - sim.entities().get(target).unwrap().health.current()
        };

        assert_eq!(crit_damage("", ""), 0, "no effect keeps the base chance");
        assert_eq!(
            crit_damage("AttachEffect.AttachTypes=CRIT\n", "Crit.ExtraChance=1.0\n"),
            50,
            "ExtraChance adds after the multiplier"
        );
        assert_eq!(
            crit_damage("AttachEffect.AttachTypes=CRIT\n", "Crit.Multiplier=30\n"),
            50,
            "a multiplier scales the chance"
        );
        assert_eq!(
            crit_damage(
                "AttachEffect.AttachTypes=CRIT\n",
                "Crit.ExtraChance=1.0\nCrit.AllowWarheads=OTHER\n",
            ),
            0,
            "an unmatched AllowWarheads list skips the effect"
        );
        assert_eq!(
            crit_damage(
                "AttachEffect.AttachTypes=CRIT\n",
                "Crit.ExtraChance=1.0\nCrit.DisallowWarheads=wh\n",
            ),
            0,
            "a matching DisallowWarheads list skips the effect"
        );
    }
}
