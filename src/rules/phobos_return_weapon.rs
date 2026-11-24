//! Phobos `ReturnWeapon`: a detonating bullet that fires a second weapon back
//! at the techno that fired it.
//!
//! ```ini
//! ReturnWeapon=                     ; Valueable<WeaponTypeClass*>, unset  :54
//! ReturnWeapon.ApplyFirepowerMult=  ; Nullable<bool>, unset               :55
//! ```
//!
//! `Ext/BulletType/Body.h:56-57`, constructor `Body.h:121-122`. The reader is
//! `Read<true>` (`Body.cpp:54`), so the named weapon type is allocated when it
//! is not already known.
//!
//! `.ApplyFirepowerMult` falls back to `[CombatDamage]
//! ReturnWeapon.ApplyFirepowerMult` (default `no`, `Ext/Rules/Body.cpp:559`,
//! constructor `Ext/Rules/Body.h:973`), which
//! `RuleSet::phobos_globals.return_weapon_apply_firepower_mult` carries.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The two `ReturnWeapon` keys of a `BulletType`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PhobosReturnWeapon {
    /// `ReturnWeapon=`: the weapon fired from the impact back at the firer.
    pub return_weapon: Option<String>,
    /// `ReturnWeapon.ApplyFirepowerMult=`, `Nullable`: `None` keeps
    /// `[CombatDamage] ReturnWeapon.ApplyFirepowerMult`.
    pub apply_firepower_mult: Option<bool>,
}

impl PhobosReturnWeapon {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            return_weapon: section
                .read_type_name("ReturnWeapon", 0x800)
                .map(str::to_owned),
            apply_firepower_mult: section
                .read_phobos_nullable_bool("ReturnWeapon.ApplyFirepowerMult", None),
        }
    }

    /// `ReturnWeapon_ApplyFirepowerMult.Get(Global()->ReturnWeapon_ApplyFirepowerMult)`
    /// (`Ext/Bullet/Hooks.DetonateLogics.cpp:490`).
    pub fn applies_firepower_mult(&self, global: bool) -> bool {
        self.apply_firepower_mult.unwrap_or(global)
    }

    /// The named weapon, or `None` for a type that fires nothing back.
    pub fn weapon(&self) -> Option<&str> {
        self.return_weapon.as_deref()
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_return_weapon_tests.rs"]
mod tests;
