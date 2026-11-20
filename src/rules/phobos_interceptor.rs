//! Phobos `Interceptor.*` TechnoType keys
//! (`Ext/TechnoType/Body.cpp:1415-1433`, `New/Type/Affiliated/
//! InterceptorTypeClass.cpp:5-26`, declarations and constructor defaults
//! `InterceptorTypeClass.h:9-24`).
//!
//! `Interceptor=` is a `Nullable<bool>`; only a value of `yes` creates the
//! affiliated type and reads the keys below. An explicit `no` resets it; in
//! VERA's single merged-section read an absent, blank or `none` key leaves no
//! type either (Phobos keeps the previous pass's type for a blank key).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_shields::{AffectedHouse, read_affected_house};
use crate::rules::promotable::read_promotable;

/// One TechnoType's `Interceptor.*` settings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InterceptorSettings {
    /// `Interceptor.TargetingDelay=`, `Valueable<int>`, default 1; a read of
    /// 0 is clamped to 1 (`InterceptorTypeClass.cpp:20-24`).
    pub targeting_delay: i32,
    /// `Interceptor.CanTargetHouses=`, `Valueable<AffectedHouse>`, default
    /// `Enemies`.
    pub can_target_houses: AffectedHouse,
    /// `Interceptor.%sGuardRange=`, `Promotable<Leptons>` (Rookie, Veteran,
    /// Elite), default 0.
    pub guard_range: [i32; 3],
    /// `Interceptor.%sMinimumGuardRange=`, default 0.
    pub minimum_guard_range: [i32; 3],
    /// `Interceptor.GuardRange.IsCylindrical=` (`Nullable<bool>`), falling
    /// back to `[CombatDamage]` (default no).
    pub guard_range_is_cylindrical: Option<bool>,
    /// `Interceptor.Weapon=`, `Valueable<int>`: a `GetWeapon(slot)` index.
    pub weapon: i32,
    /// `Interceptor.WeaponOverride=`, `Nullable<WeaponTypeClass*>`.
    pub weapon_override: Option<String>,
    /// `Interceptor.WeaponReplaceProjectile=`.
    pub weapon_replace_projectile: bool,
    /// `Interceptor.WeaponCumulativeDamage=`.
    pub weapon_cumulative_damage: bool,
    /// `Interceptor.KeepIntact=`.
    pub keep_intact: bool,
    /// `Interceptor.ApplyFirepowerMult=` (`Nullable<bool>`), falling back to
    /// `[CombatDamage]` (default yes).
    pub apply_firepower_mult: Option<bool>,
    /// `Interceptor.DeleteOnIntercept=` (`Nullable<bool>`), falling back to
    /// the intercepted bullet type's `Interceptable.DeleteOnIntercept`.
    pub delete_on_intercept: Option<bool>,
}

impl InterceptorSettings {
    /// `InterceptorTypeClass::LoadFromINI` for a section whose `Interceptor=`
    /// read as `yes`.
    pub fn from_section(section: &IniSection) -> Self {
        let mut targeting_delay = section.read_int("Interceptor.TargetingDelay", 1);
        if targeting_delay == 0 {
            targeting_delay = 1;
        }
        Self {
            targeting_delay,
            can_target_houses: read_affected_house(
                section,
                "Interceptor.CanTargetHouses",
                AffectedHouse::ENEMIES,
            ),
            guard_range: read_promotable("Interceptor.%sGuardRange", [0; 3], |key, value| {
                section.read_phobos_nullable_leptons(key, value)
            }),
            minimum_guard_range: read_promotable(
                "Interceptor.%sMinimumGuardRange",
                [0; 3],
                |key, value| section.read_phobos_nullable_leptons(key, value),
            ),
            guard_range_is_cylindrical: section
                .read_phobos_nullable_bool("Interceptor.GuardRange.IsCylindrical", None),
            weapon: section.read_int("Interceptor.Weapon", 0),
            weapon_override: section.read_phobos_nullable_with(
                "Interceptor.WeaponOverride",
                None,
                |raw| Some(raw.to_owned()),
            ),
            weapon_replace_projectile: section
                .read_bool("Interceptor.WeaponReplaceProjectile", false),
            weapon_cumulative_damage: section
                .read_bool("Interceptor.WeaponCumulativeDamage", false),
            keep_intact: section.read_bool("Interceptor.KeepIntact", false),
            apply_firepower_mult: section
                .read_phobos_nullable_bool("Interceptor.ApplyFirepowerMult", None),
            delete_on_intercept: section
                .read_phobos_nullable_bool("Interceptor.DeleteOnIntercept", None),
        }
    }

    /// The `Promotable::Get` value for a rank index (Rookie 0, Veteran 1,
    /// Elite 2).
    pub fn guard_range_for(&self, rank: i32) -> i32 {
        self.guard_range[rank.clamp(0, 2) as usize]
    }

    pub fn minimum_guard_range_for(&self, rank: i32) -> i32 {
        self.minimum_guard_range[rank.clamp(0, 2) as usize]
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_interceptor_tests.rs"]
mod tests;
