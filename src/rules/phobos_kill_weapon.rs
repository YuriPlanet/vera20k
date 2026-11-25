//! Phobos `KillWeapon.*` warhead keys and `SuppressKillWeapons` techno-type
//! keys (`Ext/WarheadType/Body.cpp:371-390`, `Ext/TechnoType/Body.cpp:1125-1126`).
//!
//! Consumers: `TechnoExt::ApplyKillWeapon` (`Ext/Techno/WeaponHelpers.cpp:230`)
//! called from `TechnoClass_ReceiveDamage` (`0x702672`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_shields::AffectedHouse;

/// The six `WarheadTypeExt` kill-weapon fields, with the constructor defaults
/// `AffectedHouse::All` / `AffectedTarget::All` (`Ext/WarheadType/Body.h:541-546`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhobosKillWeaponRules {
    /// `KillWeapon=` (`Valueable<WeaponTypeClass*>`, Find only).
    pub kill_weapon: Option<String>,
    /// `KillWeapon.OnFirer=`, fired at the killing source.
    pub kill_weapon_on_firer: Option<String>,
    /// `KillWeapon.AffectsHouse=`, default all.
    pub kill_weapon_affects_house: AffectedHouse,
    /// `KillWeapon.OnFirer.AffectsHouse=`, default all.
    pub kill_weapon_on_firer_affects_house: AffectedHouse,
    /// `KillWeapon.AffectsTarget=`, default [`crate::rules::phobos_crit::AffectedTarget::ALL`].
    pub kill_weapon_affects_target: crate::rules::phobos_crit::AffectedTarget,
    /// `KillWeapon.OnFirer.AffectsTarget=`, default all.
    pub kill_weapon_on_firer_affects_target: crate::rules::phobos_crit::AffectedTarget,
}

impl Default for PhobosKillWeaponRules {
    fn default() -> Self {
        Self {
            kill_weapon: None,
            kill_weapon_on_firer: None,
            kill_weapon_affects_house: AffectedHouse::ALL,
            kill_weapon_on_firer_affects_house: AffectedHouse::ALL,
            kill_weapon_affects_target: crate::rules::phobos_crit::AffectedTarget::ALL,
            kill_weapon_on_firer_affects_target: crate::rules::phobos_crit::AffectedTarget::ALL,
        }
    }
}

impl PhobosKillWeaponRules {
    /// The read order follows `WarheadTypeExt::LoadFromINI`: the two weapon
    /// names, then each deprecated `...Houses`/`...Affects` alias before its
    /// canonical key (the canonical one wins when both are set).
    pub fn from_section(section: &IniSection) -> Self {
        use crate::rules::phobos_crit::read_affected_target;
        use crate::rules::phobos_shields::read_affected_house;
        let mut value = Self::default();
        value.kill_weapon = section
            .read_type_name("KillWeapon", 0x100)
            .map(str::to_owned);
        value.kill_weapon_on_firer = section
            .read_type_name("KillWeapon.OnFirer", 0x100)
            .map(str::to_owned);
        value.kill_weapon_affects_house = read_affected_house(
            section,
            "KillWeapon.AffectsHouses",
            value.kill_weapon_affects_house,
        );
        value.kill_weapon_affects_house = read_affected_house(
            section,
            "KillWeapon.AffectsHouse",
            value.kill_weapon_affects_house,
        );
        value.kill_weapon_on_firer_affects_house = read_affected_house(
            section,
            "KillWeapon.OnFirer.AffectsHouses",
            value.kill_weapon_on_firer_affects_house,
        );
        value.kill_weapon_on_firer_affects_house = read_affected_house(
            section,
            "KillWeapon.OnFirer.AffectsHouse",
            value.kill_weapon_on_firer_affects_house,
        );
        value.kill_weapon_affects_target = read_affected_target(
            section,
            "KillWeapon.Affects",
            value.kill_weapon_affects_target,
        );
        value.kill_weapon_affects_target = read_affected_target(
            section,
            "KillWeapon.AffectsTarget",
            value.kill_weapon_affects_target,
        );
        value.kill_weapon_on_firer_affects_target = read_affected_target(
            section,
            "KillWeapon.OnFirer.Affects",
            value.kill_weapon_on_firer_affects_target,
        );
        value.kill_weapon_on_firer_affects_target = read_affected_target(
            section,
            "KillWeapon.OnFirer.AffectsTarget",
            value.kill_weapon_on_firer_affects_target,
        );
        value
    }
}

/// `TechnoTypeExt::SuppressKillWeapons` (default false) and its
/// `SuppressKillWeapons.Types` (`ValueableVector<WeaponTypeClass*>`, whose
/// `FindOrAllocate` read keeps an unknown name as a newly allocated dummy
/// weapon; VERA keeps the name so the filter stays non-empty).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PhobosKillWeaponSuppress {
    pub suppress: bool,
    pub types: Vec<String>,
}

impl PhobosKillWeaponSuppress {
    pub fn from_section(section: &IniSection) -> Self {
        use crate::rules::native_processing::read_nullable_type_vector;
        Self {
            suppress: section.read_ares_bool("SuppressKillWeapons", false),
            types: read_nullable_type_vector(section, "SuppressKillWeapons.Types", None, |name| {
                Some(name.to_owned())
            })
            .unwrap_or_default(),
        }
    }

    /// `TechnoExt::ApplyKillWeapon`'s gate: a suppressed field is dropped
    /// unless a non-empty filter list does not name the weapon.
    pub fn suppresses(&self, weapon: &str) -> bool {
        if !self.suppress {
            return false;
        }
        self.types.is_empty()
            || self
                .types
                .iter()
                .any(|name| name.eq_ignore_ascii_case(weapon))
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_kill_weapon_tests.rs"]
mod tests;
