//! Phobos `ForceWeapon.*` / `ForceAAWeapon.*`
//! (`Ext/TechnoType/Body.cpp:905-936`).
//!
//! `TechnoTypeExt::SelectForceWeapon` (`Ext/TechnoType/Body.cpp:60-155`)
//! answers a forced weapon slot from the `0x6F3428` hook point inside the
//! native `TechnoClass::WhatWeaponShouldIUse` ladder
//! (`Ext/Techno/Hooks.Firing.cpp:94-118`); the arms before that point return
//! first. Targets are classified by state (naval decloaked, cloaked,
//! disguised, under EMP), then by an ordered in-range list that can override
//! ranges, then by class.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The twenty `TechnoTypeExt` ForceWeapon/ForceAAWeapon fields. Integer
/// weapons construct `-1` (unset), the two Nullable modifiers `None` and the
/// four vectors empty (`Ext/TechnoType/Body.h:604-624`).
#[derive(Debug, Clone, PartialEq)]
pub struct PhobosForceWeapon {
    /// `ForceWeapon.Naval.Decloaked=`: target `Cloakable`, `Naval` and
    /// Uncloaked (`Body.cpp:75-85`).
    pub naval_decloaked: i32,
    /// `ForceWeapon.Cloaked=`: target `CloakState::Cloaked` (`:86-90`).
    pub cloaked: i32,
    /// `ForceWeapon.Disguised=`: target is disguised (`:91-95`).
    pub disguised: i32,
    /// `ForceWeapon.UnderEMP=`: target is under EMP (`:96-100`).
    pub under_emp: i32,
    /// `ForceWeapon.InRange.TechnoOnly=` (`Nullable<bool>`), falling back to
    /// `[General]` (default yes).
    pub in_range_techno_only: Option<bool>,
    /// `ForceWeapon.InRange=`: ordered weapon slots (`ValueableVector<int>`).
    pub in_range: Vec<i32>,
    /// `ForceWeapon.InRange.Overrides=`: per-slot ranges in cells
    /// (`ValueableVector<double>`); non-positive entries keep the weapon's
    /// own range.
    pub in_range_overrides: Vec<f64>,
    /// `ForceWeapon.InRange.ApplyRangeModifiers=` (`Nullable<bool>`),
    /// falling back to `[General]` (default no).
    pub in_range_apply_range_modifiers: Option<bool>,
    /// `ForceAAWeapon.InRange=` (air targets only).
    pub aa_in_range: Vec<i32>,
    /// `ForceAAWeapon.InRange.Overrides=`.
    pub aa_in_range_overrides: Vec<f64>,
    /// `ForceAAWeapon.InRange.ApplyRangeModifiers=` (`Nullable<bool>`),
    /// falling back to `[General]` (default no).
    pub aa_in_range_apply_range_modifiers: Option<bool>,
    /// `ForceWeapon.Buildings=`.
    pub buildings: i32,
    /// `ForceWeapon.Defenses=`: a `BuildCat=Combat` building target.
    pub defenses: i32,
    /// `ForceWeapon.Infantry=`.
    pub infantry: i32,
    /// `ForceWeapon.Naval.Units=`: a naval Unit target.
    pub naval_units: i32,
    /// `ForceWeapon.Units=`.
    pub units: i32,
    /// `ForceWeapon.Aircraft=`.
    pub aircraft: i32,
    /// `ForceAAWeapon.Infantry=`: an airborne Infantry target.
    pub aa_infantry: i32,
    /// `ForceAAWeapon.Units=`: an airborne Unit target.
    pub aa_units: i32,
    /// `ForceAAWeapon.Aircraft=`: an airborne Aircraft target.
    pub aa_aircraft: i32,
}

impl Default for PhobosForceWeapon {
    fn default() -> Self {
        Self {
            naval_decloaked: -1,
            cloaked: -1,
            disguised: -1,
            under_emp: -1,
            in_range_techno_only: None,
            in_range: Vec::new(),
            in_range_overrides: Vec::new(),
            in_range_apply_range_modifiers: None,
            aa_in_range: Vec::new(),
            aa_in_range_overrides: Vec::new(),
            aa_in_range_apply_range_modifiers: None,
            buildings: -1,
            defenses: -1,
            infantry: -1,
            naval_units: -1,
            units: -1,
            aircraft: -1,
            aa_infantry: -1,
            aa_units: -1,
            aa_aircraft: -1,
        }
    }
}

impl PhobosForceWeapon {
    /// The reads of `TechnoTypeExt::LoadFromINIFile` (`Body.cpp:905-936`).
    /// Integer weapons are `Valueable<int>`; the vectors replace their whole
    /// list when the key is present; both modifiers are `Nullable<bool>`.
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            naval_decloaked: section.read_ares_int("ForceWeapon.Naval.Decloaked", -1),
            cloaked: section.read_ares_int("ForceWeapon.Cloaked", -1),
            disguised: section.read_ares_int("ForceWeapon.Disguised", -1),
            under_emp: section.read_ares_int("ForceWeapon.UnderEMP", -1),
            in_range_techno_only: section
                .read_phobos_nullable_bool("ForceWeapon.InRange.TechnoOnly", None),
            in_range: section
                .read_ares_int_list("ForceWeapon.InRange")
                .unwrap_or_default(),
            in_range_overrides: section
                .read_phobos_double_list("ForceWeapon.InRange.Overrides")
                .unwrap_or_default(),
            in_range_apply_range_modifiers: section
                .read_phobos_nullable_bool("ForceWeapon.InRange.ApplyRangeModifiers", None),
            aa_in_range: section
                .read_ares_int_list("ForceAAWeapon.InRange")
                .unwrap_or_default(),
            aa_in_range_overrides: section
                .read_phobos_double_list("ForceAAWeapon.InRange.Overrides")
                .unwrap_or_default(),
            aa_in_range_apply_range_modifiers: section
                .read_phobos_nullable_bool("ForceAAWeapon.InRange.ApplyRangeModifiers", None),
            buildings: section.read_ares_int("ForceWeapon.Buildings", -1),
            defenses: section.read_ares_int("ForceWeapon.Defenses", -1),
            infantry: section.read_ares_int("ForceWeapon.Infantry", -1),
            naval_units: section.read_ares_int("ForceWeapon.Naval.Units", -1),
            units: section.read_ares_int("ForceWeapon.Units", -1),
            aircraft: section.read_ares_int("ForceWeapon.Aircraft", -1),
            aa_infantry: section.read_ares_int("ForceAAWeapon.Infantry", -1),
            aa_units: section.read_ares_int("ForceAAWeapon.Units", -1),
            aa_aircraft: section.read_ares_int("ForceAAWeapon.Aircraft", -1),
        }
    }

    /// `ForceWeapon_Check`: the field cache that lets selection skip this
    /// type entirely (`Ext/TechnoType/Body.cpp:941-955`).
    pub fn any(&self) -> bool {
        self.naval_decloaked >= 0
            || self.cloaked >= 0
            || self.disguised >= 0
            || self.under_emp >= 0
            || !self.in_range.is_empty()
            || !self.aa_in_range.is_empty()
            || self.buildings >= 0
            || self.defenses >= 0
            || self.infantry >= 0
            || self.naval_units >= 0
            || self.units >= 0
            || self.aircraft >= 0
            || self.aa_infantry >= 0
            || self.aa_units >= 0
            || self.aa_aircraft >= 0
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_force_weapon_tests.rs"]
mod tests;
