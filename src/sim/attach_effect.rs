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

#[cfg(feature = "phobos")]
use crate::rules::attach_effect::ExpireWeaponCondition;
use crate::rules::attach_effect::{AttachEffectType, AttachedAnimFlag};
#[cfg(feature = "phobos")]
use crate::rules::phobos_shields::AffectedHouse;
#[cfg(all(test, tests_sim_a_b, feature = "phobos"))]
use crate::sim::world::Simulation;

#[cfg(feature = "phobos")]
mod combat;
#[cfg(feature = "phobos")]
mod stats;
#[cfg(feature = "phobos")]
mod update;

#[cfg(feature = "phobos")]
pub(crate) use stats::PhobosStatMultipliers;
#[cfg(all(test, tests_sim_a_b, feature = "phobos"))]
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
    crate::sim::combat::can_target_house(flags, source, target, houses, alliances, interner)
}

#[cfg(all(test, tests_sim_a_b, feature = "phobos"))]
mod tests;
