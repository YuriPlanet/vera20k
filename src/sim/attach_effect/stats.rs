//! Phobos stat queries over an `AttachEffectList`: the cached stat-multiplier
//! fold, active counts, group and required/disallowed type checks, and the
//! per-instance draw offset.

use crate::rules::attach_effect::AttachEffectType;

use super::{AttachEffectInstance, AttachEffectList, paired};

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
